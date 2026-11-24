//! A turn run: its lease taken, what it answers read, its answer asked
//! for and written, and its lease let go.

use super::*;

impl Harness {
    /// The turn, its lease taken as `holder`: `message` kept (none when
    /// it's written already, or there's none), a crashed turn's calls
    /// answered first, and answered. Its lease renewed while it runs,
    /// and let go after. With `[turns] busy` steer, a message steered in
    /// that the model didn't read gets another round, by this turn or
    /// the next; one steered into a turn that failed is left to whoever
    /// takes its lease once it lapses. Its measures kept whichever way
    /// it ends.
    pub(super) async fn run(
        &self,
        agent: &Agent,
        thread: &Thread,
        holder: &str,
        message: Option<&Event>,
        (pieces, told): (
            &mpsc::Sender<String>,
            &std::sync::Mutex<Option<tokio::sync::oneshot::Sender<Event>>>,
        ),
        work: Work,
    ) -> anyhow::Result<Event> {
        let config = self.config(agent).unwrap_or_default();
        let (every, steer) = (config.lease().0, busy(&config) == Busy::Steer);
        let measures = self.measures.clone().filter(|_| work == Work::Answer);
        let measuring = measures.map(|path| {
            crate::measures::Measuring::new(path, self.store.clone(), agent.clone(), thread.clone())
        });
        let watched;
        let pieces = match &measuring {
            Some(measuring) => {
                watched = measuring.clock.watch(pieces.clone());
                &watched
            }
            None => pieces,
        };
        let own = AtomicU64::new(0);
        // Whether it may tell its answer before it ends (it summarizes
        // after): once told, its caller has what it waited for.
        let telling = told.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        // Released however the turn ends, its future dropped too.
        let _released = Released(&self.compacting, agent, thread);
        // Counted held while it runs, so a stop lets it go.
        let _holds = self.holding.hold(agent, thread, holder);
        let mut message = message;
        let binding = match off_runtime(|| self.bind_work(agent, thread, holder, message)) {
            Ok(binding) => binding,
            Err(err) => {
                let _ = off_runtime(|| self.store.end_turn(agent, thread, holder));
                return Err(err);
            }
        };
        let _work_guard = self
            .interactions
            .clone()
            .zip(binding.as_ref().map(|b| b.token.clone()))
            .map(super::interactions::WorkGuard);
        let mut admit_queued = true;
        let answered = loop {
            own.store(0, Ordering::Relaxed);
            let (cancel, cancelled) = watch::channel(false);
            let interaction = self
                .interactions
                .clone()
                .zip(binding.as_ref().map(|b| b.token.clone()));
            let renewing = Renewing::start(
                &self.store,
                agent,
                thread,
                holder,
                every,
                cancel,
                interaction,
            );
            let turn = Turn {
                admit_queued,
                binding: binding.as_ref(),
                attempt: holder,
                work,
                agent,
                thread,
                pieces,
                cancelled,
                own: &own,
                measuring: measuring.as_ref(),
                told,
                whole: Default::default(),
                written: &self.written,
                cache: Default::default(),
            };
            let result = tokio::select! {
                result = self.once(&turn, message.take()) => result,
                why = renewing.lost() => Err(why),
            };
            drop(renewing);
            if let (Some(store), Some(binding)) = (&self.interactions, &binding) {
                if result.is_ok()
                    && off_runtime(|| store.waiting_inputs(binding.token.expose(), false))
                        .is_ok_and(|pending| !pending.is_empty())
                {
                    admit_queued = false;
                    continue;
                }
                off_runtime(|| {
                    store.finish_work(
                        binding.token.expose(),
                        result
                            .as_ref()
                            .ok()
                            .map(|e| format!("{}:{}", thread.name(), e.seq)),
                    )
                })?;
                if off_runtime(|| store.reenter_redirect(binding.token.expose()))? {
                    admit_queued = true;
                    continue;
                }
            }
            // A message steered in after the last the model read; failed,
            // after its own.
            let after = match &result {
                Ok(answer) => answer.seen,
                Err(_) => own.load(Ordering::Relaxed),
            };
            let waiting = steer
                && after > 0
                && off_runtime(|| self.store.read_from(agent, thread, after + 1))
                    .is_ok_and(|newer| newer.iter().any(|e| e.role == event::USER));
            if result.is_err() && waiting {
                // Its lease left to lapse: whoever takes it next answers.
                self.compacting.release(agent, thread);
                break result;
            }
            let _ = off_runtime(|| self.store.end_turn(agent, thread, holder));
            // A message from here on finds no turn running.
            self.compacting.release(agent, thread);
            // A turn that told its answer answers nothing after it: what
            // came meanwhile waits for a turn of its own, whose caller is
            // there to hear it (this one's isn't).
            let handed = telling && told.lock().unwrap_or_else(|e| e.into_inner()).is_none();
            let again = waiting
                && !handed
                && off_runtime(|| self.store.take_turn(agent, thread, holder))
                    .is_ok_and(|t| t != Took::Held);
            if !again {
                break result;
            }
            if let (Some(store), Some(binding)) = (&self.interactions, &binding) {
                off_runtime(|| store.continue_work(binding.token.expose()))?;
                admit_queued = true;
            }
        };
        if let Some(measuring) = measuring {
            measuring.end(answered.is_err());
        }
        answered
    }

    /// The conversation's events from where its window's mark goes on (all
    /// of them without one): enough to find the next number and the last
    /// answer.
    pub(super) fn tail(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Vec<Event>> {
        Ok(self.tail_read(agent, thread)?.events)
    }

    /// [`Self::tail`], as a [`Read`].
    pub(super) fn tail_read(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Read> {
        let first = self
            .store
            .mark(agent, thread)
            .map_or(1, |mark| mark.first());
        let events = self.store.read_from(agent, thread, first)?;
        // Read from the first, it's all there is, none included.
        match events.first().map_or(first == 1, |e| e.seq == first) {
            true => Ok(Read {
                events,
                from: first,
            }),
            false => Ok(Read {
                events: self.store.read(agent, thread)?,
                from: 1,
            }),
        }
    }

    /// The events written since `events` ends, added to it: what came
    /// while the turn ran.
    pub(super) fn catch_up(
        &self,
        agent: &Agent,
        thread: &Thread,
        events: &mut Vec<Event>,
    ) -> anyhow::Result<()> {
        let next = next_seq(events);
        let newer = self.store.read_from(agent, thread, next)?;
        if newer.first().is_some_and(|e| e.seq == next) {
            events.extend(newer);
        }
        Ok(())
    }

    /// One turn, its lease held: its settings, instructions and tools,
    /// then its rounds.
    async fn once(&self, turn: &Turn<'_>, message: Option<&Event>) -> anyhow::Result<Event> {
        let agent = turn.agent;
        // As the conversation it's of chose: its model, effort and fast,
        // from a read the turn's window reuses.
        let (config, read) = self.conversing_read(agent, turn.thread)?;
        // The agent's tools, or those its conversation was narrowed to; a
        // session with one only to call it, memory's kept for the turns
        // after until idle, the rest ended with the turn.
        let only = off_runtime(|| self.store.tools(agent, turn.thread))?;
        let mut toolbox = self.toolbox(agent, &config).await;
        if let Some(only) = &only {
            toolbox.only(only);
        }
        // Its prompt, from the sections its spec says, through the store,
        // and what the tools it's offered say of themselves.
        // Ended as any turn's, if it can't be read.
        let answered = match self.instructions(agent, turn.thread, &config, &toolbox) {
            Ok(instructions) => {
                self.answer(turn, message, &config, &instructions, &toolbox, &read)
                    .await
            }
            Err(err) => Err(err),
        };
        // Its answer whole, unless it was before it summarized.
        if let Some(measuring) = turn.measuring
            && !turn.whole.load(Ordering::Relaxed)
        {
            measuring.ended();
        }
        if answered.is_ok() {
            self.keep_quiet(turn, &config, &toolbox).await;
        }
        toolbox.close().await;
        self.keep_lists(agent);
        answered
    }

    /// Lists `agent`'s tools as a turn does, and keeps what each says of
    /// itself: done at a start, so the prompt `lnk agent show --instructions`
    /// prints has the tools section its first turn sends.
    pub async fn list_tools(&self, agent: &Agent) {
        let config = match self.config(agent) {
            Ok(config) => config,
            Err(err) => return crate::warn!("listing the tools: {err:#}"),
        };
        self.toolbox(agent, &config).await.close().await;
        self.keep_lists(agent);
    }

    /// `agent`'s tools, opened for a turn: what the last process listed
    /// read once, for a tool that fails to list its tools now.
    pub(super) async fn toolbox(&self, agent: &Agent, config: &Config) -> Toolbox<'_> {
        let servers = self.agents.tools(agent);
        if !servers.is_empty() && !self.listed.read(agent) {
            let kept = off_runtime(|| self.store.tool_lists(agent));
            self.listed.read_kept(agent, kept, servers);
        }
        let mut toolbox = Toolbox::open(
            &self.tools_client,
            (&self.listed, &self.kept),
            agent,
            servers,
            (self.open_timeout, self.start_limit),
            config.opinions(),
        )
        .await;
        toolbox.register_reads(&config.read_sources);
        toolbox
    }

    /// `agent`'s tools' lists, kept for the next process when a tool
    /// listed anything new.
    fn keep_lists(&self, agent: &Agent) {
        if let Some(lists) = self.listed.to_keep(agent, self.agents.tools(agent))
            && let Err(err) = off_runtime(|| self.store.set_tool_lists(agent, &lists))
        {
            crate::warn!("keeping the tools' lists: {err:#}");
        }
    }

    /// What a turn of `thread`'s is asked with, its settings,
    /// instructions and tools given: the window is the model's, less what
    /// the system prompt and tools take; and the events of the
    /// conversation the chat is on, from where its window's walk goes
    /// on, with its mark. Nothing is written.
    pub(super) async fn prepare<'a>(
        &self,
        agent: &Agent,
        thread: &Thread,
        config: &'a Config,
        prompt: &'a crate::prompt::Prompt,
        toolbox: &Toolbox<'_>,
        given: Option<&Read>,
    ) -> anyhow::Result<Prepared<'a>> {
        let instructions = prompt.system.as_str();
        let opinions = config.opinions();
        let window = opinions.window.clone().unwrap_or_default();
        let model = opinions.model_for(&config.model.provider);
        let known = limits::known(config.model_facts.as_ref(), &config.model);
        let limits = self
            .reported
            .of(
                &self.client,
                &config.model,
                known.as_ref(),
                window.unknown.unwrap_or(1024),
            )
            .await
            .within(&opinions, &config.model.provider, &config.links());
        let turns = opinions.turns.clone().unwrap_or_default();
        let fixed = window::text_tokens(instructions)
            + toolbox
                .offered()
                .iter()
                .map(|t| {
                    window::text_tokens(&t.name)
                        + window::text_tokens(&t.description)
                        + window::text_tokens(&t.schema.to_string())
                })
                .sum::<usize>();
        let full = self.budget.unwrap_or_else(|| limits.budget(fixed));
        // Compacted, the conversation fills the spec's share of what the
        // window holds of it: the rest is room for the summary.
        let compacting = window.compact_at.and_then(|at| at.share());
        // Walked with a steady budget, so a small change to the prompt
        // keeps the window and its mark.
        let budget = window::steady(
            compacting.map_or(full, |share| ((full as f64 * share) as usize).max(1)),
        );
        let compact = compacting.map(|_| Compact {
            full,
            room: u32::try_from(full - budget)
                .unwrap_or(u32::MAX)
                .min(limits.answer)
                .max(1),
            save: window.save_notes.unwrap_or(false)
                && toolbox
                    .offered()
                    .iter()
                    .any(|t| t.name == compact::REMEMBER),
        });
        let (events, mark, kept) = off_runtime(|| self.read(agent, thread, budget, given))?;
        // The conversation the chat is on: its every event read when a
        // fork or a resume began it.
        let events = match crate::commands::needs_all(&events) {
            true => match given.and_then(|given| given.from(1)) {
                Some(events) => events,
                None => off_runtime(|| self.store.read(agent, thread))?,
            },
            false => events,
        };
        let (events, mark) = crate::commands::conversation(events, mark, budget);
        let cache = caching::Caching::of(config, thread, &events).and_then(|c| c.asked());
        let ask = Ask {
            config,
            instructions,
            spans: &prompt.spans,
            answer: limits.answer,
            window: limits.window,
            price: limits.price,
            above: known.map(|k| k.facts.above).unwrap_or_default(),
            thinking: model::Think {
                thinks: limits.thinks,
                level: model.thinking.unwrap_or(link_agent_spec::Thinking::Off),
                budget: model.thinking_budget.unwrap_or(0),
            },
            fast: config.fast
                && config.model.named().is_some_and(|name| {
                    link_model_contract::chat::offers_fast(&config.model.provider, &name)
                }),
            rounds: turns.rounds.unwrap_or(1) as usize,
            calls: turns.calls.unwrap_or(1) as usize,
            steer: turns.busy == Some(Busy::Steer),
            compact,
            second: match crate::fallback::Second::of(config, &self.reported, &self.client).await {
                // Sent its own model's section in place of the first's.
                Some(mut second) => {
                    let (_, own) = off_runtime(|| {
                        crate::prompt::model_section(
                            self.store.as_ref(),
                            agent,
                            config,
                            &second.model,
                        )
                    })?;
                    second.swap = prompt.swap(own);
                    Some(second)
                }
                None => None,
            },
            cache,
        };
        Ok(Prepared {
            ask,
            events,
            mark,
            kept,
            budget,
        })
    }

    /// The turn, with its settings, instructions and tools: as
    /// [`Self::prepare`] asks it, `message` written first, and answered,
    /// or, for `work` [`Work::Compact`], what's before the owner's last
    /// message summarized.
    async fn answer(
        &self,
        turn: &Turn<'_>,
        message: Option<&Event>,
        config: &Config,
        prompt: &crate::prompt::Prompt,
        toolbox: &Toolbox<'_>,
        read: &Read,
    ) -> anyhow::Result<Event> {
        let (agent, thread) = (turn.agent, turn.thread);
        let Prepared {
            ask,
            mut events,
            mark,
            kept,
            budget,
            ..
        } = self
            .prepare(agent, thread, config, prompt, toolbox, Some(read))
            .await?;
        let _ = turn.cache.set(ask.cache.clone());
        off_runtime(|| self.finish(agent, thread, &mut events))?;
        // Where this turn's events start, for its measures.
        if let Some(measuring) = turn.measuring {
            measuring.starts(next_seq(&events));
        }
        if let Some(message) = message {
            let mut said = message.clone();
            off_runtime(|| self.append_next(agent, thread, &mut said, &mut events))?;
        }
        if let Some(last) = events.last() {
            let _ = turn
                .own
                .compare_exchange(0, last.seq, Ordering::Relaxed, Ordering::Relaxed);
        }
        self.take_inputs(turn, &mut events, true)?;
        if turn.work == Work::Compact {
            let mark = mark.or_else(|| events.first().map(|first| window::Mark::at(first, budget)));
            return self.compact_now(turn, &ask, toolbox, events, mark).await;
        }
        let Some(first) = events.first() else {
            anyhow::bail!("nothing to answer");
        };
        let mark = mark.unwrap_or_else(|| window::Mark::at(first, budget));
        self.rounds(turn, &ask, toolbox, events, mark, kept).await
    }

    /// What a turn needs of `thread`'s events: from its window's start,
    /// or from the agent's last answer if that's earlier, with where the
    /// window's walk goes on, and the mark as kept. All of them, and a
    /// mark made from them, when the mark is missing or isn't theirs.
    fn read(
        &self,
        agent: &Agent,
        thread: &Thread,
        budget: usize,
        given: Option<&Read>,
    ) -> anyhow::Result<(Vec<Event>, Option<window::Mark>, window::Mark)> {
        let read_from = |first: u64| match given.and_then(|given| given.from(first)) {
            Some(events) => Ok(events),
            None => self.store.read_from(agent, thread, first),
        };
        let kept = self.store.mark(agent, thread);
        if let Some(mark) = kept.clone()
            && mark.budget == budget
            && mark.seq >= 1
            && mark.fits(EVENT_TOKENS_MAX)
        {
            let first = match mark.answered {
                0 => 1,
                answered => mark.seq.min(answered),
            };
            let first = match mark.summary {
                0 => first,
                summary => first.min(summary),
            };
            let events = read_from(first)?;
            let theirs = events.first().is_some_and(|e| e.seq == first)
                && events.iter().any(|e| e.seq == mark.seq && e.at == mark.at)
                && (mark.answered == 0
                    || events
                        .iter()
                        .any(|e| e.seq == mark.answered && e.is_answer()))
                && (mark.summary == 0
                    || events
                        .iter()
                        .any(|e| e.seq == mark.summary && e.role == event::SUMMARY));
            if theirs {
                return Ok((events, Some(mark.clone()), mark));
            }
        }
        let events = read_from(1)?;
        let mark = window::Mark::of(&events, budget);
        Ok((events, mark, kept.unwrap_or_default()))
    }
}

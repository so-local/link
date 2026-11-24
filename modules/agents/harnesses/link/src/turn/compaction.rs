//! A turn's compaction: what leaves the window summarized, before a round
//! or after the answer, and the instructions it's asked with.

use super::*;

impl Harness {
    /// A summary, asked of the model with `system` and the events of
    /// `said`, `request` after them: the prompt it was last sent and its
    /// answer, so the provider's cache holds up to there, or what leaves
    /// alone. With `save`, it may first save what matters to its notes,
    /// `memory__remember` the only call made, in as many rounds as a turn
    /// takes; else it calls nothing. Offered its tools as every round is,
    /// so the cache holds for them too. None when it was cut short, wrote
    /// nothing, or its work was halted meanwhile ([`Self::same_work`]): a
    /// message steered in meanwhile leaves it standing. The summary's
    /// `covers` is the caller's to set. Asked of the fallback as a turn
    /// is (`using`, the turn's).
    #[expect(
        clippy::too_many_arguments,
        reason = "the turn's parts a compaction reads and the fallback it sets, each used apart"
    )]
    async fn compact(
        &self,
        turn: &Turn<'_>,
        ask: &Ask<'_>,
        how: &Compact,
        toolbox: &Toolbox<'_>,
        system: &str,
        mut said: Vec<Event>,
        (request, save): (String, bool),
        meta: &serde_json::Value,
        using: &mut Option<crate::fallback::Why>,
        events: &mut Vec<Event>,
    ) -> anyhow::Result<Option<Event>> {
        let tools = toolbox.offered();
        // The fallback, its summary no longer than the first's room.
        let second = ask.second.as_ref().map(|second| crate::fallback::Second {
            answer: second.answer.min(how.room),
            ..second.duplicate()
        });
        let mut seq = said.iter().map(|e| e.seq).max().unwrap_or(0);
        let mut number = || {
            seq += 1;
            seq
        };
        said.push(Event {
            seq: 0,
            ..Event::user(&request)
        });
        // Nobody reads it as it's written: it's the agent's own.
        let (unread, _) = mpsc::channel(1);
        let started = std::time::Instant::now();
        let mut summary = Event {
            role: event::SUMMARY.into(),
            at: event::now(),
            ..Event::default()
        };
        // Its calls are the thread's, apart from its turns'.
        let chat = format!("{}/compaction", turn.thread.name());
        let mut round = 0;
        loop {
            let model_work = self.step_work(turn, link_interactions::runtime::Step::Model)?;
            let parts = |part: &event::Part| self.store.part(turn.agent, turn.thread, &part.file);
            let projected = off_runtime(|| {
                self.requested
                    .digests()
                    .with(turn.agent, turn.thread, |digests| {
                        digests.sweep();
                        model::projected_kept(&said, !tools.is_empty(), &parts, digests)
                    })
            });
            let prompt = model::Prompt {
                chat: Some(&chat),
                spans: ask.spans,
                parent: Some(turn.thread.name()),
                cache: ask.cache.as_deref(),
                system,
                messages: &projected.messages,
                tools: &tools,
                no_calls: round >= ask.rounds,
                answer: how.room,
                window: ask.window,
                price: ask.price,
                above: &ask.above,
                thinking: ask.thinking,
                fast: ask.fast,
            };
            let mut answer = self
                .ask_model(
                    turn,
                    request::Attempt {
                        first: &ask.config.model,
                        second: second.as_ref(),
                        prompt: &prompt,
                        sources: &projected.sources,
                        work: model_work.clone(),
                        phase: link_harness_wire::evidence::Phase::Summary,
                        pieces: &unread,
                        separate: false,
                    },
                    using,
                    events,
                )
                .await?;
            summary.request = answer.request;
            summary.model = answer.model.clone();
            summary.provider = answer.provider.clone();
            if let Some(tokens) = &answer.tokens {
                let sum = summary.tokens.get_or_insert_default();
                sum.input += tokens.input;
                sum.output += tokens.output;
                sum.cached += tokens.cached;
            }
            if let Some(cost) = answer.cost {
                *summary.cost.get_or_insert(0.0) += cost;
            }
            if answer.interrupted {
                return Ok(None);
            }
            answer.calls.truncate(ask.calls);
            let last = answer.calls.is_empty() || round >= ask.rounds;
            // The summary the model answered stands, whatever was
            // steered in meanwhile, which is answered next; only work
            // halted meanwhile refuses it. A call is made only under the
            // control it was asked under.
            let going = match last {
                true => self.same_work(turn, &model_work),
                false => self.same_step(turn, &model_work),
            };
            if going.is_err() {
                return Ok(None);
            }
            if last {
                summary.text = answer.text.trim().to_string();
                break;
            }
            answer.seq = number();
            let provider_ids = answer.calls.iter().map(|call| call.id.clone()).collect();
            model::settle_ids(&mut answer)?;
            let clock = execution::Clock::new();
            let mut proposed = Event {
                role: event::EVIDENCE.into(),
                text: answer.text.clone(),
                at: event::now(),
                model: answer.model.clone(),
                provider: answer.provider.clone(),
                response_id: answer.response_id.clone(),
                request: answer.request,
                evidence: Some(
                    clock
                        .proposal(
                            turn,
                            &answer,
                            provider_ids,
                            link_harness_wire::evidence::Phase::Summary,
                            model_work.clone(),
                        )
                        .into(),
                ),
                ..Event::default()
            };
            off_runtime(|| self.append_next(turn.agent, turn.thread, &mut proposed, events))?;
            turn.wrote(&proposed);
            answer.seq = proposed.seq;
            let mut meta = meta.clone();
            meta["link.local/attempt"] = serde_json::json!(turn.attempt);
            if let Some(binding) = self.carried(turn) {
                meta["link.local/work"] = serde_json::json!(binding.token.expose());
            }
            if let Some(work) = &model_work {
                meta["link.local/control_revision"] = serde_json::json!(work.control_revision);
            }
            let made = self
                .execute(
                    turn,
                    toolbox,
                    execution::Batch {
                        origin: proposed.seq,
                        clock,
                        phase: link_harness_wire::evidence::Phase::Summary,
                        calls: &answer.calls,
                        expected: &model_work,
                        meta,
                        policy: execution::Policy::Summary { save },
                    },
                    events,
                )
                .await?;
            for (call, result) in answer.calls.iter().zip(&made) {
                if call.name == compact::REMEMBER
                    && matches!(
                        result.evidence.as_ref().map(|e| &e.record),
                        Some(link_harness_wire::evidence::Record::Finished { outcome: link_harness_wire::evidence::Outcome::Completed { reply }, .. }) if !reply.is_error
                    )
                {
                    summary.calls.push(call.clone());
                }
            }
            said.push(answer.clone());
            said.extend(made.into_iter().map(|mut event| {
                event.role = event::TOOL.into();
                event
            }));
            if *turn.cancelled.borrow() {
                return Ok(None);
            }
            round += 1;
        }
        summary.latency_ms = Some(started.elapsed().as_millis() as u64);
        Ok((!summary.text.is_empty()).then_some(summary))
    }

    /// The summary of what leaves the window, `leaving`, which starts at
    /// `at`, added to `events` and the window's head: asked of `said`,
    /// what the model is sent up to there. Notes are the owner's: saved
    /// only from a conversation the owner spoke in, never one of
    /// teammates', schedules' or subagents' alone. Whether it saved any.
    /// Cut short, or saying nothing, or failing, the turns leave
    /// unsummarized, and the next round or turn tries again. Asked of the
    /// fallback as the turn is (`using`, the turn's).
    #[expect(
        clippy::too_many_arguments,
        reason = "the turn's parts a compaction reads and the fallback it sets, each used apart"
    )]
    pub(super) async fn summarize(
        &self,
        turn: &Turn<'_>,
        ask: &Ask<'_>,
        how: &Compact,
        toolbox: &Toolbox<'_>,
        system: &str,
        (said, leaving, at): (Vec<Event>, &[Event], u64),
        events: &mut Vec<Event>,
        mark: &mut window::Mark,
        using: &mut Option<crate::fallback::Why>,
    ) -> anyhow::Result<bool> {
        let (agent, thread) = (turn.agent, turn.thread);
        let save = how.save
            && leaving
                .iter()
                .any(|e| e.role == event::USER && e.from.is_none());
        let staying = events
            .iter()
            .find(|e| e.seq == at && said.iter().any(|s| s.seq == at));
        let request = compact::request(save, staying);
        let meta = call_meta(
            thread,
            events,
            &toolbox.offered(),
            ask.config.model.named().as_deref(),
        );
        let summary = self
            .compact(
                turn,
                ask,
                how,
                toolbox,
                system,
                said,
                (request, save),
                &meta,
                using,
                events,
            )
            .await;
        match summary {
            Ok(Some(mut summary)) => {
                summary.covers = at;
                let saved = !summary.calls.is_empty();
                off_runtime(|| self.append_next(agent, thread, &mut summary, events))?;
                turn.wrote(&summary);
                mark.summary = summary.seq;
                Ok(saved)
            }
            Ok(None) => Ok(false),
            Err(err) => {
                crate::warn!(crate::said::in_chat(agent, thread) => "summarizing: {err:#}");
                Ok(false)
            }
        }
    }

    /// After the turn's answer, handed to whoever waits for it: what left
    /// the window it was asked with summarized, asked as that prompt,
    /// `window` (its instructions, its events, and where they start in
    /// `events`), and the answer after it, so the provider's cache holds
    /// for all of it. The turn's lease is held till it's done. Asked of
    /// the fallback as the turn was (`using`, the turn's).
    #[expect(
        clippy::too_many_arguments,
        reason = "the turn's parts a compaction reads and the fallback it sets, each used apart"
    )]
    pub(super) async fn compact_after(
        &self,
        turn: &Turn<'_>,
        ask: &Ask<'_>,
        how: &Compact,
        toolbox: &Toolbox<'_>,
        (system, shown, sent): (&str, Vec<Event>, usize),
        answer: &Event,
        mut events: Vec<Event>,
        mut mark: window::Mark,
        using: &mut Option<crate::fallback::Why>,
    ) {
        if let Some(measuring) = turn.measuring {
            measuring.ended();
            turn.whole.store(true, Ordering::Relaxed);
        }
        turn.tell(answer);
        let from = events
            .iter()
            .position(|e| e.seq == mark.seq)
            .unwrap_or_default();
        let start = from + window::walk(&events[from..], &mark).0;
        let Some(at) = events.get(start).map(|e| e.seq) else {
            return;
        };
        let leaving: Vec<Event> = events[sent..start.max(sent)]
            .iter()
            .filter(|e| !matches!(e.role.as_str(), event::SUMMARY | event::EVIDENCE))
            .cloned()
            .collect();
        if leaving.is_empty() {
            return;
        }
        let mut said = shown;
        said.push(answer.clone());
        let summarized = self
            .summarize(
                turn,
                ask,
                how,
                toolbox,
                system,
                (said, &leaving, at),
                &mut events,
                &mut mark,
                using,
            )
            .await;
        if let Err(err) = summarized {
            crate::warn!(crate::said::in_chat(turn.agent, turn.thread) => "keeping its summary: {err:#}");
        }
    }

    /// `/chat:compact`'s turn: what of `events` is before the owner's
    /// last message summarized now, as what leaves the window is, that
    /// message and what follows it staying. What the owner is told comes
    /// back, as a command's event, never kept.
    pub(super) async fn compact_now(
        &self,
        turn: &Turn<'_>,
        ask: &Ask<'_>,
        toolbox: &Toolbox<'_>,
        mut events: Vec<Event>,
        mark: Option<window::Mark>,
    ) -> anyhow::Result<Event> {
        let told = |text: &str| Event {
            role: event::COMMAND.into(),
            text: text.into(),
            at: event::now(),
            ..Event::default()
        };
        let Some(how) = ask.compact.as_ref() else {
            return Ok(told(crate::commands::NO_COMPACTION));
        };
        let head = compact::head(&events).cloned();
        let covered = head.as_ref().map_or(1, |h| h.covers);
        let staying = events
            .iter()
            .rposition(|e| e.role == event::USER && e.from.is_none());
        let leaving: Vec<Event> = events[..staying.unwrap_or_default()]
            .iter()
            .filter(|e| {
                e.seq >= covered
                    && !matches!(
                        e.role.as_str(),
                        event::SUMMARY | event::COMMAND | event::EVIDENCE
                    )
            })
            .cloned()
            .collect();
        let (Some(at), Some(mut mark)) = (
            staying
                .map(|i| events[i].seq)
                .filter(|_| !leaving.is_empty()),
            mark,
        ) else {
            return Ok(told(crate::commands::NOTHING_TO_COMPACT));
        };
        let said: Vec<Event> = head.iter().chain(&leaving).cloned().collect();
        let saved = self
            .summarize(
                turn,
                ask,
                how,
                toolbox,
                ask.instructions,
                (said, &leaving, at),
                &mut events,
                &mut mark,
                &mut None,
            )
            .await?;
        if compact::head(&events).is_none_or(|h| h.covers != at) {
            return Ok(told(crate::commands::NOT_COMPACTED));
        }
        Ok(told(&crate::commands::compacted(leaving.len(), saved)))
    }

    /// `agent`'s instructions, composed from the sections its spec says,
    /// through the store, for its model and the tools in `toolbox`, in
    /// conversation `thread`: its notes only in one of the owner's.
    pub(super) fn instructions(
        &self,
        agent: &Agent,
        thread: &Thread,
        config: &Config,
        toolbox: &Toolbox<'_>,
    ) -> anyhow::Result<crate::prompt::Prompt> {
        let folder = self.agents.paths(agent)?.folder();
        let tools = toolbox.instructions();
        let given = crate::prompt::Given {
            model: Some(&config.model),
            tools: &tools,
            others: !link_agent_identity::access::is_owners_thread(thread.name()),
        };
        off_runtime(|| crate::prompt::compose(self.store.as_ref(), agent, config, &folder, given))
    }
}

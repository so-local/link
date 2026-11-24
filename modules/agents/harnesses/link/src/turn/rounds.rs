//! A turn's rounds: the model asked, and its calls made, until it answers
//! with none.

use super::*;

impl Harness {
    /// The model asked, and its calls made, until it answers with none:
    /// with `ask.steer`, each round reads first what came while it ran,
    /// and an answer followed by a message steered in goes on to answer
    /// that too. Asked to end, it ends: a half-written answer kept,
    /// marked interrupted, and the calls still running cancelled.
    pub(super) async fn rounds(
        &self,
        turn: &Turn<'_>,
        ask: &Ask<'_>,
        toolbox: &Toolbox<'_>,
        mut events: Vec<Event>,
        mut mark: window::Mark,
        // Kept at the first round, then again only when it moves.
        mut kept: window::Mark,
    ) -> anyhow::Result<Event> {
        let (agent, thread, pieces) = (turn.agent, turn.thread, turn.pieces);
        let tools = toolbox.offered();
        let mut separate = false;
        let mut round = 0;
        // The instructions composed again, after a summary saved notes.
        let mut recomposed: Option<String> = None;
        // Where the window as last sent starts, while what leaves it is
        // summarized after the answer.
        let mut later: Option<usize>;
        // Once its fallback answered, the turn's other rounds go to it.
        let mut fell_back = None;
        loop {
            let model_work = self.step_work(turn, link_interactions::runtime::Step::Model)?;
            self.take_inputs(turn, &mut events, round == 0)?;
            if ask.steer {
                off_runtime(|| self.catch_up(agent, thread, &mut events))?;
            }
            let from = events
                .iter()
                .position(|e| e.seq == mark.seq)
                .unwrap_or_default();
            let (walked, moved) = window::walk(&events[from..], &mark);
            let mut start = from + walked;
            // The summary the window starts with: the latest, or a new one
            // of what left it since that was written.
            let mut head = compact::head(&events).cloned();
            let covered = head.as_ref().map_or(1, |h| h.covers);
            let due = ask
                .compact
                .as_ref()
                .filter(|_| events.get(start).is_some_and(|e| e.seq > covered));
            match due.map(|how| sent(&events, head.as_ref(), covered, how.full)) {
                // Answered first with the window as last sent, which its
                // room still holds, and summarized after: its mark moves
                // once what leaves is summarized.
                Some(Some(sent)) => {
                    start = sent;
                    later = Some(sent);
                }
                // Grown past even that: summarized first, of what leaves
                // alone, and the window moves on.
                Some(None) => {
                    mark = moved;
                    later = None;
                    let at = events[start].seq;
                    let leaving: Vec<Event> =
                        compact::leaving(&events, start, covered, mark.budget)
                            .iter()
                            .filter(|e| {
                                !matches!(e.role.as_str(), event::SUMMARY | event::EVIDENCE)
                            })
                            .cloned()
                            .collect();
                    if !leaving.is_empty()
                        && let Some(how) = due
                    {
                        let said: Vec<Event> = head.iter().chain(&leaving).cloned().collect();
                        let saved = self
                            .summarize(
                                turn,
                                ask,
                                how,
                                toolbox,
                                system(&recomposed, ask),
                                (said, &leaving, at),
                                &mut events,
                                &mut mark,
                                &mut fell_back,
                            )
                            .await?;
                        head = compact::head(&events).cloned();
                        // Notes saved: the prompt from here on has them.
                        if saved {
                            recomposed = Some(
                                self.instructions(agent, turn.thread, ask.config, toolbox)?
                                    .system,
                            );
                        }
                    }
                }
                None => {
                    mark = moved;
                    later = None;
                }
            }
            // A summary that covers past the walk's start (a smaller
            // window since): the window starts where it ends.
            if let Some(covers) = head.as_ref().map(|h| h.covers)
                && let Some(after) = events.iter().position(|e| e.seq >= covers)
            {
                start = start.max(after);
            }
            // What the model has seen: a message of the owner's after it
            // came while it answered.
            let seen = events.last().map_or(0, |e| e.seq);
            let shown: Vec<Event> = head
                .into_iter()
                .chain(
                    events[start..]
                        .iter()
                        .filter(|e| !matches!(e.role.as_str(), event::SUMMARY | event::EVIDENCE))
                        .cloned(),
                )
                .collect();
            // Each part read from its file, as the store keeps it.
            let parts = |part: &event::Part| self.store.part(agent, thread, &part.file);
            let projected = off_runtime(|| {
                self.requested.digests().with(agent, thread, |digests| {
                    // What the request before sent and this one doesn't
                    // is forgotten.
                    digests.sweep();
                    model::projected_kept(&shown, !tools.is_empty(), &parts, digests)
                })
            });
            let prompt = model::Prompt {
                system: system(&recomposed, ask),
                messages: &projected.messages,
                tools: &tools,
                no_calls: round >= ask.rounds,
                answer: ask.answer,
                window: ask.window,
                price: ask.price,
                above: &ask.above,
                thinking: ask.thinking,
                fast: ask.fast,
                chat: Some(thread.name()),
                spans: ask.spans,
                parent: None,
                cache: ask.cache.as_deref(),
            };
            if let Some(measuring) = turn.measuring {
                measuring.clock.asking();
            }
            let mut answer = self
                .ask_model(
                    turn,
                    request::Attempt {
                        first: &ask.config.model,
                        second: ask.second.as_ref(),
                        prompt: &prompt,
                        sources: &projected.sources,
                        work: model_work.clone(),
                        phase: link_harness_wire::evidence::Phase::Turn,
                        pieces,
                        separate,
                    },
                    &mut fell_back,
                    &mut events,
                )
                .await?;
            if let (Some(store), Some(binding)) = (&self.interactions, turn.binding) {
                let included: Vec<_> = shown.iter().filter_map(|e| e.input.clone()).collect();
                off_runtime(|| store.inputs_included(binding.token.expose(), &included))?;
            }
            if self.same_step(turn, &model_work).is_err() {
                answer.interrupted = true;
                answer.calls.clear();
            }
            separate |= !answer.text.is_empty();
            answer.calls.truncate(ask.calls);
            // An answer cut short makes none of its calls.
            if answer.interrupted {
                answer.calls.clear();
            }
            answer.seq = next_seq(&events);
            answer.seen = seen;
            if answer.is_answer() {
                mark.answered = answer.seq;
            }
            // Kept with the answer, when it moved.
            if mark != kept {
                answer.window = Some(mark.clone());
            }
            let provider_ids = answer.calls.iter().map(|call| call.id.clone()).collect();
            model::settle_ids(&mut answer)?;
            let clock = execution::Clock::new();
            if !answer.calls.is_empty() {
                answer.evidence = Some(
                    clock
                        .proposal(
                            turn,
                            &answer,
                            provider_ids,
                            link_harness_wire::evidence::Phase::Turn,
                            model_work.clone(),
                        )
                        .into(),
                );
            }
            off_runtime(|| self.append_next(agent, thread, &mut answer, &mut events))?;
            turn.wrote(&answer);
            if answer.is_answer() {
                mark.answered = answer.seq;
            }
            if let Some(window) = &answer.window {
                kept = window.clone();
            }
            if answer.interrupted {
                return Ok(answer);
            }
            if answer.calls.is_empty() {
                // While it summarizes after, and until its lease is let
                // go, a message waits for a turn of its own: marked before
                // looking for one steered in.
                if later.is_some() {
                    self.compacting.mark(agent, thread);
                }
                // A message steered in while it answered: answered too.
                if ask.steer {
                    off_runtime(|| self.catch_up(agent, thread, &mut events))?;
                    let steered = events.iter().any(|e| e.seq > seen && e.role == event::USER);
                    if steered && round < ask.rounds {
                        self.compacting.release(agent, thread);
                        round += 1;
                        continue;
                    }
                }
                if let (Some(sent), Some(how)) = (later, &ask.compact) {
                    let window = (system(&recomposed, ask), shown, sent);
                    self.compact_after(
                        turn,
                        ask,
                        how,
                        toolbox,
                        window,
                        &answer,
                        events,
                        mark,
                        &mut fell_back,
                    )
                    .await;
                }
                return Ok(answer);
            }
            // Where the calls are made, for a tool that sends messages:
            // this conversation, the hops of the message being answered,
            // and its tools (a subagent it starts gets those or fewer).
            let mut meta = call_meta(thread, &events, &tools, ask.config.model.named().as_deref());
            meta["link.local/attempt"] = serde_json::json!(turn.attempt);
            if let Some(binding) = self.carried(turn) {
                meta["link.local/work"] = serde_json::json!(binding.token.expose());
            }
            if let Some(work) = &model_work {
                meta["link.local/control_revision"] = serde_json::json!(work.control_revision);
            }
            self.execute(
                turn,
                toolbox,
                execution::Batch {
                    origin: answer.seq,
                    clock,
                    phase: link_harness_wire::evidence::Phase::Turn,
                    calls: &answer.calls,
                    expected: &model_work,
                    meta,
                    policy: execution::Policy::Turn {
                        limited: round >= ask.rounds,
                    },
                },
                &mut events,
            )
            .await?;
            if *turn.cancelled.borrow()
                || round >= ask.rounds
                || self
                    .step_work(turn, link_interactions::runtime::Step::Model)
                    .is_err()
            {
                return Ok(answer);
            }
            round += 1;
        }
    }
}

/// Where a turn's calls are made, for a tool that sends messages: its
/// conversation, the hops of the message being answered, and its tools
/// (a subagent it starts gets those or fewer).
pub(super) fn call_meta(
    thread: &Thread,
    events: &[Event],
    tools: &[crate::tools::Tool],
    model: Option<&str>,
) -> serde_json::Value {
    let hops = events
        .iter()
        .rev()
        .find(|e| e.role == event::USER)
        .map_or(0, |e| e.hops);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    // The standing of the message the turn answers: its sender's, at
    // most what its firing carried. A schedule or subagent the call makes
    // keeps it, so it never fires as more.
    let standing = events.iter().rev().find(|e| e.role == event::USER).map_or(
        link_connection_contract::access::Standing::Owner,
        |e| {
            let of = link_connection_contract::access::Standing::of(e.from.as_deref());
            e.standing.map_or(of, |cap| of.at_most(cap))
        },
    );
    use link_connection_contract::access::{STANDING_META, THREAD_META};
    let mut meta = serde_json::json!({"link.local/hops": hops, "link.local/tools": names});
    meta[THREAD_META] = serde_json::json!(thread.name());
    meta[STANDING_META] = serde_json::json!(standing);
    if let Some(from) = events
        .iter()
        .rev()
        .find(|event| event.role == event::USER)
        .and_then(|event| event.from.as_deref())
    {
        meta["link.local/from"] = serde_json::json!(from);
    }
    if let Some(model) = model {
        meta["link.local/model"] = serde_json::json!(model);
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_calls_identify_the_schedule_being_answered() {
        let mut event = Event::user("Send the joke on Discord");
        event.from = Some("schedule:abcdef123456".into());
        let thread = Thread::new("link:reminder").unwrap();
        let meta = call_meta(&thread, &[event], &[], None);
        assert_eq!(meta["link.local/from"], "schedule:abcdef123456");
        assert_eq!(meta["link.local/thread"], "link:reminder");
        let owner = call_meta(&thread, &[Event::user("Hi")], &[], None);
        assert!(owner.get("link.local/from").is_none());
    }

    /// Each call says the standing of the message its turn answers: its
    /// sender's, at most what its firing carried, so what the call makes
    /// never stands as more.
    #[test]
    fn tool_calls_say_the_turns_standing() {
        use link_connection_contract::access::Standing;
        let thread = Thread::new("telegram:42").unwrap();
        let said = |from: Option<&str>, cap: Option<Standing>| {
            let mut event = Event::user("Again tomorrow.");
            event.from = from.map(str::to_string);
            event.standing = cap;
            call_meta(&thread, &[event], &[], None)["link.local/standing"].clone()
        };
        assert_eq!(said(None, None), "owner");
        assert_eq!(said(Some("member:telegram:7"), None), "member");
        assert_eq!(said(Some("webhook:deploys"), None), "stranger");
        // A schedule a stranger's conversation made fires as a stranger.
        assert_eq!(
            said(Some("schedule:abcdef123456"), Some(Standing::Stranger)),
            "stranger"
        );
        // A cap never raises it.
        assert_eq!(
            said(Some("member:telegram:7"), Some(Standing::Owner)),
            "member"
        );
    }
}

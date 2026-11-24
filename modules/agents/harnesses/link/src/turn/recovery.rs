//! Resolves incomplete work without claiming an unknown remote effect failed.

use super::*;

impl Harness {
    /// Answers each call of the agent's last answer in `events` that has
    /// none: [`NOT_MADE_AGAIN`]. The lease held, no turn is making them:
    /// the one that did crashed, or failed.
    pub(super) fn finish(
        &self,
        agent: &Agent,
        thread: &Thread,
        events: &mut Vec<Event>,
    ) -> anyhow::Result<()> {
        use link_harness_wire::evidence::{
            Evidence, Outcome, Phase, Record,
            index::{self, Located},
        };
        let first = events.first().map_or(1, |event| event.seq);
        let trace = index::index(events.iter().filter_map(|event| {
            let evidence = event.evidence.as_ref()?;
            let in_range = match &evidence.record {
                // Each lists every message before it: a request never
                // leaves a call to answer, so a turn doesn't read them.
                Record::ModelRequested { .. } => false,
                Record::Dispatched { call, .. } | Record::Finished { call, .. } => {
                    call.proposal >= first
                }
                Record::SkillLoaded { load } => match load.anchor {
                    link_harness_wire::evidence::SkillAnchor::Call { call } => {
                        call.proposal >= first
                    }
                    _ => true,
                },
                _ => true,
            };
            in_range.then_some(Located {
                seq: event.seq,
                evidence,
            })
        }))?;
        let pending: Vec<_> = trace
            .executions
            .iter()
            .filter(|(_, execution)| execution.finished.is_none())
            .map(|(reference, execution)| {
                (
                    *reference,
                    execution.proposal.phase,
                    execution.proposed.call.clone(),
                )
            })
            .collect();
        for (reference, phase, call) in pending {
            let mut event = Event {
                role: if phase == Phase::Turn {
                    event::TOOL
                } else {
                    event::EVIDENCE
                }
                .into(),
                text: NOT_MADE_AGAIN.into(),
                call: Some(call.id),
                tool: Some(call.name),
                failed: true,
                at: event::now(),
                evidence: Some(Evidence::new(Record::Finished {
                    call: reference,
                    offset_ns: None,
                    outcome: Outcome::Unknown {
                        reason:
                            "Interrupted work has no recorded reply; remote effects are unknown."
                                .into(),
                    },
                }).into()),
                ..Event::default()
            };
            self.append_next(agent, thread, &mut event, events)?;
        }
        let Some(last) = events.iter().rposition(|e| e.role == event::ASSISTANT) else {
            return Ok(());
        };
        // Imported and hand-written events can have calls without native
        // observations. Keep their model history usable, without inventing
        // evidence of dispatch or a remote outcome.
        if events[last].evidence.is_some() {
            return Ok(());
        }
        let answered: Vec<String> = events[last..]
            .iter()
            .filter_map(|e| e.call.clone())
            .collect();
        for call in events[last].calls.clone() {
            if answered.contains(&call.id) {
                continue;
            }
            let mut event = Event {
                role: event::TOOL.into(),
                text: NOT_MADE_AGAIN.into(),
                call: Some(call.id.clone()),
                tool: Some(call.name.clone()),
                failed: true,
                at: event::now(),
                ..Event::default()
            };
            self.append_next(agent, thread, &mut event, events)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_harness_wire::evidence::{
        CallRef, Evidence, Outcome, Phase, Proposal, ProposedCall, Record,
    };

    #[test]
    fn missing_markers_and_replies_recover_as_unknown_once_in_turns_and_summaries() {
        for phase in [Phase::Turn, Phase::Summary] {
            for dispatched in [false, true] {
                let root = link_test_kit::TempDir::new("recover-evidence");
                let agent = Agent::new("main").unwrap();
                let thread = Thread::new("link:recover").unwrap();
                let mut agents = Agents::default();
                agents.add(
                    agent.clone(),
                    root.join("harness"),
                    crate::agent::Folder {
                        home: root.join("home"),
                        agent: root.join("own"),
                    },
                );
                let store = Arc::new(crate::store::Files::new(agents.clone()));
                let harness = Harness::new(agents, store.clone()).unwrap();
                let call = event::Call {
                    id: "repeated-provider-id".into(),
                    name: "task__run".into(),
                    arguments: serde_json::json!({"command":"effect"}),
                };
                let mut events = vec![Event {
                    seq: 1,
                    role: if phase == Phase::Turn {
                        event::ASSISTANT
                    } else {
                        event::EVIDENCE
                    }
                    .into(),
                    calls: vec![call.clone()],
                    at: event::now(),
                    evidence: Some(
                        Evidence::new(Record::Proposed(Proposal {
                            phase,
                            attempt: "fixture".into(),
                            work: None,
                            started_unix_ms: 0,
                            calls: vec![ProposedCall {
                                provider_id: call.id.clone(),
                                call,
                            }],
                        }))
                        .into(),
                    ),
                    ..Default::default()
                }];
                store.append(&agent, &thread, &events[0], &[]).unwrap();
                if dispatched {
                    let marker = Event {
                        seq: 2,
                        role: event::EVIDENCE.into(),
                        at: event::now(),
                        evidence: Some(
                            Evidence::new(Record::Dispatched {
                                call: CallRef {
                                    proposal: 1,
                                    ordinal: 0,
                                },
                                offset_ns: 5,
                                request: link_harness_wire::evidence::McpDispatch {
                                    server: "task".into(),
                                    operation: "run".into(),
                                    arguments: link_harness_wire::evidence::Captured::Retained {
                                        value: serde_json::json!({"command":"effect"}),
                                    },
                                },
                            })
                            .into(),
                        ),
                        ..Default::default()
                    };
                    store.append(&agent, &thread, &marker, &events).unwrap();
                    events.push(marker);
                    std::fs::write(root.join("effect"), "happened once").unwrap();
                }
                harness.finish(&agent, &thread, &mut events).unwrap();
                let recovered = events.last().unwrap();
                assert!(matches!(
                    recovered.evidence.as_ref().map(|e| &e.record),
                    Some(Record::Finished {
                        offset_ns: None,
                        outcome: Outcome::Unknown { .. },
                        ..
                    })
                ));
                let count = events.len();
                harness.finish(&agent, &thread, &mut events).unwrap();
                assert_eq!(events.len(), count);
                assert_eq!(store.read(&agent, &thread).unwrap(), events);
                if dispatched {
                    assert_eq!(
                        std::fs::read_to_string(root.join("effect")).unwrap(),
                        "happened once"
                    );
                }
            }
        }
    }
}

//! Export snapshots retain partial storage boundaries as explicit gaps.

use super::*;
use link_harness_wire::evidence::export::{Gap, ReadState};
use link_test_kit::TempDir;

fn fixture(name: &str) -> (TempDir, Files, Agent, Thread) {
    let root = TempDir::new(name);
    let agent = Agent::new("main").unwrap();
    let thread = Thread::new("link:snapshot").unwrap();
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        root.join("harness"),
        Folder {
            home: root.join("home"),
            agent: root.join("own"),
        },
    );
    (root, Files::new(agents), agent, thread)
}

#[test]
fn a_log_snapshot_reports_a_partial_tail_and_refuses_a_malformed_whole_line() {
    let (root, store, agent, thread) = fixture("snapshot-log");
    let event = said(1, "retained");
    store.append(&agent, &thread, &event, &[]).unwrap();
    assert_eq!(
        store.snapshot(&agent, &thread).unwrap().read,
        ReadState::Complete
    );
    let path = root.join("own/chats/link-snapshot/events.jsonl");
    let mut log = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    log.write_all(b"{\"seq\":2").unwrap();
    let snapshot = store.snapshot(&agent, &thread).unwrap();
    assert_eq!(snapshot.events, [event]);
    assert_eq!(
        snapshot.read,
        ReadState::Incomplete {
            gaps: vec![Gap::PartialLogTail { bytes: 8 }],
        }
    );
    assert_eq!(store.read(&agent, &thread).unwrap().len(), 1);
    log.write_all(b"\n").unwrap();
    assert!(store.snapshot(&agent, &thread).is_err());
}

#[test]
fn a_compact_snapshot_reports_both_incomplete_frame_and_tail() {
    let (root, store, agent, thread) = fixture("snapshot-compact");
    let event = said(1, "retained");
    store.append(&agent, &thread, &event, &[]).unwrap();
    store.move_conversations(&agent, &Way::Compact).unwrap();
    assert_eq!(
        store.snapshot(&agent, &thread).unwrap().read,
        ReadState::Complete
    );
    let folder = root.join("own/chats/link-snapshot");
    std::fs::OpenOptions::new()
        .append(true)
        .open(folder.join("events.zst"))
        .unwrap()
        .write_all(b"torn")
        .unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(folder.join(LOG))
        .unwrap()
        .write_all(b"{\"seq\":2")
        .unwrap();
    let snapshot = store.snapshot(&agent, &thread).unwrap();
    assert_eq!(snapshot.events, [event]);
    assert_eq!(
        snapshot.read,
        ReadState::Incomplete {
            gaps: vec![
                Gap::PartialCompactFrames { bytes: 4 },
                Gap::PartialLogTail { bytes: 8 },
            ]
        }
    );
}

#[test]
fn a_retained_suffix_is_not_a_complete_conversation_snapshot() {
    let (root, store, agent, thread) = fixture("snapshot-prefix");
    store.append(&agent, &thread, &said(1, "one"), &[]).unwrap();
    let suffix = said(5, "suffix");
    std::fs::write(
        root.join("own/chats/link-snapshot/events.jsonl"),
        link_files::saved::line(&suffix).unwrap(),
    )
    .unwrap();
    let snapshot = store.snapshot(&agent, &thread).unwrap();
    assert_eq!(snapshot.events, [suffix]);
    assert_eq!(
        snapshot.read,
        ReadState::Incomplete {
            gaps: vec![Gap::MissingPrefix { first_seq: 5 }],
        }
    );
}

#[test]
fn a_copy_failure_does_not_report_a_committed_event_as_uncommitted() {
    use link_harness_wire::evidence::{CallRef, Evidence, Phase, Proposal, ProposedCall, Record};
    let (root, store, agent, thread) = fixture("snapshot-copy-fault");
    let call = event::Call {
        id: "provider".into(),
        name: "file__read".into(),
        arguments: serde_json::json!({"path":"note.md"}),
    };
    let proposed = Event {
        seq: 1,
        role: event::ASSISTANT.into(),
        calls: vec![call.clone()],
        evidence: Some(
            Evidence::new(Record::Proposed(Proposal {
                phase: Phase::Turn,
                attempt: "attempt".into(),
                work: None,
                started_unix_ms: 0,
                calls: vec![ProposedCall {
                    provider_id: "provider".into(),
                    call,
                }],
            }))
            .into(),
        ),
        ..Event::default()
    };
    store.append(&agent, &thread, &proposed, &[]).unwrap();
    let copy = root.join("own/chats/link-snapshot/conversation.md");
    std::fs::remove_file(&copy).unwrap();
    std::fs::create_dir(&copy).unwrap();
    let marker = Event {
        seq: 2,
        role: event::EVIDENCE.into(),
        evidence: Some(
            Evidence::new(Record::Dispatched {
                call: CallRef {
                    proposal: 1,
                    ordinal: 0,
                },
                offset_ns: 0,
                request: link_harness_wire::evidence::McpDispatch {
                    server: "fixture".into(),
                    operation: "read".into(),
                    arguments: link_harness_wire::evidence::Captured::Retained {
                        value: serde_json::json!({}),
                    },
                },
            })
            .into(),
        ),
        ..Event::default()
    };
    store.append(&agent, &thread, &marker, &[]).unwrap();
    let events = store.read(&agent, &thread).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1], marker);
    assert!(store.append(&agent, &thread, &marker, &[]).is_err());
    std::fs::remove_dir(&copy).unwrap();
    let answer = Event {
        seq: 3,
        role: event::ASSISTANT.into(),
        ..Event::user("done")
    };
    store.append(&agent, &thread, &answer, &events).unwrap();
    assert_eq!(store.read(&agent, &thread).unwrap().len(), 3);
    assert!(std::fs::read_to_string(copy).unwrap().contains("done"));
}

#[test]
fn internal_summary_records_stay_out_of_incremental_and_rebuilt_copies() {
    let (root, store, agent, thread) = fixture("snapshot-internal-copy");
    let answer = |seq, text| Event {
        seq,
        role: event::ASSISTANT.into(),
        ..Event::user(text)
    };
    let internal = |seq, text| Event {
        seq,
        role: event::EVIDENCE.into(),
        ..Event::user(text)
    };
    let mut events = vec![
        said(1, "owner's first message"),
        answer(2, "first visible answer"),
        internal(3, "internal summary proposal"),
        said(4, "owner's next message"),
        answer(5, "next visible answer"),
    ];
    for (index, event) in events.iter().enumerate() {
        store
            .append(&agent, &thread, event, &events[..index])
            .unwrap();
    }
    let path = root.join("own/chats/link-snapshot/conversation.md");
    let incremental = std::fs::read_to_string(&path).unwrap();
    assert!(incremental.contains("owner's next message"));
    assert!(incremental.contains("next visible answer"));
    assert!(!incremental.contains("internal summary proposal"));

    // A stale copy forces the complete projection at the next answer.
    std::fs::write(&path, "stale copy").unwrap();
    for event in [
        internal(6, "internal summary result"),
        answer(7, "last visible answer"),
    ] {
        store.append(&agent, &thread, &event, &events).unwrap();
        events.push(event);
    }
    let rebuilt = std::fs::read_to_string(path).unwrap();
    assert!(rebuilt.contains("first visible answer"));
    assert!(rebuilt.contains("owner's next message"));
    assert!(rebuilt.contains("last visible answer"));
    assert!(!rebuilt.contains("internal summary"));
    assert_eq!(store.read(&agent, &thread).unwrap(), events);
}

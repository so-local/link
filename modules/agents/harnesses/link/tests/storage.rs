//! Where a conversation is kept, as its agent's spec says (`[storage]
//! conversations`): the log, compact, or a store driver (`lnk-sqlite-store`,
//! from `link-core`); read where it's kept whatever the spec says now,
//! and moved by hand.

use crate::common;

use std::path::Path;
use std::time::Duration;

use link_agent_spec::{Spec, Storage};
use link_harness::{
    agent::{Agent, Agents, Thread},
    config::Config,
    event::{self, Event},
    store::{Files, Store, Took, Way},
};
use link_test_kit::TempDir;

/// An agent `main` whose files are under a folder of `name`'s.
fn world(name: &str) -> (TempDir, Agent, Files) {
    let root = TempDir::new(&format!("storage-{name}"));
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        root.join("home"),
        common::flat(root.join("files")),
    );
    (root, agent, Files::new(agents))
}

/// Keeps `agent`'s spec saying new conversations are kept `way`.
fn keep(store: &Files, agent: &Agent, way: &str) {
    let mut spec: Spec = link_agent_spec::link();
    spec.storage = Some(Storage {
        conversations: Some(way.into()),
        ..Storage::default()
    });
    let config = Config {
        spec: Some(spec),
        ..Config::default()
    };
    store.set_config(agent, &config).unwrap();
}

/// The SQLite driver, found as Link Harness finds a driver: on the PATH.
pub(crate) fn with_sqlite() {
    static FOUND: std::sync::Once = std::sync::Once::new();
    FOUND.call_once(|| {
        let dir = common::sqlite().parent().unwrap().to_path_buf();
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path =
            std::env::join_paths(std::iter::once(dir).chain(std::env::split_paths(&path))).unwrap();
        // SAFETY: set once, before any test that runs a driver reads it.
        unsafe { std::env::set_var("PATH", path) };
    });
}

fn said(seq: u64, text: &str) -> Event {
    Event {
        seq,
        ..Event::user(text)
    }
}

fn answer(seq: u64, text: &str) -> Event {
    Event {
        seq,
        role: event::ASSISTANT.into(),
        text: text.into(),
        at: event::now(),
        ..Event::default()
    }
}

/// `n` events, a question and its answer in turn, appended.
fn talk(store: &Files, agent: &Agent, thread: &Thread, n: u64) {
    for seq in 1..=n {
        let event = match seq % 2 {
            1 => said(seq, &format!("Question {seq}")),
            _ => answer(seq, &format!("Answer {seq}")),
        };
        store.append(agent, thread, &event, &[]).unwrap();
    }
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn texts(events: &[Event]) -> Vec<String> {
    events.iter().map(|e| e.text.clone()).collect()
}

/// The stopped adapter uses the same event/export boundary on every store.
#[test]
fn stopped_state_export_preserves_native_evidence_on_every_conversation_store() {
    use link_files::reading::ContentDigest;
    use link_harness_wire::evidence::{
        CallRef, Captured, Evidence, Phase, Proposal, ProposedCall, Record,
        export::{Gap, ReadState},
        index::{Located, index_with_responses},
        request::{MediaDelivery, ModelRequest, Projected, RequestProjection, RequestTool},
    };
    use link_store_contract::store::{Driver, Store as _};
    use std::io::Write;
    let sqlite = common::sqlite();
    if !link_test_kit::gate::program(sqlite.to_str().unwrap(), &["--version"]) {
        return;
    }
    for way in ["log", "compact", "sqlite"] {
        let root = TempDir::under(
            Path::new(env!("CARGO_TARGET_TMPDIR")),
            &format!("offline-export-{way}"),
        );
        if !link_test_kit::gate::out_of_tmp(&root) {
            return;
        }
        let agent = Agent::new("main").unwrap();
        let mut agents = Agents::default();
        agents.add(
            agent.clone(),
            root.join("home"),
            common::flat(root.join("files")),
        );
        let store = Files::new(agents);
        keep(&store, &agent, way);
        let thread = Thread::new("link:main").unwrap();
        let call = event::Call {
            id: "provider".into(),
            name: "memory__remember".into(),
            arguments: serde_json::json!({"path":"notes.md","text":"fixture note"}),
        };
        let requested = Event {
            seq: 1,
            role: event::EVIDENCE.into(),
            evidence: Some(
                Evidence::new(Record::ModelRequested {
                    request: ModelRequest {
                        phase: Phase::Summary,
                        attempt: "fixture-attempt".into(),
                        work: None,
                        model: Some("fixture-model".into()),
                        provider: "fixture".into(),
                        system: ContentDigest::of(b"fixture summary system"),
                        tools: vec![RequestTool {
                            name: call.name.clone(),
                            declaration: ContentDigest::of(b"fixture declaration"),
                        }],
                        no_calls: false,
                        projection: Captured::Retained {
                            value: RequestProjection::Whole(Projected::default()),
                        },
                        media_delivery: MediaDelivery::CanonicalOnly,
                    },
                })
                .into(),
            ),
            ..Event::default()
        };
        let event = Event {
            seq: 2,
            role: event::EVIDENCE.into(),
            request: Some(1),
            model: Some("fixture-model".into()),
            evidence: Some(
                Evidence::new(Record::Proposed(Proposal {
                    phase: Phase::Summary,
                    attempt: "fixture-attempt".into(),
                    work: None,
                    started_unix_ms: 7,
                    calls: vec![ProposedCall {
                        provider_id: "provider".into(),
                        call,
                    }],
                }))
                .into(),
            ),
            ..Event::default()
        };
        if way == "sqlite" {
            let chats = root.join("files/chats");
            let folder = chats.join(thread.folder());
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("store.sqlite"), "").unwrap();
            let driver = Driver::at("sqlite", sqlite.clone(), &chats);
            let whose = link_store_contract::store::Agent {
                id: "main".into(),
                owner: "".into(),
            };
            for event in [&requested, &event] {
                let value =
                    serde_json::from_slice(&link_files::saved::print_json(event).unwrap()).unwrap();
                assert_eq!(
                    driver.append(&whose, thread.name(), &value).unwrap(),
                    link_store_contract::store::Appended::Written
                );
            }
        } else {
            store.append(&agent, &thread, &requested, &[]).unwrap();
            store
                .append(&agent, &thread, &event, std::slice::from_ref(&requested))
                .unwrap();
            std::fs::OpenOptions::new()
                .append(true)
                .open(
                    root.join("files/chats")
                        .join(thread.folder())
                        .join("events.jsonl"),
                )
                .unwrap()
                .write_all(b"{\"seq\":3")
                .unwrap();
        }
        let output =
            link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-harness"), &root.join("home"))
                .args(["state", "export", "--json"])
                .env("LNK_FILES", root.join("files"))
                .env("LNK_AGENT", "main")
                .output()
                .unwrap();
        assert!(output.status.success(), "{way}: {output:?}");
        let state: link_agent_state_protocol::State =
            serde_json::from_slice(&output.stdout).unwrap();
        let sessions = state.sessions.unwrap();
        let evidence = sessions[0].native_evidence().unwrap().unwrap();
        assert!(
            sessions[0].updates.is_empty(),
            "internal summary records are not user updates"
        );
        assert_eq!(
            evidence.events[0].evidence.as_ref(),
            requested.evidence.as_deref(),
            "{way}"
        );
        assert_eq!(
            evidence.events[1].evidence.as_ref(),
            event.evidence.as_deref(),
            "{way}"
        );
        assert_eq!(evidence.events[1].model, event.model, "{way}");
        assert_eq!(evidence.events[1].request, Some(1), "{way}");
        let trace = index_with_responses(
            evidence.events.iter().filter_map(|event| {
                event.evidence.as_ref().map(|evidence| Located {
                    seq: event.seq,
                    evidence,
                })
            }),
            evidence
                .events
                .iter()
                .filter_map(|event| event.request.map(|request| (event.seq, request))),
        )
        .unwrap();
        assert!(
            trace
                .producing_request(CallRef {
                    proposal: 2,
                    ordinal: 0
                })
                .is_some(),
            "{way}"
        );
        match way {
            "sqlite" => assert!(matches!(evidence.read, ReadState::Unknown { .. })),
            _ => assert_eq!(
                evidence.read,
                ReadState::Incomplete {
                    gaps: vec![Gap::PartialLogTail { bytes: 8 }]
                }
            ),
        }
    }
}

/// A fork preserves the immutable prefix on every store, including the
/// hidden provider blocks and window marks that make its cache reusable.
#[test]
fn forks_keep_the_exact_prefix_and_never_widen_tools() {
    with_sqlite();
    for way in ["log", "compact", "sqlite"] {
        let (_root, agent, store) = world(&format!("fork-{way}"));
        keep(&store, &agent, way);
        let thread = Thread::new("link:main").unwrap();
        let mut events = vec![
            said(1, "Find it"),
            Event {
                seq: 2,
                role: event::ASSISTANT.into(),
                calls: vec![event::Call {
                    id: "call-stable".into(),
                    name: "memory__search".into(),
                    arguments: serde_json::json!({"q":"notes"}),
                }],
                thinking: vec![
                    serde_json::json!({"type":"thinking","thinking":"kept","signature":"original-signature"}),
                ],
                at: event::now(),
                ..Event::default()
            },
            Event {
                seq: 3,
                role: event::TOOL.into(),
                text: "found".into(),
                call: Some("call-stable".into()),
                tool: Some("memory__search".into()),
                at: event::now(),
                ..Event::default()
            },
            Event {
                seq: 4,
                role: event::SUMMARY.into(),
                text: "Earlier context".into(),
                covers: 2,
                at: event::now(),
                ..Event::default()
            },
            answer(5, "Here it is"),
            said(6, "Original question"),
            answer(7, "Original answer"),
        ];
        events[0].now = Some("original time and thread".into());
        events[4].response_id = Some("provider-response".into());
        events[4].window = Some(link_harness::window::Mark::at(&events[0], 1000));
        for (i, event) in events.iter().enumerate() {
            store.append(&agent, &thread, event, &events[..i]).unwrap();
        }
        store
            .narrow_tools(&agent, &thread, &["memory__search".into()])
            .unwrap();
        let forked = store.fork_conversation(&agent, &thread, 6).unwrap();
        let branch = Thread::new(&forked.thread.to_string()).unwrap();
        assert_ne!(branch, thread);
        let copied = store.read(&agent, &branch).unwrap();
        assert_eq!(copied, events[..5], "{way}");
        assert_eq!(
            store.read(&agent, &thread).unwrap(),
            events,
            "source changed: {way}"
        );
        assert_eq!(
            store.tools(&agent, &branch).unwrap(),
            Some(vec!["memory__search".into()])
        );
        assert_eq!(
            link_harness::model::messages(&copied, true, &|_| anyhow::bail!("not read")),
            link_harness::model::messages(&events[..5], true, &|_| anyhow::bail!("not read"))
        );
        assert_eq!(forked.messages.len(), 2, "hidden events exposed: {way}");
        assert!(
            store.fork_conversation(&agent, &thread, 2).is_err(),
            "assistant event accepted"
        );
        assert!(store.fork_conversation(&agent, &thread, 999).is_err());
        let empty = store.fork_conversation(&agent, &thread, 1).unwrap();
        assert!(empty.messages.is_empty());
        assert!(
            store
                .list_conversations(&agent)
                .unwrap()
                .threads
                .contains(&empty.thread),
            "empty fork was lost: {way}"
        );
    }
}

/// A new conversation is kept as the spec says; one kept already stays
/// where it is when the spec changes, read there; a move puts each where
/// the spec says, what else its folder held with it.
#[test]
fn each_conversation_is_kept_as_it_started_until_moved() {
    let (root, agent, store) = world("compact");
    keep(&store, &agent, "compact");
    let small = Thread::new("telegram:42").unwrap();
    talk(&store, &agent, &small, 70);
    let folder = root.join("files/chats/telegram-42");
    assert_eq!(names(&folder), ["events.jsonl", "events.zst"]);
    let events = store.read(&agent, &small).unwrap();
    assert_eq!(events.len(), 70);
    assert_eq!(
        texts(&store.read_from(&agent, &small, 69).unwrap()),
        ["Question 69", "Answer 70"]
    );
    assert_eq!(store.last_events(&agent, &small, 3).unwrap().len(), 3);
    // Read as a person reads it, with no copy on disk.
    let copy = store.conversation(&agent, &small).unwrap();
    assert!(copy.starts_with("# telegram:42\n"), "{copy}");
    assert!(copy.contains("Answer 70"), "{copy}");
    store
        .narrow_tools(&agent, &small, &["memory__search".into()])
        .unwrap();

    // The spec says the log now: the compact one stays compact, a new
    // one is the log.
    keep(&store, &agent, "log");
    let new = Thread::new("slack:C1").unwrap();
    talk(&store, &agent, &new, 2);
    assert_eq!(
        names(&root.join("files/chats/slack-C1")),
        ["conversation.md", "events.jsonl"]
    );
    store
        .append(&agent, &small, &said(71, "Still compact"), &[])
        .unwrap();
    assert!(!folder.join("conversation.md").exists());

    // Moved: the compact one is the log now, its events and tools kept.
    let moved = store.move_conversations(&agent, &Way::Log).unwrap();
    assert_eq!(moved, [(small.clone(), Way::Compact)]);
    assert_eq!(
        names(&folder),
        ["conversation.md", "events.jsonl", "tools.json"]
    );
    assert_eq!(store.read(&agent, &small).unwrap().len(), 71);
    assert_eq!(
        store.tools(&agent, &small).unwrap(),
        Some(vec!["memory__search".to_string()])
    );
    let md = std::fs::read_to_string(folder.join("conversation.md")).unwrap();
    assert!(md.contains("Answer 70"), "{md}");
    // And back: nothing left over from either.
    let moved = store.move_conversations(&agent, &Way::Compact).unwrap();
    assert_eq!(moved.len(), 2);
    assert_eq!(names(&folder), ["events.jsonl", "events.zst", "tools.json"]);
    assert_eq!(
        texts(&store.read(&agent, &small).unwrap())[70],
        "Still compact"
    );
    assert!(
        names(&root.join("files/chats"))
            .iter()
            .all(|n| !n.starts_with('.'))
    );
}

/// A store driver keeps a conversation's events and its lease: the
/// folder holds only its marker, and what isn't an event.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_driver_keeps_events_and_the_lease() {
    with_sqlite();
    let (root, agent, store) = world("sqlite");
    keep(&store, &agent, "sqlite");
    let thread = Thread::new("telegram:42").unwrap();
    assert_eq!(store.take_turn(&agent, &thread, "a").unwrap(), Took::Took);
    assert!(store.turn_running(&agent, &thread));
    assert_eq!(store.take_turn(&agent, &thread, "b").unwrap(), Took::Held);
    talk(&store, &agent, &thread, 6);
    let err = store
        .append(&agent, &thread, &said(3, "again"), &[])
        .unwrap_err();
    assert!(err.to_string().contains("another turn came first"), "{err}");
    store.renew_turn(&agent, &thread, "a").unwrap();
    assert!(store.renew_turn(&agent, &thread, "b").is_err());
    store.end_turn(&agent, &thread, "a").unwrap();
    assert!(!store.turn_running(&agent, &thread));
    let folder = root.join("files/chats/telegram-42");
    assert_eq!(names(&folder), ["store.sqlite"]);
    assert!(root.join("files/chats/events.sqlite").is_file());
    assert_eq!(
        texts(&store.read_from(&agent, &thread, 6).unwrap()),
        ["Answer 6"]
    );
    assert!(
        store
            .conversation(&agent, &thread)
            .unwrap()
            .contains("Answer 6")
    );
    // A lease not renewed lapses: its turn is taken over.
    keep_lapse(&store, &agent, "sqlite", "1s");
    assert_eq!(store.take_turn(&agent, &thread, "c").unwrap(), Took::Took);
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(store.lapsed_turns(&agent), std::slice::from_ref(&thread));
    assert_eq!(store.take_turn(&agent, &thread, "d").unwrap(), Took::Lapsed);
    store.end_turn(&agent, &thread, "d").unwrap();

    // Moved out of the driver, and its rows with it.
    let moved = store.move_conversations(&agent, &Way::Log).unwrap();
    assert_eq!(moved, [(thread.clone(), Way::Driver("sqlite".into()))]);
    assert_eq!(names(&folder), ["conversation.md", "events.jsonl"]);
    assert_eq!(store.read(&agent, &thread).unwrap().len(), 6);
    let (driver, whose) = store.driver(&agent, "sqlite").unwrap();
    assert!(driver.threads(&whose).unwrap().is_empty());
    // And into it again.
    store
        .move_conversations(&agent, &Way::Driver("sqlite".into()))
        .unwrap();
    assert_eq!(names(&folder), ["store.sqlite"]);
    assert_eq!(store.read(&agent, &thread).unwrap().len(), 6);
}

/// Behind the store bridge, as Link runs it outside the sandbox: each of
/// the driver's calls goes through it, proved with the agent's key, and
/// the harness runs no driver; with a key that isn't the agent's, nothing
/// is read or kept.
#[test]
fn a_driver_behind_the_store_bridge_keeps_events_and_the_lease() {
    let root = TempDir::new("storage-bridged");
    let agent = Agent::with_identity("main", "aaaaaaaaaaaa", "me@box").unwrap();
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        root.join("home"),
        common::flat(root.join("files")),
    );
    let store = Files::new(agents);
    // The bridge: the driver run outside, counting its calls.
    let conversations = root.join("files/chats");
    let driver = link_store_contract::store::Driver::at("sqlite", common::sqlite(), &conversations);
    let keys = root.join("keys");
    link_store_contract::testing::bound(&keys, agent.id());
    let socket = root.join("store.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = calls.clone();
    std::thread::spawn(move || {
        link_store_contract::store::serve_bridge(
            listener,
            std::sync::Arc::new(link_sandbox_binding::Keyring::at(&keys)),
            std::sync::Arc::new(move |_: &link_sandbox_binding::Identity| {
                counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(driver.clone())
            }),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    });
    let mut spec: Spec = link_agent_spec::link();
    spec.storage = Some(Storage {
        conversations: Some("sqlite".into()),
        ..Storage::default()
    });
    let config = |key: &str| Config {
        spec: Some(spec.clone()),
        endpoint_key: Some(key.to_string().into()),
        store: Some(link_store_contract::store::BridgeAt {
            driver: "sqlite".into(),
            socket: socket.clone(),
        }),
        ..Config::default()
    };
    store
        .set_config(&agent, &config(link_store_contract::testing::TEST_KEY))
        .unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    assert_eq!(store.take_turn(&agent, &thread, "a").unwrap(), Took::Took);
    talk(&store, &agent, &thread, 4);
    store.end_turn(&agent, &thread, "a").unwrap();
    assert_eq!(
        texts(&store.read_from(&agent, &thread, 4).unwrap()),
        ["Answer 4"]
    );
    assert!(conversations.join("events.sqlite").is_file());
    assert!(calls.load(std::sync::atomic::Ordering::Relaxed) >= 7);
    // A key that isn't the agent's proves nothing: nothing read.
    store.set_config(&agent, &config(&"0".repeat(64))).unwrap();
    let refused = store.read_from(&agent, &thread, 1).unwrap_err();
    assert!(
        format!("{refused:#}").contains("proof isn't"),
        "{refused:#}"
    );
}

/// Handed the storage bridge, as `lnk agent start` hands it on every
/// machine, Link Harness reaches each collection its machine keeps
/// elsewhere only through it, and keeps those at `file` in its folder
/// itself, as `lnk-file-store` keeps them: here a machine that lists only
/// `sqlite` for conversations, whose bridge answers each collection by
/// its place's driver. Its instructions are read from its folder, its
/// conversations are rows of the bridge's SQLite file, and nothing of
/// them is a file in its folder.
#[test]
fn a_collection_kept_elsewhere_goes_through_the_storage_bridge() {
    let root = TempDir::new("storage-every");
    let agent = Agent::with_identity("main", "aaaaaaaaaaaa", "me@box").unwrap();
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        root.join("home"),
        common::flat(root.join("files")),
    );
    let store = Files::new(agents);
    std::fs::create_dir_all(root.join("files/instructions")).unwrap();
    std::fs::write(
        root.join("files/instructions/instruction.md"),
        "From the folder.",
    )
    .unwrap();
    let machine: link_store_contract::storage::Supported =
        serde_json::from_str(r#"{"conversations": ["sqlite"]}"#).unwrap();
    let places = machine.resolve(&Storage::default());
    // The bridge's drivers: `lnk-file-store` in its folder, `lnk-sqlite-store` in
    // another, so what's read is plainly where its place keeps it.
    let kept = root.join("kept");
    std::fs::create_dir_all(&kept).unwrap();
    let file =
        link_store_contract::store::Driver::at("file", common::file_driver(), &root.join("files"))
            .with_env(link_store_contract::store::AGENT_ENV, agent.id());
    let sqlite = link_store_contract::store::Driver::at("sqlite", common::sqlite(), &kept);
    let routed = places.clone();
    let keys = root.join("keys");
    link_store_contract::testing::bound(&keys, agent.id());
    let socket = root.join("storage.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    std::thread::spawn(move || {
        link_store_contract::store::serve_storage(
            listener,
            std::sync::Arc::new(link_sandbox_binding::Keyring::at(&keys)),
            std::sync::Arc::new(
                move |_: &link_sandbox_binding::Identity,
                      collection,
                      _: &link_store_contract::store::Request| {
                    Ok(match routed.of(collection) {
                        "sqlite" => sqlite.clone(),
                        _ => file.clone(),
                    })
                },
            ),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    });
    let config = Config {
        spec: Some(link_agent_spec::link()),
        endpoint_key: Some(link_store_contract::testing::TEST_KEY.to_string().into()),
        storage: Some(link_store_contract::storage::StorageAt {
            places,
            socket: Some(socket.clone()),
        }),
        ..Config::default()
    };
    store.set_config(&agent, &config).unwrap();

    assert_eq!(
        store
            .instruction(&agent, "instructions/instruction.md")
            .unwrap()
            .as_deref(),
        Some("From the folder.")
    );
    let thread = Thread::new("telegram:42").unwrap();
    assert_eq!(store.take_turn(&agent, &thread, "a").unwrap(), Took::Took);
    talk(&store, &agent, &thread, 2);
    store.end_turn(&agent, &thread, "a").unwrap();
    store
        .narrow_tools(&agent, &thread, &["web__search".into()])
        .unwrap();
    assert_eq!(
        texts(&store.read_from(&agent, &thread, 1).unwrap()),
        ["Question 1", "Answer 2"]
    );
    assert!(kept.join("events.sqlite").is_file());
    assert!(!root.join("files/chats/telegram-42").exists());
    // Printed as people read it, though no folder of it is here.
    let printed = store.conversation(&agent, &thread).unwrap();
    assert!(printed.contains("Question 1"), "{printed}");
    let none = Thread::new("telegram:43").unwrap();
    assert!(store.conversation(&agent, &none).is_err());
}

/// Keeps `agent`'s spec saying `way`, and that a lease lasts `lapse`.
fn keep_lapse(store: &Files, agent: &Agent, way: &str, lapse: &str) {
    keep(store, agent, way);
    let mut config = store.config(agent).unwrap();
    let spec = config.spec.as_mut().unwrap();
    spec.turns.get_or_insert_default().lapse = Some(lapse.parse().unwrap());
    store.set_config(agent, &config).unwrap();
}

/// Compact against the log, for 2,000 events: bytes, blocks, and the time
/// to read every event. Prints what it measured; run by hand:
/// `cargo test --release --test storage -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, run by hand with --release"]
fn compact_against_the_log() {
    use std::os::unix::fs::MetadataExt;
    let n = 2_000;
    for way in ["log", "compact"] {
        let (root, agent, store) = world(&format!("measure-{way}"));
        keep(&store, &agent, way);
        let thread = Thread::new("telegram:42").unwrap();
        // Words of English, picked as a person might: a few hundred
        // characters a message, an answer a few times that.
        const WORDS: &[&str] = &[
            "the",
            "plan",
            "meeting",
            "tomorrow",
            "flight",
            "hotel",
            "budget",
            "report",
            "draft",
            "send",
            "call",
            "notes",
            "about",
            "which",
            "could",
            "would",
            "should",
            "maybe",
            "Albuquerque",
            "Thursday",
            "invoice",
            "contract",
            "review",
            "and",
            "or",
            "but",
            "because",
            "after",
            "before",
            "client",
            "project",
            "deadline",
            "schedule",
            "room",
            "car",
            "dinner",
            "booked",
            "changed",
            "moved",
            "found",
            "checked",
            "remember",
            "list",
            "files",
            "folder",
            "version",
            "price",
            "dollars",
            "minutes",
            "hours",
        ];
        let mut seed: u64 = 42;
        let mut words = |n: usize| -> String {
            (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    WORDS[(seed >> 33) as usize % WORDS.len()]
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut events = Vec::new();
        let started = std::time::Instant::now();
        for seq in 1..=n {
            let mut event = match seq % 2 {
                1 => said(seq, &words(40)),
                _ => answer(seq, &words(120)),
            };
            if event.role == event::ASSISTANT {
                event.model = Some("claude-sonnet-5-5".into());
                event.provider = Some("anthropic".into());
                event.response_id = Some(format!("msg_{seq:024}"));
                event.tokens = Some(event::Tokens {
                    input: 1_000 + seq,
                    output: 200,
                    cached: 900,
                });
                event.latency_ms = Some(1_500);
                event.seen = seq - 1;
            }
            store.append(&agent, &thread, &event, &events).unwrap();
            events.push(event);
        }
        let written = started.elapsed();
        let folder = root.join("files/chats/telegram-42");
        let (mut bytes, mut blocks) = (0, 0);
        for name in names(&folder) {
            let meta = std::fs::metadata(folder.join(&name)).unwrap();
            bytes += meta.len();
            blocks += meta.blocks() * 512 / 4096;
        }
        let started = std::time::Instant::now();
        let read = store.read(&agent, &thread).unwrap();
        let reading = started.elapsed();
        let started = std::time::Instant::now();
        let tail = store.last_events(&agent, &thread, 32).unwrap();
        let last = started.elapsed();
        assert_eq!((read.len(), tail.len()), (n as usize, 32));
        println!(
            "{way}: {bytes} bytes, {blocks} blocks of 4 KB, written in {written:?}, \
             read whole in {reading:?}, the last 32 in {last:?} ({:?})",
            names(&folder)
        );
    }
}

/// A turn's dozen calls to the SQLite driver, through one process kept
/// for them all against one process a call: the time a turn's calls
/// take, each way. Prints what it measured; run by hand:
/// `cargo test --release --test harness storage::a_sqlite_turn -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, run by hand with --release"]
fn a_sqlite_turn_kept_against_one_process_a_call() {
    use link_store_contract::store::{Agent as Whose, Driver, Reply, Request};
    /// The driver as it was run before: one process a call.
    struct OneACall(Driver);
    impl link_store_contract::store::Store for OneACall {
        fn name(&self) -> &str {
            &self.0.name
        }
        fn call(&self, request: &Request) -> anyhow::Result<Reply> {
            self.0.call_once(request)
        }
    }
    let turns = 50;
    let whose = Whose {
        id: "aaaaaaaaaaaa".into(),
        owner: "me@box".into(),
    };
    let turn = |store: &dyn link_store_contract::store::Store, n: u64| {
        let lapse = Duration::from_secs(60);
        store.take(&whose, "t", "turn", lapse).unwrap();
        let last = store.last(&whose, "t").unwrap();
        store.read(&whose, "t", last.saturating_sub(8) + 1).unwrap();
        store.tools_read(&whose, "t").unwrap();
        for seq in last + 1..=last + 4 {
            let event =
                serde_json::json!({"seq": seq, "role": "user", "text": format!("turn {n}")});
            store.append(&whose, "t", &event).unwrap();
            store.renew(&whose, "t", "turn").unwrap();
        }
        store.last(&whose, "t").unwrap();
        store.end(&whose, "t", "turn").unwrap();
    };
    for way in ["one process a call", "one process kept"] {
        let root = TempDir::new("measure-sqlite-turn");
        let driver = Driver::at("sqlite", common::sqlite(), &root);
        let store: Box<dyn link_store_contract::store::Store> = match way {
            "one process a call" => Box::new(OneACall(driver)),
            _ => Box::new(driver),
        };
        turn(&*store, 0);
        let started = std::time::Instant::now();
        for n in 1..=turns {
            turn(&*store, n);
        }
        println!(
            "{way}: a turn of 14 calls in {:?}",
            started.elapsed() / turns as u32
        );
    }
}

/// A conversation narrowed before its first event (a subagent whose turn
/// crashed at once) names its thread in its folder, though the folder's
/// name can't (`:` is `-` there): a move reads it back as itself, still
/// narrow, never as its folder's name with every tool.
#[test]
fn a_conversation_narrowed_before_its_first_event_keeps_its_name() {
    let (root, agent, store) = world("log");
    let thread = Thread::new("task:b1").unwrap();
    store
        .narrow_tools(&agent, &thread, &["web__search".into()])
        .unwrap();
    let folder = root.join("files/chats/task-b1");
    assert_eq!(names(&folder), ["conversation.md", "tools.json"]);
    let files: std::collections::BTreeMap<String, Vec<u8>> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|f| {
            let f = f.unwrap();
            (
                f.file_name().to_string_lossy().into_owned(),
                std::fs::read(f.path()).unwrap(),
            )
        })
        .collect();
    let moved = link_store_contract::store::folder::Conversation::from_files("task-b1", &files)
        .unwrap()
        .unwrap();
    assert_eq!(moved.thread, "task:b1");
    assert_eq!(moved.tools, Some(vec!["web__search".to_string()]));
    // Its first event after: kept as the spec says, still narrow.
    talk(&store, &agent, &thread, 2);
    assert_eq!(
        store.tools(&agent, &thread).unwrap(),
        Some(vec!["web__search".to_string()])
    );
}

/// A copy kept beside a thread, named for the harness it came from, is
/// listed with the owner's Link threads; another channel's isn't.
#[test]
fn a_copy_beside_a_link_thread_is_listed() {
    let (_root, agent, store) = world("copy-listed");
    keep(&store, &agent, "log");
    for name in ["link:main", "link:main~hermes", "telegram:42"] {
        talk(&store, &agent, &Thread::new(name).unwrap(), 2);
    }
    let listed = store.list_conversations(&agent).unwrap().threads;
    let names: Vec<&str> = listed.iter().map(|thread| thread.as_str()).collect();
    assert_eq!(names, ["link:main", "link:main~hermes"]);
}

/// A conversation kept before its machine kept them off it is read
/// where it is, as its files say, until it's moved: never through the
/// place, which holds none of it, so a move takes its events whole.
#[test]
fn a_conversation_kept_before_is_read_where_it_is() {
    let (_root, agent, store) = world("kept-before");
    keep(&store, &agent, "log");
    let thread = Thread::new("link:main").unwrap();
    talk(&store, &agent, &thread, 2);
    let machine: link_store_contract::storage::Supported =
        serde_json::from_str(r#"{"conversations": ["sqlite"]}"#).unwrap();
    let config = Config {
        spec: Some(link_agent_spec::link()),
        storage: Some(link_store_contract::storage::StorageAt {
            places: machine.resolve(&Storage::default()),
            socket: None,
        }),
        ..Config::default()
    };
    store.set_config(&agent, &config).unwrap();
    assert_eq!(
        texts(&store.read_from(&agent, &thread, 1).unwrap()),
        ["Question 1", "Answer 2"]
    );
}

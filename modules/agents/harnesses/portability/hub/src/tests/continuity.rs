//! Portable handoffs cross the native ACP boundary, including runtimes without
//! native import. These fixtures prove supplied input, not live model behavior.

use super::*;
use link_harness_contract::asp::forms::{By, Message};
use link_harness_contract::harness::{Settings, continuation::Context};

fn personal(dir: &std::path::Path, asked: &Asked, adapter: &str) -> Arc<Hub> {
    Context::configure(dir, adapter, &Settings::default()).unwrap();
    super::hub(dir, asked, 1).0.with_continuity(dir).unwrap()
}

fn history(thread: &str) -> asp::forms::Session {
    asp::forms::Session::of_messages(
        thread.to_string().into(),
        None,
        &[
            Message {
                by: By::User,
                text: "Prepare the delivery. We decided on blue.".into(),
            },
            Message {
                by: By::Agent,
                text: "I will check the invoice before delivery.".into(),
            },
            Message {
                by: By::User,
                text: "Correction: green replaces blue. The invoice check is unfinished.".into(),
            },
        ],
    )
}

#[tokio::test]
async fn portable_history_reaches_the_next_native_turn_and_survives_a_return() {
    let dir = temp("hub-personal-handoff");
    let asked = Asked::default();
    let hub = personal(&dir, &asked, "example");
    let initial = history("telegram:account:group:42:topic:1");
    let imported = hub
        .import(asp::State {
            sessions: Some(vec![initial.clone()]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(imported.sessions[&initial.session_id], initial.session_id);
    assert!(imported.not_taken[0].reason.contains("bounded context"));
    assert!(
        asked.lock().unwrap().is_empty(),
        "import never makes an empty native replacement"
    );
    let at = served(hub.clone()).await;
    say(
        &at,
        &initial.session_id.0,
        "What should you check next?",
        Said::default(),
    )
    .await
    .1
    .unwrap();
    let sent = asked.lock().unwrap().last().unwrap().clone();
    assert!(
        sent.contains("green replaces blue") && sent.contains("invoice check is unfinished"),
        "{sent}"
    );
    let exported = hub.export(vec![asp::Kind::Sessions]).await.unwrap().state;
    let continued = exported
        .sessions
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == initial.session_id)
        .unwrap();
    assert!(continued.updates.starts_with(&initial.updates));
    assert!(
        continued
            .messages()
            .iter()
            .any(|m| m.text == "What should you check next?")
    );
    assert!(
        !continued
            .messages()
            .iter()
            .any(|m| m.text.starts_with("[Portable history")),
        "the seed is never captured as a new user instruction"
    );
    // Return to an already bound native session: import adds missing context
    // without deleting or replacing that session's native binding.
    let binding = hub.sessions.of(&initial.session_id.0).unwrap();
    let mut returned = continued;
    returned.updates.extend(history("unused").updates);
    hub.import(asp::State {
        sessions: Some(vec![returned]),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(hub.sessions.of(&initial.session_id.0).unwrap(), binding);
}

#[tokio::test]
async fn groups_topics_and_accounts_never_share_a_continuation() {
    let dir = temp("hub-separated-context");
    let asked = Asked::default();
    let hub = personal(&dir, &asked, "example");
    let at = served(hub.clone()).await;
    let threads = [
        "telegram:one:group:42:topic:1",
        "telegram:one:group:42:topic:2",
        "telegram:two:group:42:topic:1",
    ];
    for (i, thread) in threads.iter().enumerate() {
        let session = asp::forms::Session::of_messages(
            (*thread).into(),
            None,
            &[Message {
                by: By::User,
                text: format!("private-context-{i}"),
            }],
        );
        hub.import(asp::State {
            sessions: Some(vec![session]),
            ..Default::default()
        })
        .await
        .unwrap();
    }
    for (i, thread) in threads.iter().enumerate() {
        say(&at, thread, "continue", Said::default())
            .await
            .1
            .unwrap();
        let sent = asked.lock().unwrap().last().unwrap().clone();
        for j in 0..threads.len() {
            assert_eq!(
                sent.contains(&format!("private-context-{j}")),
                i == j,
                "{sent}"
            );
        }
    }
}

#[test]
fn portable_capture_replays_exactly_and_corrupt_input_refuses_continuation() {
    let dir = temp("hub-capture-replay");
    let context = Context {
        adapter: "example".into(),
        agent: None,
        max_bytes: 32768,
        memory: Vec::new(),
    };
    let initial = history("link:main");
    {
        let archive = crate::continuity::Archive::at(&dir, context.clone()).unwrap();
        archive.import_all(std::slice::from_ref(&initial)).unwrap();
        archive.import_all(std::slice::from_ref(&initial)).unwrap();
        assert_eq!(
            archive.sessions().unwrap().as_slice(),
            std::slice::from_ref(&initial)
        );
    }
    {
        let archive = crate::continuity::Archive::at(&dir, context.clone()).unwrap();
        assert_eq!(archive.sessions().unwrap(), [initial]);
        assert!(archive.prepare("link:main", 10, false).unwrap().is_some());
    }
    let pending =
        link_harness_contract::harness::continuation::folder(&dir).join("handoffs/pending.new");
    std::fs::write(&pending, "{").unwrap();
    assert!(crate::continuity::Archive::at(&dir, context).is_err());
    assert_eq!(std::fs::read_to_string(pending).unwrap(), "{");
}

#[test]
fn an_invalid_import_group_preserves_every_retained_thread() {
    let dir = temp("hub-atomic-portable-import");
    let context = Context {
        adapter: "example".into(),
        agent: None,
        max_bytes: 32768,
        memory: Vec::new(),
    };
    let archive = crate::continuity::Archive::at(&dir, context).unwrap();
    let initial = history("link:main");
    archive.import_all(std::slice::from_ref(&initial)).unwrap();
    let mut changed = initial.clone();
    changed.updates.extend(history("unused").updates);
    let invalid = history("");
    assert!(archive.import_all(&[changed, invalid]).is_err());
    let mut one = history("telegram:one");
    let original = link_harness_contract::harness::transcript::original(
        "example",
        "native-one",
        "r1",
        serde_json::json!({"value":"original"}),
    );
    one.updates.push(original);
    let mut two = history("telegram:two");
    two.updates
        .push(link_harness_contract::harness::transcript::original(
            "example",
            "native-one",
            "r1",
            serde_json::json!({"value":"changed"}),
        ));
    assert!(archive.import_all(&[one, two]).is_err());
    assert_eq!(archive.sessions().unwrap(), [initial]);
}

#[test]
fn an_archive_survives_a_home_move_and_refuses_a_foreign_agent_or_changed_source() {
    let root = temp("hub-portable-move");
    let from = root.join("from");
    let to = root.join("to");
    let context = Context {
        adapter: "example".into(),
        agent: Some(link_harness_contract::store::Agent {
            id: "a1b2c3d4e5f6".into(),
            owner: "owner@box".into(),
        }),
        max_bytes: 32768,
        memory: Vec::new(),
    };
    let mut session = history("link:main");
    session
        .updates
        .push(link_harness_contract::harness::transcript::original(
            "example",
            "native-one",
            "r1",
            serde_json::json!({"tool":"invoice","result":"pending"}),
        ));
    {
        let archive = crate::continuity::Archive::at(&from, context.clone()).unwrap();
        archive.import_all(std::slice::from_ref(&session)).unwrap();
    }
    std::fs::rename(&from, &to).unwrap();
    {
        let archive = crate::continuity::Archive::at(&to, context.clone()).unwrap();
        assert_eq!(archive.sessions().unwrap(), [session.clone()]);
        let mut changed = session.clone();
        *changed.updates.last_mut().unwrap() = link_harness_contract::harness::transcript::original(
            "example",
            "native-one",
            "r1",
            serde_json::json!({"tool":"invoice","result":"changed"}),
        );
        assert!(archive.backfill(changed).is_err());
        assert_eq!(archive.sessions().unwrap(), [session]);
    }
    let foreign = Context {
        agent: Some(link_harness_contract::store::Agent {
            id: "b1b2c3d4e5f6".into(),
            owner: "owner@box".into(),
        }),
        ..context
    };
    assert!(crate::continuity::Archive::at(&to, foreign).is_err());
}

#[test]
fn a_complete_interrupted_capture_replays_once() {
    let dir = temp("hub-interrupted-portable-capture");
    let context = Context {
        adapter: "example".into(),
        agent: None,
        max_bytes: 32768,
        memory: Vec::new(),
    };
    let initial = history("link:main");
    {
        let archive = crate::continuity::Archive::at(&dir, context.clone()).unwrap();
        archive.import_all(std::slice::from_ref(&initial)).unwrap();
    }
    let folder = link_harness_contract::harness::continuation::folder(&dir).join("handoffs");
    std::fs::rename(
        folder.join("00000000000000000000.json"),
        folder.join("pending.new"),
    )
    .unwrap();
    {
        let archive = crate::continuity::Archive::at(&dir, context.clone()).unwrap();
        assert_eq!(
            archive.sessions().unwrap().as_slice(),
            std::slice::from_ref(&initial)
        );
    }
    let archive = crate::continuity::Archive::at(&dir, context).unwrap();
    assert_eq!(archive.sessions().unwrap(), [initial]);
    assert!(!folder.join("pending.new").exists());
}

#[test]
fn native_backfill_keeps_raw_and_compacted_branches_and_deduplicates_extensions() {
    let dir = temp("hub-native-backfill");
    let context = Context::archival("example").unwrap();
    let archive = crate::continuity::Archive::at(&dir, context).unwrap();
    let mut original = history("link:main");
    original
        .updates
        .push(link_harness_contract::harness::transcript::original(
            "example",
            "native-older-window",
            "r1",
            serde_json::json!({"compacted":true,"unknown":{"evidence":"invoice is pending"}}),
        ));
    archive.backfill(original.clone()).unwrap();
    let mut compacted = history("link:main");
    compacted.updates.truncate(1);
    compacted
        .updates
        .push(link_harness_contract::harness::transcript::original(
            "example",
            "native-current-window",
            "r2",
            serde_json::json!({"summary":"green; check invoice before sending"}),
        ));
    archive.backfill(compacted.clone()).unwrap();
    let mut extended = compacted.clone();
    extended.updates.extend(history("unused").updates);
    archive.backfill(extended.clone()).unwrap();
    archive.backfill(extended.clone()).unwrap();
    let sessions = archive.sessions().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(archive.session("link:main").unwrap(), Some(original));
    assert!(sessions.iter().any(|s| s.updates == extended.updates));
}

//! What a turn reads of its conversation: only what was added since the
//! turn before, however long the conversation is.

use crate::common;
use crate::turns::keep_config;

use std::sync::Arc;

use link_harness::{
    agent::{Agent, Agents, Thread},
    config::Config,
    store::Files,
    turn::Harness,
};
use link_harness_contract::harness::Model;
use link_test_kit::TempDir;
use serde_json::json;

/// A long conversation, its window holding all of it: each model request
/// writes a record listing every message before it, so the log grows
/// with the square of its turns. A turn parses only the lines added
/// since the store last read the log, never the whole log again, so
/// every line is parsed about once however many turns there are.
#[tokio::test]
async fn a_turn_parses_only_what_was_added() {
    let root = TempDir::new("reads-added");
    let (port, _asked) = common::fake_model(|_, n| {
        vec![
            common::delta(json!({"content": format!("Answer {n}")})),
            json!({"id": "gen-1", "choices": [],
                "usage": {"prompt_tokens": 12, "completion_tokens": 2}}),
        ]
    })
    .await;
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    keep_config(
        &home,
        &files,
        Config {
            model: Model {
                provider: "local".into(),
                name: Some("fake".into()),
                base_url: Some(format!("http://127.0.0.1:{port}")),
                ..Model::default()
            },
            ..Config::default()
        },
    );
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(files.clone()));
    let store = Arc::new(Files::new(agents.clone()));
    let harness = Harness::new(agents, store.clone()).unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    let log = files.join("chats/telegram-42/events.jsonl");
    let length = || std::fs::metadata(&log).map_or(0, |meta| meta.len());
    let turns = 20;
    let (mut before, mut parsed) = (0, Vec::new());
    for n in 1..=turns {
        let text = format!("Message {n}: {}", "words ".repeat(20));
        let (pieces, _) = tokio::sync::mpsc::channel(64);
        let at = store.parsed();
        harness.turn(&agent, &thread, &text, pieces).await.unwrap();
        let added = length() - before;
        before = length();
        parsed.push((store.parsed() - at, added));
    }
    // A turn's reads parse what it wrote and what the turn before it
    // wrote after its last read: never more than those two added.
    for (n, pair) in parsed.windows(2).enumerate() {
        let [(_, earlier), (read, added)] = pair else {
            unreachable!()
        };
        assert!(
            *read <= earlier + added,
            "turn {}: parsed {read} bytes, {added} added by it and {earlier} by the one before",
            n + 2,
        );
    }
    // All told, each line once, give or take the last turn's.
    let all: u64 = parsed.iter().map(|(read, _)| read).sum();
    assert!(
        all <= length(),
        "parsed {all} bytes of a {}-byte log",
        length()
    );
}

/// Each model request records only what changed since the one before,
/// so what a turn adds to the log stays the same as the window fills,
/// and the index makes every request whole again: exactly what was sent.
#[tokio::test]
async fn a_turn_adds_the_same_however_full_its_window() {
    use link_harness_wire::evidence::{
        Record,
        index::{Located, index},
    };
    let root = TempDir::new("reads-same");
    let (port, asked) = common::fake_model(|_, n| {
        vec![
            common::delta(json!({"content": format!("Answer {n}")})),
            json!({"id": "gen-1", "choices": [],
                "usage": {"prompt_tokens": 12, "completion_tokens": 2}}),
        ]
    })
    .await;
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    keep_config(
        &home,
        &files,
        Config {
            model: Model {
                provider: "local".into(),
                name: Some("fake".into()),
                base_url: Some(format!("http://127.0.0.1:{port}")),
                ..Model::default()
            },
            ..Config::default()
        },
    );
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(files.clone()));
    let store = Arc::new(Files::new(agents.clone()));
    let harness = Harness::new(agents, store.clone()).unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    let log = files.join("chats/telegram-42/events.jsonl");
    let length = || std::fs::metadata(&log).map_or(0, |meta| meta.len());
    let mut added = Vec::new();
    for n in 1..=60 {
        let before = length();
        let (pieces, _) = tokio::sync::mpsc::channel(64);
        let text = format!("Message {n}: {}", "words ".repeat(20));
        harness.turn(&agent, &thread, &text, pieces).await.unwrap();
        added.push(length() - before);
    }
    // The window holds every turn: a request listing it whole would add
    // ten times as much at turn 60 as at turn 6.
    let (early, late) = (added[5], added[59]);
    assert!(
        late <= 2 * early,
        "turn 6 added {early} bytes, turn 60 {late}"
    );
    let events = common::events(&files.join("chats/telegram-42"));
    let records: Vec<(u64, link_harness_wire::evidence::Evidence)> = events
        .iter()
        .filter(|e| e["role"] == link_harness::event::EVIDENCE)
        .map(|e| {
            (
                e["seq"].as_u64().unwrap(),
                serde_json::from_value(e["evidence"].clone()).unwrap(),
            )
        })
        .collect();
    let trace = index(records.iter().map(|(seq, evidence)| Located {
        seq: *seq,
        evidence,
    }))
    .unwrap();
    let requests: Vec<u64> = records
        .iter()
        .filter(|(_, e)| matches!(e.record, Record::ModelRequested { .. }))
        .map(|(seq, _)| *seq)
        .collect();
    assert_eq!(requests.len(), 60);
    let last = trace.projection(*requests.last().unwrap()).unwrap();
    let sent = asked.lock().unwrap().last().unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone();
    let said: Vec<&serde_json::Value> = sent.iter().filter(|m| m["role"] != "system").collect();
    assert_eq!(last.messages.len(), said.len());
    let text = |m: &serde_json::Value| m["content"].as_str().unwrap_or_default().to_string();
    assert!(
        last.messages
            .iter()
            .zip(said)
            .all(|(kept, sent)| kept.text.matches(text(sent).as_bytes())),
        "the last request made whole isn't what was sent"
    );
}

/// A long chat kept compact, or by a store driver, over ACP as a bridge
/// asks it, its window a few turns long: each turn, and the goal looked
/// for after its answer, reads only what was added since, never the chat
/// again, so what a turn reads stays the same however long the chat is.
/// Read whole after every answer, the turns 31 to 40 would read about
/// three times what turns 6 to 15 did.
#[tokio::test]
async fn a_turn_reads_only_what_was_added_however_kept() {
    use link_harness::server::{self, Server};
    crate::storage::with_sqlite();
    for way in ["compact", "sqlite"] {
        let root = TempDir::new(&format!("reads-kept-{way}"));
        let (port, _asked) = common::fake_model(|_, n| {
            vec![
                common::delta(json!({"content": format!("Answer {n}")})),
                json!({"id": "gen-1", "choices": [],
                    "usage": {"prompt_tokens": 12, "completion_tokens": 2}}),
            ]
        })
        .await;
        let (home, files) = (root.join("home"), root.join("files"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&files).unwrap();
        let mut spec = link_agent_spec::link();
        spec.storage = Some(link_agent_spec::Storage {
            conversations: Some(way.into()),
            ..Default::default()
        });
        keep_config(
            &home,
            &files,
            Config {
                model: Model {
                    provider: "local".into(),
                    name: Some("fake".into()),
                    base_url: Some(format!("http://127.0.0.1:{port}")),
                    ..Model::default()
                },
                endpoint_key: Some("the-key".to_string().into()),
                spec: Some(spec),
                ..Config::default()
            },
        );
        let agent = Agent::new("main").unwrap();
        let mut agents = Agents::default();
        agents.add(agent.clone(), home, common::flat(files));
        let store = Arc::new(Files::new(agents.clone()));
        let harness = Harness::new(agents, store.clone())
            .unwrap()
            .with_budget(2_000);
        let server = Arc::new(Server::new(harness, agent.clone()));
        let at = common::serve(server::router(server)).await;
        let asking = common::Asking {
            key: "the-key",
            ..Default::default()
        };
        let mut fetched = vec![store.fetched()];
        for n in 1..=40 {
            // About 400 tokens: the window holds a few turns.
            let text = format!("Message {n}: {}", "words ".repeat(250));
            common::ask_as(at, "long", &text, asking).await.unwrap();
            fetched.push(store.fetched());
        }
        let early = fetched[15] - fetched[5];
        let late = fetched[40] - fetched[30];
        assert!(
            late <= 2 * early,
            "{way}: turns 6 to 15 read {early} events, turns 31 to 40 {late}"
        );
        let thread = Thread::new("long").unwrap();
        let events = link_harness::store::Store::read(&*store, &agent, &thread).unwrap();
        assert!(events.len() > 100, "{way}: {} events", events.len());
    }
}

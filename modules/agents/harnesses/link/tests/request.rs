//! What reaches the model, whole: Link Harness's request for one message,
//! with every section of Link's spec given content of its own and a note
//! kept, each found in it by that content. A section that stops reaching
//! the model, or a note dropped on the way, fails here (K8).

use crate::common;

use std::sync::Arc;
use std::time::Duration;

use link_harness::{
    agent::{Agent, Agents, Thread},
    config::Config,
    event::Event,
    store::{Files, Store},
    turn::Harness,
};
use link_harness_contract::harness::Model;
use link_test_kit::TempDir;
use serde_json::json;

/// Each file of the agent's own, and the text only it holds: its marker
/// the first word that starts `qa-`.
const MARKED: [(&str, &str); 7] = [
    (
        "instructions/instruction.md",
        "qa-standing: answer in French.",
    ),
    ("instructions/persona.md", "qa-persona: you are Ada."),
    ("instructions/owner.md", "qa-owner: Sam, in Leeds."),
    (
        "instructions/models/local/fake/instruction.md",
        "qa-model: keep it short.",
    ),
    ("AGENTS.md", "qa-repository: run the checks."),
    (
        "skills/tides/SKILL.md",
        "---\nname: tides\ndescription: qa-skill reads the tides.\n---\nLook them up.\n",
    ),
    ("memory/notes.md", "- qa-note: the owner's cat is Miso.\n"),
];

#[tokio::test]
async fn every_section_and_note_reaches_the_model_by_content() {
    let root = TempDir::new("request-whole");
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(files.join(".git")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    for (path, text) in MARKED {
        let path = files.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let (port, asked) = common::fake_model_with(axum::Router::new(), Duration::ZERO, |_, _| {
        vec![json!({"id": "g", "choices": [{"delta": {"content": "qa-ok"}}]})]
    })
    .await;
    let agent = Agent::with_identity("main", "aaaaaaaaaaa1", "alice@box").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(&files));
    let store = Arc::new(Files::new(agents.clone()));
    let resolved = link_agent_spec::resolve(
        None,
        &[],
        &link_agent_spec::Known {
            name: "main".into(),
        },
    )
    .unwrap();
    store
        .set_config(
            &agent,
            &Config {
                model: Model {
                    provider: "local".into(),
                    name: Some("fake".into()),
                    base_url: Some(format!("http://127.0.0.1:{port}")),
                    ..Model::default()
                },
                endpoint_key: Some("test-key".to_string().into()),
                defaults: resolved.links(),
                spec: Some(resolved.spec),
                ..Config::default()
            },
        )
        .unwrap();
    let harness = Harness::new(agents, store.clone()).unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    let (pieces, mut shown) = tokio::sync::mpsc::channel::<String>(64);
    tokio::spawn(async move { while shown.recv().await.is_some() {} });
    harness
        .ask(
            &agent,
            &thread,
            Event::user("qa-message: hello"),
            pieces,
            None,
        )
        .await
        .unwrap();

    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1, "{asked:#?}");
    let request = asked[0]["messages"].to_string();
    let base = link_agent_spec::LINK_BASE
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap();
    let mut missing: Vec<&str> = MARKED
        .iter()
        .map(|(_, text)| {
            let marked = text
                .split_whitespace()
                .find(|w| w.starts_with("qa-"))
                .unwrap();
            marked.trim_end_matches(':')
        })
        .chain(["qa-message", "on Telegram"])
        .filter(|marker| !request.contains(marker))
        .collect();
    // As the request writes it: escaped as JSON, without its quotes.
    let base = serde_json::to_string(base.trim()).unwrap();
    if !request.contains(base.trim_matches('"')) {
        missing.push("Link's base");
    }
    assert!(
        missing.is_empty(),
        "not in the request: {missing:?}\n{request}"
    );
}

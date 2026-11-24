//! Goals end to end, in process: `/goal` sets one, a model judges it
//! after each answer, and the agent goes on on its own until it holds.

use crate::common;

use std::sync::Arc;

use link_harness::{
    agent::{Agent, Agents},
    config::Config,
    goal,
    server::{self, Server},
    store::Files,
    turn::Harness,
};
use link_harness_contract::harness::Model;
use link_test_kit::TempDir;
use serde_json::json;

/// The fake model's answers, in turn: the goal's first turn, the judge,
/// a turn of the agent's own, the judge again.
const SAID: [&str; 4] = [
    "Working on it",
    "GOING one is left",
    "All done",
    "HOLDS all done",
];

#[tokio::test]
async fn a_goal_is_pursued_until_it_holds() {
    let root = TempDir::new("goals");
    let (port, asked) =
        common::fake_model(|_, n| vec![common::delta(json!({"content": SAID[(n - 1).min(3)]}))])
            .await;
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(&files));
    let store = Files::new(agents.clone());
    link_harness::store::Store::set_config(
        &store,
        &agent,
        &Config {
            model: Model {
                provider: "openai".into(),
                name: Some("fake".into()),
                base_url: Some(format!("http://127.0.0.1:{port}/v1")),
                ..Model::default()
            },
            endpoint_key: Some("the-key".to_string().into()),
            ..Config::default()
        },
    )
    .unwrap();
    let server = Arc::new(Server::new(
        Harness::new(agents, Arc::new(store)).unwrap(),
        agent,
    ));
    let endpoint = common::acp(common::serve(server::router(server)).await);
    let text = |t: &str| {
        link_agent_state_protocol::acp::schema::v1::ContentBlock::Text(
            link_agent_state_protocol::acp::schema::v1::TextContent::new(t),
        )
    };
    let said = common::prompt(
        &endpoint,
        "the-key",
        "telegram:42",
        vec![text("/goal the list is done")],
    )
    .await
    .unwrap();
    assert_eq!(said, "Working on it");
    let folder = root.join("files/chats/telegram-42");
    let ended = || {
        common::events(&folder).iter().any(|e| {
            e["text"]
                .as_str()
                .is_some_and(|t| t.starts_with(goal::ENDED))
        })
    };
    link_test_kit::wait::eventually_async(
        "the goal ending",
        std::time::Duration::from_secs(10),
        ended,
    )
    .await;
    let events = common::conversation_events(&folder);
    let texts: Vec<&str> = events.iter().filter_map(|e| e["text"].as_str()).collect();
    assert!(
        texts[0].starts_with("/goal the list is done\n\n"),
        "{texts:?}"
    );
    assert_eq!(texts[1], "Working on it");
    assert!(texts[2].starts_with(goal::GOING_ON) && texts[2].ends_with("(Judged: one is left)"));
    assert_eq!(texts[3], "All done");
    assert_eq!(texts[4], format!("{} it holds: all done", goal::ENDED));
    assert_eq!(events[4]["role"], "command");
    // The judge was asked alone, with its own words.
    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 4);
    let judge = asked[1]["messages"].to_string();
    assert!(
        judge.contains("You judge whether an agent's goal holds"),
        "{judge}"
    );
    assert!(judge.contains("The goal:\\nthe list is done") && judge.contains("Working on it"));
    assert!(asked[1]["tools"].is_null() || asked[1]["tools"] == json!([]));
    // Shown done, with why.
    let shown = common::prompt(&endpoint, "the-key", "telegram:42", vec![text("/goal")])
        .await
        .unwrap();
    assert_eq!(
        shown,
        "No goal: the last ended, it holds: all done. /goal <condition> sets one."
    );
}

//! Two owners, one harness process: each with an agent named `main`,
//! of one ID (the same backup restored by both), and the same instructions, the same model and the same tool server's
//! address. Each turn answers from its own conversation, settings and
//! tool sessions; neither can reach, list or name the other's, by
//! request or by tool. Every cache is keyed by the agent's ID and owner,
//! never its name, so one that drops the owner fails here.

use crate::common;

use std::sync::{Arc, Mutex};

use axum::{Json, Router, extract::State, http::HeaderMap, response::IntoResponse, routing::post};
use link_harness::{
    agent::{Agent, Agents, Thread},
    config::Config,
    server::{self, Server},
    store::{Files, Store},
    tools,
    turn::Harness,
};
use link_harness_contract::harness::Model;
use serde_json::{Value, json};

/// An MCP server with one tool, `search`, whose answers name the session
/// they were made in; the sessions it started.
async fn fake_tool() -> (String, Arc<Mutex<Vec<String>>>) {
    let sessions = Arc::new(Mutex::new(Vec::<String>::new()));
    let app = Router::new()
        .route(
            "/_link/mcp/memory-main",
            post(
                |State(sessions): State<Arc<Mutex<Vec<String>>>>,
                 headers: HeaderMap,
                 Json(message): Json<Value>| async move {
                    let method = message["method"].as_str().unwrap_or_default().to_string();
                    let session = headers
                        .get("mcp-session-id")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    let mut sessions = sessions.lock().unwrap();
                    if method == "initialize" {
                        let id = format!("s{}", sessions.len() + 1);
                        sessions.push(id.clone());
                        let answer = json!({"jsonrpc": "2.0", "id": message["id"],
                            "result": {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                            "serverInfo": {"name": "fake", "version": "1"}}});
                        return ([("mcp-session-id", id)], Json(answer)).into_response();
                    }
                    if message.get("id").is_none() {
                        return axum::http::StatusCode::ACCEPTED.into_response();
                    }
                    let session = session.unwrap_or_default();
                    let result = match method.as_str() {
                        "tools/list" => json!({"tools": [{"name": "search",
                            "description": "Search.", "inputSchema": {"type": "object"}}]}),
                        _ => json!({"content": [{"type": "text",
                            "text": format!("session {session}")}]}),
                    };
                    Json(json!({"jsonrpc": "2.0", "id": message["id"], "result": result}))
                        .into_response()
                },
            ),
        )
        .with_state(sessions.clone());
    let port = common::serve(app).await;
    (
        format!("http://127.0.0.1:{port}/_link/mcp/memory-main"),
        sessions,
    )
}

/// A model that searches once for each new message, then answers with
/// what the tool said; it keeps what it was asked.
async fn fake_model() -> (String, common::Asked) {
    let (port, asked) = common::fake_model(|body, _| {
        let messages = body["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        vec![match last["role"].as_str() {
            Some("user") => json!({"id": "g", "choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": format!("call_{}", messages.len()), "type": "function",
                 "function": {"name": "memory__search", "arguments": "{}"}}]}}]}),
            _ => json!({"id": "g", "choices": [{"delta": {"content":
                format!("Found {}", last["content"].as_str().unwrap_or_default())}}]}),
        }]
    })
    .await;
    (format!("http://127.0.0.1:{port}"), asked)
}

/// Asks over ACP as a bridge would, naming the agent by `name` and,
/// when given, `id`, with `key`: 200 and the answer, or a refusal's
/// status and why.
async fn ask(endpoint: &str, name: &str, id: Option<&str>, key: &str, text: &str) -> (u16, String) {
    let asking = common::Asking {
        key,
        agent: Some(name),
        id,
        ..Default::default()
    };
    common::answered(common::ask_at(endpoint, "telegram:42", text, asking).await)
        .unwrap_or((0, "nothing answered".into()))
}

#[tokio::test]
async fn two_owners_mains_never_meet() {
    link_fetch::install_crypto_provider();
    let root = link_test_kit::TempDir::new("owners");
    let (model_url, asked) = fake_model().await;
    let (tool_url, sessions) = fake_tool().await;
    let alice = Agent::with_identity("main", "aaaaaaaaaaa1", "alice@box").unwrap();
    let bob = Agent::with_identity("main", "aaaaaaaaaaa1", "github-2").unwrap();
    assert_ne!(alice, bob);
    let mut agents = Agents::default();
    for (agent, key) in [(&alice, "alice-key"), (&bob, "bob-key")] {
        let (home, files) = (
            root.join(agent.owner()).join("home"),
            root.join(agent.owner()).join("files"),
        );
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&files).unwrap();
        std::fs::create_dir_all(files.join("instructions")).unwrap();
        std::fs::write(files.join("instructions/instruction.md"), "Be brief.\n").unwrap();
        agents.add(agent.clone(), home, common::flat(files));
        // The same tool, at the same address: only the agent tells them apart.
        agents.allow_tools(
            agent,
            tools::from_env([("LNK_MEMORY".to_string(), tool_url.clone())]),
        );
        Files::new(agents.clone())
            .set_config(
                agent,
                &Config {
                    model: Model {
                        provider: "local".into(),
                        name: Some("fake".into()),
                        base_url: Some(model_url.clone()),
                        ..Model::default()
                    },
                    endpoint_key: Some(key.to_string().into()),
                    spec: None,
                    ..Config::default()
                },
            )
            .unwrap();
    }
    let store = Arc::new(Files::new(agents.clone()));
    let server = Arc::new(Server::new(
        Harness::new(agents, store.clone()).unwrap(),
        alice.clone(),
    ));
    let port = common::serve(server::router(server.clone())).await;
    let endpoint = common::acp(port);

    // Each by its ID, which they share, with its own key.
    let (status, a) = ask(
        &endpoint,
        "main",
        Some(alice.id()),
        "alice-key",
        "Alice's secret",
    )
    .await;
    assert_eq!(status, 200, "{a}");
    let (status, b) = ask(&endpoint, "main", Some(bob.id()), "bob-key", "Bob's secret").await;
    assert_eq!(status, 200, "{b}");
    // A session each with the one tool server: the kept sessions, and
    // the listings, are the agent's own.
    assert_eq!(*sessions.lock().unwrap(), ["s1", "s2"]);
    assert_eq!(a, "Found session s1");
    assert_eq!(b, "Found session s2");
    let (_, a) = ask(&endpoint, "main", Some(alice.id()), "alice-key", "Again").await;
    assert_eq!(a, "Found session s1");
    assert_eq!(server.harness.tool_sessions(), 2);

    // Never by a key of neither, by a name two owners share, by an ID
    // that isn't here, or by an ID with another agent's name.
    let (status, _) = ask(&endpoint, "main", Some(bob.id()), "eve-key", "Let me in").await;
    assert_eq!(status, 401);
    let (status, _) = ask(&endpoint, "main", None, "alice-key", "Which?").await;
    assert_eq!(status, 409);
    let (status, _) = ask(&endpoint, "main", Some("ccccccccccc3"), "alice-key", "Hi").await;
    assert_eq!(status, 404);
    let (status, _) = ask(&endpoint, "work", Some(alice.id()), "alice-key", "Hi").await;
    assert_eq!(status, 404);

    // Each conversation holds its own owner's messages only, and each
    // prompt held only its own.
    let thread = Thread::new("telegram:42").unwrap();
    let alices: Vec<String> = store
        .read(&alice, &thread)
        .unwrap()
        .into_iter()
        .map(|e| e.text)
        .collect();
    let bobs: Vec<String> = store
        .read(&bob, &thread)
        .unwrap()
        .into_iter()
        .map(|e| e.text)
        .collect();
    assert!(alices.iter().any(|t| t == "Alice's secret"));
    assert!(alices.iter().all(|t| !t.contains("Bob")), "{alices:?}");
    assert!(
        bobs.iter().all(|t| !t.contains("Alice") && t != "Again"),
        "{bobs:?}"
    );
    for body in asked.lock().unwrap().iter() {
        let prompt = body["messages"].to_string();
        assert!(
            !(prompt.contains("Alice's secret") && prompt.contains("Bob's secret")),
            "{prompt}"
        );
    }
    // Their settings are their own.
    assert_eq!(
        store
            .config(&alice)
            .unwrap()
            .endpoint_key
            .map(|k| k.expose().clone())
            .as_deref(),
        Some("alice-key")
    );
    assert_eq!(
        store
            .config(&bob)
            .unwrap()
            .endpoint_key
            .map(|k| k.expose().clone())
            .as_deref(),
        Some("bob-key")
    );
}

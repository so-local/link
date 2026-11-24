//! Tools end to end, in process: a fake MCP server over HTTP, as Link's
//! tool bridge serves one at `$LNK_TOOL_<NAME>`, a fake model (OpenAI's
//! chat completions, streamed) that calls it, and the conversation that
//! keeps each call and its answer as events.

use crate::common;

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{delete, post},
};
use link_harness::{
    agent::{Agent, Agents},
    config::Config,
    server::{self, Server},
    store::{Files, Store},
    tools,
    turn::Harness,
};
use link_harness_contract::harness::Model;
use link_test_kit::TempDir;
use serde_json::{Value, json};

mod calling;
mod evidence;
mod listing;
mod order;
mod sessions;

/// Keeps `config` as `main`'s settings, as `configure` does: through its
/// store.
fn keep_config(home: &std::path::Path, files: &std::path::Path, config: Config) {
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home.into(), common::flat(files));
    Files::new(agents).set_config(&agent, &config).unwrap();
}

/// How long Link's spec keeps memory's session idle, and at most.
const IDLE: std::time::Duration = std::time::Duration::from_secs(10 * 60);
const AGE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Link's spec's rounds and calls a turn: what a harness configured by
/// a Link before specs runs too.
fn rounds() -> usize {
    link_agent_spec::link().turns.unwrap().rounds.unwrap() as usize
}

fn calls() -> usize {
    link_agent_spec::link().turns.unwrap().calls.unwrap() as usize
}

/// What the fake tool saw: each session it started, and each call.
#[derive(Default)]
struct Seen {
    sessions: Vec<String>,
    /// The sessions the harness ended.
    ended: Vec<String>,
    calls: Vec<Value>,
    /// Each `tools/list`.
    lists: usize,
    /// It fails to start: every request answers 500.
    down: bool,
    /// The next requests are refused transiently by a busy bridge.
    busy: usize,
    /// The next requests are refused by its sandbox (403), as one is
    /// while its agent's grant isn't there yet.
    refused: usize,
    /// It lists a second tool, `remember`, as one added since.
    more: bool,
    /// The method it answers only after a long while.
    slow: Option<&'static str>,
    /// Its `initialize` waits for this, when given, as a start on a busy
    /// machine waits for others' starts.
    held: Option<Arc<tokio::sync::Semaphore>>,
    /// Its sessions are gone, as when its process stopped: their
    /// requests answer 404.
    lost: bool,
}

/// An MCP server with one tool, `search`, answering over SSE when `sse`,
/// else JSON, as the tool bridge does.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn fake_tool(sse: bool) -> (String, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let app = Router::new()
        .route(
            "/_link/mcp/memory-main",
            post(
                move |State(seen): State<Arc<Mutex<Seen>>>,
                      headers: HeaderMap,
                      Json(message): Json<Value>| async move {
                    let method = message["method"].as_str().unwrap_or_default().to_string();
                    let session = headers
                        .get("mcp-session-id")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    let held = seen.lock().unwrap().held.clone();
                    if let Some(held) = held.filter(|_| method == "initialize") {
                        let _ = held.acquire().await.unwrap();
                    }
                    let slow = seen.lock().unwrap().slow == Some(method.as_str());
                    if slow {
                        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                    }
                    let mut seen = seen.lock().unwrap();
                    if seen.busy > 0 {
                        seen.busy -= 1;
                        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    if seen.refused > 0 {
                        seen.refused -= 1;
                        return (
                            axum::http::StatusCode::FORBIDDEN,
                            "Link's sandbox: memory-main isn't a tool this sandbox is granted now",
                        )
                            .into_response();
                    }
                    if seen.down {
                        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                    if method == "initialize" {
                        seen.lost = false;
                        let id = format!("s{}", seen.sessions.len() + 1);
                        seen.sessions.push(id.clone());
                        let answer = json!({"jsonrpc": "2.0", "id": message["id"],
                            "result": {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                            "serverInfo": {"name": "fake", "version": "1"},
                            "instructions": "Search before you say you don't know."}});
                        return ([("mcp-session-id", id)], Json(answer)).into_response();
                    }
                    if seen.lost && session.is_some() {
                        return axum::http::StatusCode::NOT_FOUND.into_response();
                    }
                    if session.is_none_or(|s| !seen.sessions.contains(&s)) {
                        return (axum::http::StatusCode::BAD_REQUEST, "no session").into_response();
                    }
                    if message.get("id").is_none() {
                        return axum::http::StatusCode::ACCEPTED.into_response();
                    }
                    let result = match method.as_str() {
                        "tools/list" => {
                            seen.lists += 1;
                            let mut tools = vec![json!({"name": "search",
                            "description": "Search your files and conversations.",
                            "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}},
                                "required": ["query"]},
                            "_meta": {"link.local/summary": ["query"]}})];
                            if seen.more {
                                tools.push(json!({"name": "remember",
                                    "description": "Note something for later.",
                                    "inputSchema": {"type": "object"}}));
                            }
                            json!({"tools": tools})
                        }
                        "tools/call" => {
                            seen.calls.push(message["params"].clone());
                            let query = message["params"]["arguments"]["query"]
                                .as_str()
                                .unwrap_or_default();
                            json!({"content": [{"type": "text",
                                "text": format!("chats/telegram-42: you'd drive to {query} on Friday.")}]})
                        }
                        _ => {
                            return Json(json!({"jsonrpc": "2.0", "id": message["id"],
                                "error": {"code": -32601, "message": "no such method"}}))
                            .into_response();
                        }
                    };
                    let answer = json!({"jsonrpc": "2.0", "id": message["id"], "result": result});
                    if sse {
                        let text = format!(
                            "event: message\ndata: {}\n\ndata: {answer}\n\n",
                            json!({"jsonrpc": "2.0", "method": "notifications/progress"})
                        );
                        return Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(axum::body::Body::from(text))
                            .unwrap();
                    }
                    Json(answer).into_response()
                },
            ),
        )
        .route(
            "/_link/mcp/memory-main",
            delete(
                |State(seen): State<Arc<Mutex<Seen>>>, headers: HeaderMap| async move {
                    let id = headers["mcp-session-id"].to_str().unwrap().to_string();
                    seen.lock().unwrap().ended.push(id);
                },
            ),
        )
        .with_state(seen.clone());
    let port = common::serve(app).await;
    let url = format!("http://127.0.0.1:{port}/_link/mcp/memory-main");
    (url, seen)
}

/// A model that, offered tools, calls `memory__search` for "Albuquerque"
/// (every time when `stubborn`, unless told to call none), and answers
/// with what the tool said once it has an answer, making `calls` calls
/// at once each time it calls. It keeps what it was asked.
async fn fake_model_calling(stubborn: bool, calls: usize) -> (String, common::Asked) {
    let (port, asked) = common::fake_model(move |body, _| {
        let messages = body["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        let offered = body["tools"].is_array() && body["tool_choice"] != "none";
        let call = offered && (stubborn || last["role"] == "user");
        let mut said = Vec::new();
        if call {
            said.push(
                json!({"id": "gen-1", "model": "fake", "choices": [{"delta": {
                "content": "Let me look."}}]}),
            );
            said.push(json!({"id": "gen-1", "choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": format!("call_{}", messages.len()), "type": "function",
                 "function": {"name": "memory__search", "arguments": "{\"query\":"}}]}}]}));
            said.push(json!({"id": "gen-1", "choices": [{"delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "\"Albuquerque\"}"}}]}}]}));
            for n in 1..calls {
                said.push(json!({"id": "gen-1", "choices": [{"delta": {"tool_calls": [
                    {"index": n, "id": format!("call_{}_{n}", messages.len()), "type": "function",
                     "function": {"name": "memory__search",
                        "arguments": format!("{{\"query\":\"{n}\"}}")}}]}}]}));
            }
        } else {
            let text = match last["role"].as_str() {
                Some("tool") => format!("Found it. {}", last["content"].as_str().unwrap()),
                _ => "No tools here.".into(),
            };
            said.push(
                json!({"id": "gen-2", "model": "fake", "choices": [{"delta": {
                "content": text}}]}),
            );
        }
        said.push(
            json!({"id": "gen-1", "choices": [], "usage": {"prompt_tokens": 30,
            "completion_tokens": 5}}),
        );
        said
    })
    .await;
    (format!("http://127.0.0.1:{port}"), asked)
}

struct World {
    root: TempDir,
    endpoint: String,
    asked: Arc<Mutex<Vec<Value>>>,
    server: Arc<Server>,
}

impl World {
    /// Every persisted event, including runtime evidence.
    fn raw_events(&self, folder: &str) -> Vec<Value> {
        common::events(&self.root.join("files/chats").join(folder))
    }

    /// Conversation entries in their persisted order, apart from the
    /// dedicated evidence records that do not enter model history.
    fn conversation_events(&self, folder: &str) -> Vec<Value> {
        self.raw_events(folder)
            .into_iter()
            .filter(|event| event["role"] != link_harness::event::EVIDENCE)
            .collect()
    }
}

/// Link Harness for agent `main`, with the tools of `env` (as
/// `$LNK_MEMORY` and `$LNK_TOOL_<NAME>` give them).
async fn world(name: &str, stubborn: bool, env: Vec<(String, String)>) -> World {
    world_calling(name, stubborn, 1, env).await
}

/// [`world`], its model making `calls` calls at once.
async fn world_calling(
    name: &str,
    stubborn: bool,
    calls: usize,
    env: Vec<(String, String)>,
) -> World {
    let root = TempDir::new(&format!("tools-{name}"));
    let (model_url, asked) = fake_model_calling(stubborn, calls).await;
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(root.join("files")).unwrap();
    keep_config(
        &home,
        &root.join("files"),
        Config {
            model: Model {
                provider: "local".into(),
                name: Some("fake".into()),
                base_url: Some(model_url),
                ..Model::default()
            },
            endpoint_key: Some("test-key".to_string().into()),
            spec: None,
            ..Config::default()
        },
    );
    let (endpoint, server) = serve_with(&root, env, tools::OPEN_TIMEOUT).await;
    World {
        root,
        endpoint,
        asked,
        server,
    }
}

/// A harness answering for `main`, whose folders are in `root`, with the
/// tools of `env`: its endpoint.
async fn serve(root: &std::path::Path, env: Vec<(String, String)>) -> String {
    serve_waiting(root, env, tools::OPEN_TIMEOUT).await
}

/// [`serve`], a tool having `timeout` to start and list its tools.
async fn serve_waiting(
    root: &std::path::Path,
    env: Vec<(String, String)>,
    timeout: std::time::Duration,
) -> String {
    serve_with(root, env, timeout).await.0
}

/// [`serve_waiting`], and the server behind it.
async fn serve_with(
    root: &std::path::Path,
    env: Vec<(String, String)>,
    timeout: std::time::Duration,
) -> (String, Arc<Server>) {
    serve_limited(root, env, timeout, timeout).await
}

/// [`serve_with`], a tool's start going on past a turn's wait, up to
/// `limit` in all.
async fn serve_limited(
    root: &std::path::Path,
    env: Vec<(String, String)>,
    timeout: std::time::Duration,
    limit: std::time::Duration,
) -> (String, Arc<Server>) {
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        root.join("home"),
        common::flat(root.join("files")),
    );
    agents.allow_tools(&agent, tools::from_env(env));
    let store = Arc::new(Files::new(agents.clone()));
    let server = Arc::new(Server::new(
        Harness::new(agents, store)
            .unwrap()
            .with_open_timeout(timeout)
            .with_start_limit(limit),
        agent,
    ));
    let port = common::serve(server::router(server.clone())).await;
    let endpoint = common::acp(port);
    (endpoint, server)
}

async fn ask(endpoint: &str, thread: &str, text: &str) -> String {
    let bridge = link_connection_contract::Bridge {
        agent: "main".into(),
        endpoint: endpoint.into(),
        key: "test-key".to_string().into(),
        settings: Value::Null.into(),
        ..Default::default()
    };
    let (progress, _) = tokio::sync::watch::channel(String::new());
    link_connection_contract::ask(&bridge, thread, text, &progress)
        .await
        .unwrap()
}

/// [`ask`], narrowing `thread` to `tools` (`Said::tools`) when some are
/// given.
async fn ask_with(
    endpoint: &str,
    thread: &str,
    from: &str,
    text: &str,
    tools: Option<&[String]>,
) -> String {
    let bridge = link_connection_contract::Bridge {
        agent: "main".into(),
        endpoint: endpoint.into(),
        key: "test-key".to_string().into(),
        ..Default::default()
    };
    let (progress, _) = tokio::sync::watch::channel(String::new());
    let from = Some(link_connection_contract::Speaker {
        from,
        name: None,
        hops: 1,
    });
    match link_connection_contract::ask_with(&bridge, thread, text, from, tools, &progress)
        .await
        .unwrap()
    {
        link_connection_contract::Asked::Answer(answer) => answer,
        link_connection_contract::Asked::Steered => panic!("steered"),
    }
}

fn memory(url: &str) -> Vec<(String, String)> {
    vec![
        ("LNK_MEMORY".into(), url.into()),
        ("LNK_TOOL_MEMORY_MAIN".into(), url.into()),
    ]
}

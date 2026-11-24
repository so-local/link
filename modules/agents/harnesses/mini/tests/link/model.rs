use super::*;
use crate::{
    core::{self, Answer, Call},
    link::tools::Tools,
    runtime::conversation::*,
};
use axum::{
    Json, Router,
    extract::{OriginalUri, State},
    http::header,
    response::Response,
    routing::post,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Mock {
    kind: &'static str,
    replies: Vec<Message>,
    requests: Mutex<Vec<(String, Value)>>,
    lists: AtomicUsize,
}
struct Server {
    url: String,
    mock: Arc<Mock>,
    running: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.running.abort();
    }
}
impl Server {
    fn start(kind: &'static str, replies: Vec<Message>) -> Self {
        link_fetch::install_crypto_provider();
        let held = link_test_kit::net::Held::loopback();
        let url = format!("http://{}", held.address());
        let listener = held.listener();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let mock = Arc::new(Mock {
            kind,
            replies,
            requests: Mutex::default(),
            lists: AtomicUsize::new(0),
        });
        let app = Router::new()
            .fallback(post(answer))
            .with_state(mock.clone());
        let running = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { url, mock, running }
    }
}
fn sse(events: Vec<Value>) -> String {
    events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect()
}
async fn answer(
    State(mock): State<Arc<Mock>>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Response {
    let index = {
        let mut requests = mock.requests.lock().unwrap();
        let n = requests.len();
        requests.push((uri.path().into(), body.clone()));
        n
    };
    let (content_type, body) = if mock.kind == "mcp" {
        let result = match body["method"].as_str().unwrap() {
            "tools/list" => {
                let mut names = vec!["z", "a"];
                if mock.lists.fetch_add(1, Ordering::Relaxed) % 2 == 1 {
                    names.reverse();
                }
                json!({"tools": names.into_iter().map(|name| json!({"name":name,"description":"a tool","inputSchema":{"type":"object"}})).collect::<Vec<_>>()})
            }
            "tools/call" => json!({"isError":true,"content":[{"type":"text","text":"refused"}]}),
            _ => {
                json!({"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"fixture","version":"1"}})
            }
        };
        (
            "application/json",
            json!({"jsonrpc":"2.0","id":body["id"],"result":result}).to_string(),
        )
    } else {
        let message = &mock.replies[index];
        match mock.kind {
            "canonical" => {
                let answer = Answered {
                    text: message.text.clone(),
                    calls: message.calls.clone(),
                    thinking: message.thinking.clone(),
                    ..Answered::default()
                };
                (
                    "application/x-ndjson",
                    format!(
                        "{}\n",
                        serde_json::to_string(&link_model_contract::chat::Stream::Answer {
                            answer
                        })
                        .unwrap()
                    ),
                )
            }
            "anthropic" => {
                let mut events = vec![
                    json!({"type":"message_start","message":{"id":"answer","model":"fixture","usage":{"input_tokens":10,"output_tokens":0}}}),
                ];
                for (index, thinking) in message.thinking.iter().enumerate() {
                    events.push(json!({"type":"content_block_start","index":index,"content_block":thinking}));
                }
                events.push(json!({"type":"content_block_delta","index":message.thinking.len(),"delta":{"type":"text_delta","text":message.text}}));
                for (index, call) in message.calls.iter().enumerate() {
                    let index = message.thinking.len() + 1 + index;
                    events.push(json!({"type":"content_block_start","index":index,"content_block":{"type":"tool_use","id":call.id,"name":call.name,"input":{}}}));
                    events.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":call.arguments.to_string()}}));
                }
                events.push(json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}));
                events.push(json!({"type":"message_stop"}));
                ("text/event-stream", sse(events))
            }
            _ => {
                let calls: Vec<_> = message.calls.iter().enumerate().map(|(index, call)| json!({"index":index,"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments.to_string()}})).collect();
                (
                    "text/event-stream",
                    sse(vec![
                        json!({"id":"answer","model":"fixture","choices":[{"delta":{"content":message.text,"tool_calls":calls}}]}),
                        json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":2}}),
                    ]),
                )
            }
        }
    };
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(body))
        .unwrap()
}
fn text(role: &str, text: &str) -> Message {
    Message {
        role: role.into(),
        text: text.into(),
        ..Message::default()
    }
}
fn without_cache_markers(mut value: Value) -> Value {
    fn remove(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                fields.remove("cache_control");
                for child in fields.values_mut() {
                    remove(child);
                }
            }
            Value::Array(items) => {
                for child in items {
                    remove(child);
                }
            }
            _ => (),
        }
    }
    remove(&mut value);
    value
}
async fn native_round_trip(provider: &'static str) {
    let thinking = if provider == "anthropic" {
        vec![json!({"type":"thinking","thinking":"Read it.","signature":"opaque-signature"})]
    } else {
        Vec::new()
    };
    let server = Server::start(
        provider,
        vec![
            Message {
                role: "assistant".into(),
                text: "Reading.".into(),
                calls: vec![Call {
                    id: "read-1".into(),
                    name: "files__read".into(),
                    arguments: json!({"path":"note.txt"}),
                }],
                thinking: thinking.clone(),
                ..Message::default()
            },
            text("assistant", "hello"),
            text("assistant", "Still hello."),
        ],
    );
    let model = Model {
        provider: provider.into(),
        name: Some("fixture".into()),
        base_url: Some(if provider == "anthropic" {
            server.url.clone()
        } else {
            format!("{}/v1", server.url)
        }),
        ..Model::default()
    };
    let client = reqwest::Client::new();
    let tools = vec![Tool {
        name: "files__read".into(),
        description: "read a note".into(),
        schema: json!({"type":"object"}),
    }];
    let (pieces, _received) = mpsc::channel(16);
    let request = Request {
        client: &client,
        model: &model,
        known: None,
        window: None,
        instructions: "",
        tools: &tools,
        pieces: &pieces,
        thread: "example",
    };
    let dir = link_test_kit::TempDir::new("mini-model-history");
    let path = conversation_path(&dir, "example").unwrap();
    append_message(&path, &text("user", "Read note.txt.")).unwrap();
    core::run_turn(
        load_conversation(&path).unwrap(),
        |messages| {
            let request = &request;
            async move { request.ask_model(&messages).await }
        },
        |call| async move {
            assert_eq!(call.arguments, json!({"path":"note.txt"}));
            Answer {
                call: call.id,
                text: "hello".into(),
                failed: false,
                parts: Vec::new(),
            }
        },
        |message| append_message(&path, message),
    )
    .await
    .unwrap();
    let prefix = std::fs::read(&path).unwrap();
    let reloaded = load_conversation(&path).unwrap();
    assert_eq!(reloaded.len(), 4);
    assert_eq!(reloaded[1].thinking, thinking);
    check_complete(&reloaded).unwrap();
    append_message(&path, &text("user", "What did the note say?")).unwrap();
    core::run_turn(
        load_conversation(&path).unwrap(),
        |messages| {
            let request = &request;
            async move { request.ask_model(&messages).await }
        },
        |_| async { panic!("the earlier tool must not be replayed") },
        |message| append_message(&path, message),
    )
    .await
    .unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(&prefix));
    let asked = server.mock.requests.lock().unwrap();
    assert_eq!(asked.len(), 3);
    let tool_reply = asked[1].1["messages"].as_array().unwrap().last().unwrap();
    if provider == "anthropic" {
        assert_eq!(tool_reply["content"][0]["tool_use_id"], "read-1");
        assert_eq!(tool_reply["content"][0]["content"][0]["text"], "hello");
    } else {
        assert_eq!(tool_reply["tool_call_id"], "read-1");
        assert_eq!(tool_reply["content"], "hello");
    }
    for (path, body) in asked.iter() {
        assert_eq!(
            path,
            if provider == "anthropic" {
                "/v1/messages"
            } else {
                "/v1/chat/completions"
            }
        );
        assert!(
            body.get("system").is_none(),
            "empty instructions were serialized"
        );
        assert_eq!(body["tools"], asked[0].1["tools"]);
        if provider == "anthropic" {
            let last = body["messages"].as_array().unwrap().last().unwrap();
            assert_eq!(
                last["content"].as_array().unwrap().last().unwrap()["cache_control"]["type"],
                "ephemeral"
            );
        } else {
            assert!(
                body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|m| m["role"] != "system")
            );
        }
    }
    let earlier = without_cache_markers(asked[1].1["messages"].clone());
    let later = without_cache_markers(asked[2].1["messages"].clone());
    let earlier = earlier.as_array().unwrap();
    assert_eq!(earlier, &later.as_array().unwrap()[..earlier.len()]);
}

#[tokio::test]
async fn anthropic_keeps_tool_history_and_cache_boundaries_after_reload() {
    native_round_trip("anthropic").await;
}
#[tokio::test]
async fn openai_keeps_the_same_prompt_prefix_after_reload() {
    native_round_trip("openai").await;
}

#[tokio::test]
async fn canonical_model_actions_own_provider_mapping() {
    let server = Server::start("canonical", vec![text("assistant", "hello")]);
    let model = Model {
        provider: "not-a-native-provider".into(),
        name: Some("fixture".into()),
        actions_url: Some(format!("{}/model", server.url)),
        ..Model::default()
    };
    let client = reqwest::Client::new();
    let (pieces, _received) = mpsc::channel(16);
    let request = Request {
        client: &client,
        model: &model,
        known: None,
        window: None,
        instructions: "Answer briefly.",
        tools: &[],
        pieces: &pieces,
        thread: "example",
    };
    let answer = request.ask_model(&[text("user", "Hi")]).await.unwrap();
    assert_eq!(answer.text, "hello");
    let asked = server.mock.requests.lock().unwrap();
    assert_eq!(asked[0].0, "/model/chat");
    assert_eq!(asked[0].1["system"], "Answer briefly.");
    let answer_limit = asked[0].1["answer"].as_u64().unwrap();
    assert!(answer_limit > 0 && answer_limit <= asked[0].1["window"].as_u64().unwrap());
    assert!(asked[0].1.get("provider").is_none());
    assert!(asked[0].1.get("cache_control").is_none());
}

#[tokio::test]
async fn tool_discovery_order_does_not_change_the_model_prompt() {
    let server = Server::start("mcp", Vec::new());
    let client = reqwest::Client::new();
    let first = Tools::open(
        &client,
        vec![
            ("b".into(), server.url.clone()),
            ("a".into(), server.url.clone()),
        ],
    )
    .await;
    let second = Tools::open(
        &client,
        vec![
            ("a".into(), server.url.clone()),
            ("b".into(), server.url.clone()),
        ],
    )
    .await;
    assert_eq!(first.offered(), second.offered());
    assert_eq!(first.names(), ["a__a", "a__z", "b__a", "b__z"]);
    let result = first
        .call_tool(
            &client,
            &Call {
                id: "call".into(),
                name: "a__a".into(),
                arguments: json!({}),
            },
            &json!({}),
        )
        .await;
    assert!(result.failed);
    assert_eq!(result.text, "refused");
    assert_eq!(result.call, "call");
    first.close(&client).await;
    second.close(&client).await;
}

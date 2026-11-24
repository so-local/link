//! The order of one step's calls: those that only read run beside each
//! other, one that changes something runs alone, after the calls before
//! it and before those after it, and the answers keep the asked order.

use super::*;

use std::time::Duration;

/// How long the reads asked side by side wait for each other: they meet
/// at once when they run together.
const MEET: Duration = Duration::from_secs(10);

/// How long a change watches for another call running beside it. None
/// does when the step is in order, so this is how long a change takes.
const WATCH: Duration = Duration::from_millis(300);

/// What the store saw: each call's start and end, in order, and how many
/// run now.
#[derive(Default)]
struct Log {
    said: Vec<String>,
    running: usize,
}

impl Log {
    fn at(&self, line: &str) -> usize {
        self.said
            .iter()
            .position(|said| said == line)
            .unwrap_or_else(|| panic!("{line} not in {:?}", self.said))
    }
}

/// An MCP server, `kv`, whose `get` only reads (its `readOnlyHint`) and
/// whose `set` says nothing of itself, so it's taken as a change. A `get`
/// naming another in `with` waits for that one to start; a `set` watches
/// for another call running beside it. Each answers what it saw.
async fn store() -> (String, Arc<Mutex<Log>>) {
    let log = Arc::new(Mutex::new(Log::default()));
    let app = Router::new()
        .route(
            "/mcp",
            post(
                move |State(log): State<Arc<Mutex<Log>>>, Json(message): Json<Value>| async move {
                    let id = message["id"].clone();
                    let result = match message["method"].as_str().unwrap_or_default() {
                        "initialize" => {
                            let answer = json!({"jsonrpc": "2.0", "id": id,
                                "result": {"protocolVersion": "2025-06-18",
                                "capabilities": {"tools": {}},
                                "serverInfo": {"name": "kv", "version": "1"}}});
                            return ([("mcp-session-id", "s1")], Json(answer)).into_response();
                        }
                        "tools/list" => json!({"tools": [
                            {"name": "get", "description": "Reads a value.",
                             "inputSchema": {"type": "object"},
                             "annotations": {"readOnlyHint": true}},
                            {"name": "set", "description": "Writes a value.",
                             "inputSchema": {"type": "object"}}]}),
                        "tools/call" => called(&log, &message["params"]).await,
                        _ => return axum::http::StatusCode::ACCEPTED.into_response(),
                    };
                    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
                },
            ),
        )
        .with_state(log.clone());
    let port = common::serve(app).await;
    (format!("http://127.0.0.1:{port}/mcp"), log)
}

/// One call of the store: logged as it starts and ends, and what it saw
/// of the calls beside it.
async fn called(log: &Mutex<Log>, params: &Value) -> Value {
    let key = params["arguments"]["key"].as_str().unwrap().to_string();
    {
        let mut log = log.lock().unwrap();
        log.said.push(format!("start {key}"));
        log.running += 1;
    }
    let saw = match params["name"].as_str() {
        Some("get") => match params["arguments"]["with"].as_str() {
            Some(other) => {
                let other = format!("start {other}");
                let met = link_test_kit::wait::within_async("the other read", MEET, || {
                    log.lock().unwrap().said.contains(&other)
                })
                .await;
                if met.is_ok() { "together" } else { "alone" }
            }
            None => "read",
        },
        _ => {
            let beside = link_test_kit::wait::within_async("a call beside it", WATCH, || {
                log.lock().unwrap().running > 1
            })
            .await;
            if beside.is_ok() { "beside" } else { "alone" }
        }
    };
    {
        let mut log = log.lock().unwrap();
        log.said.push(format!("end {key}"));
        log.running -= 1;
    }
    json!({"content": [{"type": "text", "text": format!("{key} {saw}")}]})
}

/// A model that, asked by its owner, makes one step of `calls` (each a
/// tool and its arguments), and otherwise answers "Done."
async fn model(calls: Vec<(&'static str, Value)>) -> (String, common::Asked) {
    let (port, asked) = common::fake_model(move |body, _| {
        let last = body["messages"].as_array().unwrap().last().unwrap().clone();
        if last["role"] != "user" {
            return vec![common::delta(json!({"content": "Done."}))];
        }
        let calls: Vec<Value> = calls
            .iter()
            .enumerate()
            .map(|(index, (name, arguments))| {
                json!({"index": index, "id": arguments["key"], "type": "function",
                    "function": {"name": name, "arguments": arguments.to_string()}})
            })
            .collect();
        vec![common::delta(json!({"tool_calls": calls}))]
    })
    .await;
    (format!("http://127.0.0.1:{port}"), asked)
}

/// Link Harness for `main`, with the store as its tool `kv`, and a model
/// making one step of `calls`: the request that gave the model their
/// answers once a message is answered, and what the store saw.
async fn step(name: &str, calls: Vec<(&'static str, Value)>) -> (Value, Arc<Mutex<Log>>) {
    let root = TempDir::new(&format!("order-{name}"));
    let (url, log) = store().await;
    let (model_url, asked) = model(calls).await;
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
            ..Config::default()
        },
    );
    let endpoint = serve(&root, vec![("LNK_TOOL_KV".into(), url)]).await;
    assert_eq!(ask(&endpoint, "telegram:42", "Go.").await, "Done.");
    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2);
    (asked[1].clone(), log)
}

/// The tool answers the model was given in `request`, in their order:
/// each call's id and its text.
fn answers(request: &Value) -> Vec<(String, String)> {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| {
            let text = message["content"].as_str().unwrap_or_default().to_string();
            (message["tool_call_id"].as_str().unwrap().to_string(), text)
        })
        .collect()
}

/// A model that deletes and then lists in one step sees the list after
/// the delete: a change runs after the reads before it have ended, alone,
/// and the read after it starts once it has ended. Two reads side by side
/// still run at once, and every answer keeps the order asked.
#[tokio::test]
async fn a_change_in_a_step_runs_between_the_calls_around_it() {
    let (request, log) = step(
        "change",
        vec![
            ("kv__get", json!({"key": "a", "with": "b"})),
            ("kv__get", json!({"key": "b", "with": "a"})),
            ("kv__set", json!({"key": "c"})),
            ("kv__get", json!({"key": "d"})),
        ],
    )
    .await;
    let log = log.lock().unwrap();
    assert!(log.at("start c") > log.at("end a"), "{:?}", log.said);
    assert!(log.at("start c") > log.at("end b"), "{:?}", log.said);
    assert!(log.at("start d") > log.at("end c"), "{:?}", log.said);
    let answers = answers(&request);
    let said: Vec<(&str, &str)> = answers
        .iter()
        .map(|(id, text)| (id.as_str(), text.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("a", "a together"),
            ("b", "b together"),
            ("c", "c alone"),
            ("d", "d read")
        ]
    );
}

/// Two changes in a row each run alone, in the order asked.
#[tokio::test]
async fn changes_in_a_step_run_one_after_another() {
    let (request, log) = step(
        "changes",
        vec![
            ("kv__set", json!({"key": "a"})),
            ("kv__set", json!({"key": "b"})),
        ],
    )
    .await;
    let log = log.lock().unwrap();
    assert_eq!(log.said, ["start a", "end a", "start b", "end b"]);
    let answers = answers(&request);
    assert_eq!(
        answers,
        [
            ("a".to_string(), "a alone".to_string()),
            ("b".to_string(), "b alone".to_string())
        ]
    );
}

use super::*;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use link_files::reading::{ByteSpan, ReadReceipt, ReadSnapshot, TextTransform};
use link_harness::{
    agent::{Agents, Thread},
    config::Config,
    event::Event,
    store::{Files, Store},
    tools::Server,
    turn::Harness,
};
use link_harness_contract::harness::Model;
use link_harness_wire::evidence::{ManagedReadSource, ReadProvider, ReadRegistration};
use std::sync::{Arc, Mutex};

pub(super) const FACT: &str = "zinnia-47";
const BODY: &str = "---\nname: example\ndescription: Read the controlled fact.\n---\n\n# Instructions\n\nThe checksum word is zinnia-47.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    File,
    Memory,
    Repository,
    Partial,
    Clipped,
    Tampered,
    Unregistered,
}

impl Mode {
    pub fn path(self) -> &'static str {
        if self == Self::Repository {
            ".agents/skills/example/SKILL.md"
        } else {
            "skills/example/SKILL.md"
        }
    }

    pub fn complete(self) -> bool {
        matches!(self, Self::File | Self::Memory | Self::Repository)
    }

    fn source(self) -> String {
        if self == Self::Clipped {
            BODY.replace(
                "The checksum word",
                &format!("{}\nThe checksum word", "x".repeat(4096)),
            )
        } else {
            BODY.into()
        }
    }
}

struct Mcp {
    url: String,
    calls: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Mcp {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Mcp {
    async fn new(result: Value) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/mcp", post(reply))
            .with_state((result, calls.clone()));
        let held = link_test_kit::Held::loopback();
        let port = held.port();
        let listener = held.listener();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            url: format!("http://127.0.0.1:{port}/mcp"),
            calls,
            task,
        }
    }
}

async fn reply(
    State((result, calls)): State<(Value, Arc<Mutex<Vec<Value>>>)>,
    Json(message): Json<Value>,
) -> axum::response::Response {
    let result = match message["method"].as_str().unwrap_or_default() {
        "initialize" => {
            json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"controlled-reader","version":"1"}})
        }
        "tools/list" => {
            json!({"tools":[{"name":"read","description":"Read the requested skill.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}]})
        }
        "tools/call" => {
            calls.lock().unwrap().push(message["params"].clone());
            result
        }
        _ => return axum::http::StatusCode::ACCEPTED.into_response(),
    };
    (
        [("mcp-session-id", "controlled-reader")],
        Json(json!({"jsonrpc":"2.0","id":message["id"],"result":result})),
    )
        .into_response()
}

fn result(mode: Mode, source: &str) -> Value {
    let selected = if mode == Mode::Partial {
        &source[..source.find(FACT).unwrap()]
    } else {
        source
    };
    let text = if mode == Mode::Memory {
        selected.lines().collect::<Vec<_>>().join("\n")
    } else {
        selected.into()
    };
    let read = ReadReceipt {
        path: mode.path().into(),
        source: ReadSnapshot::Whole {
            content: ContentDigest::of(source.as_bytes()),
        },
        span: ByteSpan {
            start: 0,
            end: selected.len() as u64,
        },
        text: ContentDigest::of(text.as_bytes()),
        text_prefix_bytes: 0,
        transformations: if mode == Mode::Memory {
            vec![TextTransform::Lines]
        } else {
            vec![]
        },
    };
    let returned = if mode == Mode::Tampered {
        "an altered response".to_string()
    } else {
        text
    };
    let mut document = json!({"path":mode.path(),"text":returned,"read":read});
    let displayed = if mode == Mode::Memory {
        document["kind"] = "file".into();
        document["title"] = "Example".into();
        document["lines"] = source.lines().count().into();
        let mut shown = document.clone();
        shown.as_object_mut().unwrap().remove("read");
        shown.to_string()
    } else {
        if mode == Mode::Unregistered {
            document["claimedAuthority"] = "managed".into();
        }
        returned
    };
    // As file and memory give it: the text is the model-facing content's
    // alone, never in the structured content beside its receipt.
    if let Some(document) = document.as_object_mut() {
        document.remove("text");
        document.remove("title");
    }
    json!({"content":[{"type":"text","text":displayed}],"structuredContent":document})
}

pub(super) struct Run {
    pub state: link_harness_wire::evidence::export::SessionEvidence,
    pub asked: Vec<Value>,
    pub calls: Vec<Value>,
    pub source: String,
}

pub(super) async fn run(mode: Mode) -> Run {
    let root = link_test_kit::TempDir::new("skill-model-inclusion");
    let files = root.join("files");
    std::fs::create_dir_all(files.join(".git")).unwrap();
    let path = files.join(mode.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, mode.source()).unwrap();
    let source = std::fs::read_to_string(path).unwrap();
    let tool = Mcp::new(result(mode, &source)).await;
    let name = if mode == Mode::Memory {
        "memory"
    } else {
        "file"
    };
    let called = format!("{name}__read");
    let (port, asked) = common::fake_model(move |_, n| vec![common::delta(if n == 1 {
        json!({"tool_calls":[{"index":0,"id":"read_fixture","type":"function","function":{"name":called,"arguments":json!({"path":mode.path()}).to_string()}}]})
    } else { json!({"content":"Read result observed."}) })]).await;
    let agent = Agent::with_identity("main", "abcdef123456", "owner").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), root.join("home"), common::flat(&files));
    agents.allow_tools(
        &agent,
        vec![Server {
            name: name.into(),
            url: tool.url.clone(),
            kept: true,
        }],
    );
    let known = link_agent_spec::Known {
        name: "main".into(),
    };
    let own = link_agent_spec::parse("schema = 1\n[tools]\nanswer = '1 KB'\n").unwrap();
    let spec = link_agent_spec::resolve(Some(&own), &[], &known).unwrap();
    let store = Files::new(agents.clone());
    store
        .set_config(
            &agent,
            &Config {
                model: Model {
                    provider: "openai".into(),
                    name: Some("fake".into()),
                    base_url: Some(format!("http://127.0.0.1:{port}/v1")),
                    ..Model::default()
                },
                spec: Some(spec.spec),
                read_sources: if mode == Mode::Unregistered {
                    vec![]
                } else {
                    vec![ReadRegistration {
                        server: name.into(),
                        operation: "read".into(),
                        endpoint: tool.url.clone(),
                        source: ManagedReadSource {
                            provider: if mode == Mode::Memory {
                                ReadProvider::Memory
                            } else {
                                ReadProvider::File
                            },
                            owner: agent.owner().into(),
                            agent: agent.id().into(),
                            definition: format!("controlled-{name}"),
                            argv_sha256: ContentDigest::of(b"controlled native reader").sha256,
                        },
                    }]
                },
                ..Config::default()
            },
        )
        .unwrap();
    let harness = Harness::new(agents, Arc::new(store)).unwrap();
    let (pieces, _received) = tokio::sync::mpsc::channel(8);
    harness
        .ask(
            &agent,
            &Thread::new("link:main").unwrap(),
            Event::user("Read the example skill normally."),
            pieces,
            None,
        )
        .await
        .unwrap();
    let exported = harness
        .exported_state(&agent, &[link_agent_state_protocol::Kind::Sessions])
        .unwrap();
    let mut sessions = exported.sessions.unwrap();
    assert_eq!(sessions.len(), 1);
    let session = sessions.pop().unwrap();
    let state = session.evidence.unwrap();
    state.validate(session.updates.len()).unwrap();
    let asked = asked.lock().unwrap().clone();
    let calls = tool.calls.lock().unwrap().clone();
    Run {
        state,
        asked,
        calls,
        source,
    }
}

//! What the tests share: programs of the folders beside this one
//! (`lnk-sandbox` from the core, `LNK_CORE` for another place, and
//! `lnk-memory-tool`, `lnk-run-tool` and `lnk-file-tool` from the
//! workspace's `tools/`, `LINK_TOOLS`), each built there once, as Link
//! harness runs them and links none; a fake model; and a conversation's
//! events.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{Json, Router, extract::State, routing::post};
use futures_util::StreamExt;
use serde_json::{Value, json};

/// `program` in the folder beside this one named `folder`, or at
/// `$env`, built there with all its programs.
fn beside(env: &str, folder: &str, program: &str) -> PathBuf {
    let root = std::env::var_os(env).map(PathBuf::from).unwrap_or_else(|| {
        // modules/agents/harnesses/link is three folders below the workspace root.
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../..")
            .join(folder)
    });
    link_test_kit::programs_in(&root).join(program)
}

/// `lnk-sandbox`, from `link-core`.
#[cfg(target_os = "linux")]
pub fn sandbox() -> PathBuf {
    beside("LNK_CORE", "core", "lnk-sandbox")
}

/// `lnk-sqlite-store`, the SQLite store driver, from `link-core`.
pub fn sqlite() -> PathBuf {
    beside("LNK_CORE", "core", "lnk-sqlite-store")
}

/// `lnk-file-store`, the `file` place's driver, from `link-core`.
pub fn file_driver() -> PathBuf {
    beside("LNK_CORE", "core", "lnk-file-store")
}

/// `lnk-memory-tool`, from `link-tools`.
#[cfg(target_os = "linux")]
pub fn memory() -> PathBuf {
    beside("LINK_TOOLS", "modules/agents/tools", "lnk-memory-tool")
}

/// `lnk-run-tool`, from `link-tools`.
#[cfg(target_os = "linux")]
pub fn run() -> PathBuf {
    beside("LINK_TOOLS", "modules/agents/tools", "lnk-run-tool")
}

/// `lnk-file-tool`, from `link-tools`.
#[cfg(target_os = "linux")]
pub fn file() -> PathBuf {
    beside("LINK_TOOLS", "modules/agents/tools", "lnk-file-tool")
}

/// The events of the conversation in `folder`, in order: its log
/// (`events.jsonl`), a line each.
pub fn events(folder: &Path) -> Vec<serde_json::Value> {
    let log = std::fs::read_to_string(folder.join("events.jsonl")).unwrap_or_default();
    log.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The logical conversation, excluding only internal evidence records.
pub fn conversation_events(folder: &Path) -> Vec<serde_json::Value> {
    events(folder)
        .into_iter()
        .filter(|event| event["role"] != link_harness::event::EVIDENCE)
        .collect()
}

/// Takes the window's mark out of every event of the log in `folder`,
/// as a conversation kept before the mark was: the next turn walks the
/// window from the first event.
pub fn forget_marks(folder: &Path) {
    let mut log = String::new();
    for mut event in events(folder) {
        event.as_object_mut().unwrap().remove("window");
        log += &format!("{event}\n");
    }
    std::fs::write(folder.join("events.jsonl"), log).unwrap();
}

/// Link's spec with `[window] compact_at = "never"`, and the keys of it
/// that are Link's (all but that one): what leaves the window leaves
/// unsummarized, as a test of dropping alone wants.
pub fn never_compacting() -> (link_agent_spec::Spec, Vec<String>) {
    use link_agent_spec::{CompactAt, Known, NEVER, resolve};
    let resolved = resolve(
        None,
        &[],
        &Known {
            name: "main".into(),
        },
    )
    .unwrap();
    let mut spec = resolved.spec.clone();
    spec.window.get_or_insert_default().compact_at = Some(CompactAt::Word(NEVER.into()));
    let defaults = resolved
        .links()
        .into_iter()
        .filter(|key| key != "window.compact_at")
        .collect();
    (spec, defaults)
}

/// What a fake model was asked: each request's body, in order.
pub type Asked = Arc<Mutex<Vec<Value>>>;

/// Serves `app` on a random port of 127.0.0.1: the port.
pub async fn serve(app: Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

/// A model speaking OpenAI's chat completions, streamed, on a port of its
/// own: it keeps each request, and answers with the events `answer` makes
/// of it and of how many it has been asked (this one too), then `[DONE]`.
pub async fn fake_model<F>(answer: F) -> (u16, Asked)
where
    F: Fn(&Value, usize) -> Vec<Value> + Clone + Send + Sync + 'static,
{
    fake_model_with(Router::new(), Duration::ZERO, answer).await
}

/// How a stand-in holds what it's asked, such as a fake model each answer
/// after its first piece.
#[derive(Clone)]
pub enum Hold {
    /// For a while.
    For(Duration),
    /// Until the test lets it go: the `n`th held once `n` are let go
    /// ([`LetGo`]). A test that orders a step on it waits for what it
    /// let go, never for a pause.
    Until(tokio::sync::watch::Receiver<usize>),
}

impl Hold {
    /// Waits as it says for what's held `n`th: a pause, or until `n`
    /// are let go (or its test is gone).
    #[expect(clippy::disallowed_methods, reason = "a stand-in taking its time")]
    pub async fn wait(&self, n: usize) {
        match self {
            Self::For(pause) => tokio::time::sleep(*pause).await,
            Self::Until(let_go) => {
                let _ = let_go.clone().wait_for(|&up_to| up_to >= n).await;
            }
        }
    }
}

impl From<Duration> for Hold {
    fn from(pause: Duration) -> Self {
        Self::For(pause)
    }
}

/// What lets a fake model's [`Hold::Until`] answers go.
pub struct LetGo(tokio::sync::watch::Sender<usize>);

impl LetGo {
    /// None let go yet, and the hold that waits for it.
    pub fn new() -> (Self, Hold) {
        let (sender, receiver) = tokio::sync::watch::channel(0);
        (Self(sender), Hold::Until(receiver))
    }

    /// The first `n` answers let go, and no more: those asked after
    /// are held.
    pub fn up_to(&self, n: usize) {
        self.0.send_replace(n);
    }

    /// Every answer let go, those asked later too.
    pub fn all(&self) {
        self.up_to(usize::MAX);
    }
}

/// [`fake_model`], with `routes` beside it, and the events after the
/// first sent as `hold` says: a pause later, or once let go.
pub async fn fake_model_with<F>(routes: Router, hold: impl Into<Hold>, answer: F) -> (u16, Asked)
where
    F: Fn(&Value, usize) -> Vec<Value> + Clone + Send + Sync + 'static,
{
    let asked = Asked::default();
    let hold = hold.into();
    let chat = move |State(asked): State<Asked>, Json(body): Json<Value>| async move {
        let n = {
            let mut asked = asked.lock().unwrap();
            asked.push(body.clone());
            asked.len()
        };
        let mut events = answer(&body, n)
            .into_iter()
            .map(|e| format!("data: {e}\n\n"));
        let first = events.next().unwrap_or_default();
        let rest: String = events.chain(["data: [DONE]\n\n".to_string()]).collect();
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(first)]).chain(
            futures_util::stream::once(async move {
                hold.wait(n).await;
                Ok(rest)
            }),
        );
        axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from_stream(stream))
            .unwrap()
    };
    let app = Router::new()
        .route("/v1/chat/completions", post(chat))
        .with_state(asked.clone())
        .merge(routes);
    (serve(app).await, asked)
}

/// A streamed event of one `delta`, such as `{"content": "Hi"}`.
pub fn delta(delta: Value) -> Value {
    json!({"id": "gen-1", "model": "fake", "choices": [{"delta": delta}]})
}

/// An agent's folder as a Link before Home and its own folder were two
/// laid it out: everything in `files`.
pub fn flat(files: impl Into<PathBuf>) -> link_harness::agent::Folder {
    let files = files.into();
    link_harness::agent::Folder {
        home: files.clone(),
        agent: files,
    }
}

/// The first `turns` lines of a harness's measures at `path`, once
/// they're written whole: each turn's is written after its answer, off
/// its way.
pub async fn measures(path: &Path, turns: usize) -> Vec<Value> {
    link_test_kit::wait::eventually_async(
        &format!("{turns} turns' measures in {}", path.display()),
        Duration::from_secs(5),
        || {
            let lines = link_measure_contract::client::written(path).unwrap();
            (lines.len() >= turns).then_some(lines)
        },
    )
    .await
}

/// Where a test harness on `port` takes ACP, as a bridge's endpoint.
pub fn acp(port: u16) -> String {
    format!(
        "http://127.0.0.1:{port}{}",
        link_connection_contract::acp::ACP_PATH
    )
}

/// How a message is asked of a test harness ([`ask_as`]): with its key,
/// naming its agent and ID when given, from someone else when given, its
/// conversation narrowed to `tools` and run on `model` when given.
#[derive(Clone, Copy, Default)]
pub struct Asking<'a> {
    pub key: &'a str,
    pub agent: Option<&'a str>,
    pub id: Option<&'a str>,
    pub from: Option<&'a str>,
    pub tools: Option<&'a [String]>,
    pub model: Option<&'a str>,
}

/// Asks `text` on `thread` of the harness on `port`, over ACP as a bridge
/// asks it: its whole answer, or why it was refused (the status a refused
/// connection got among it).
pub async fn ask_as(
    port: u16,
    thread: &str,
    text: &str,
    asking: Asking<'_>,
) -> Result<String, String> {
    ask_at(&acp(port), thread, text, asking).await
}

/// [`ask_as`], of the harness whose ACP is at `endpoint`.
pub async fn ask_at(
    endpoint: &str,
    thread: &str,
    text: &str,
    asking: Asking<'_>,
) -> Result<String, String> {
    let bridge = link_connection_contract::Bridge {
        agent: asking.agent.unwrap_or_default().into(),
        identity: link_connection_contract::bridge::Identity {
            agent: asking.id.unwrap_or_default().into(),
            ..Default::default()
        },
        endpoint: endpoint.into(),
        key: asking.key.to_string().into(),
        parts: true,
        settings: Value::Null.into(),
        ..Default::default()
    };
    let from = asking.from.map(|from| link_connection_contract::Speaker {
        from,
        name: None,
        hops: 0,
    });
    let (progress, _) = tokio::sync::watch::channel(String::new());
    let asked = link_connection_contract::ask_accepted(
        &bridge,
        thread,
        text,
        from,
        asking.tools,
        asking.model,
    );
    match async { asked.await?.answer(&progress).await }.await {
        Ok(link_connection_contract::Asked::Answer(answer)) => Ok(answer),
        Ok(link_connection_contract::Asked::Steered) => Ok(String::new()),
        Err(err) => Err(format!("{err:#}")),
    }
}

/// What [`ask_as`] came to as a status: 200 and the answer; a refused
/// connection's status and why; none when nothing answered on its port.
pub fn answered(asked: Result<String, String>) -> Option<(u16, String)> {
    match asked {
        Ok(answer) => Some((200, answer)),
        Err(why) if why.contains("reaching the agent") => None,
        Err(why) => {
            let status = why
                .split_once("the agent answered ")
                .and_then(|(_, rest)| rest.split_whitespace().next()?.parse().ok())
                .unwrap_or(0);
            Some((status, why))
        }
    }
}

/// The status an ACP connection to `endpoint` gets, opened with
/// `authorization` and `headers`: 101 when it opens.
pub async fn opens(endpoint: &str, authorization: Option<&str>, headers: &[(&str, &str)]) -> u16 {
    use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
    let address = endpoint.replacen("http://", "ws://", 1);
    let mut request = address.into_client_request().unwrap();
    let sent = request.headers_mut();
    for (name, value) in authorization
        .map(|a| ("authorization", a))
        .iter()
        .chain(headers)
    {
        sent.insert(
            tungstenite::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok(_) => 101,
        Err(tungstenite::Error::Http(response)) => response.status().as_u16(),
        Err(err) => panic!("{err}"),
    }
}

/// ACP's lines over a WebSocket to `endpoint`, opened with `key`.
pub async fn acp_lines(
    endpoint: &str,
    key: &str,
) -> impl link_agent_state_protocol::acp::ConnectTo<link_agent_state_protocol::acp::Client> {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
    let mut request = endpoint
        .replacen("http://", "ws://", 1)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {key}").parse().unwrap());
    let (socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("the key opens it");
    let (sink, stream) = socket.split();
    let outgoing = sink
        .sink_map_err(std::io::Error::other)
        .with(|line: String| async move {
            Ok::<_, std::io::Error>(tungstenite::Message::Text(line.into()))
        });
    let incoming = stream.filter_map(|frame| async move {
        match frame {
            Ok(tungstenite::Message::Text(text)) => Some(Ok(text.to_string())),
            Ok(_) => None,
            Err(err) => Some(Err(std::io::Error::other(err))),
        }
    });
    link_agent_state_protocol::acp::Lines::new(outgoing, incoming)
}

/// Prompts `thread` of the harness whose ACP is at `endpoint` with
/// `blocks`, as any ACP client may: the answer's text, or why it was
/// refused.
pub async fn prompt(
    endpoint: &str,
    key: &str,
    thread: &str,
    blocks: Vec<link_agent_state_protocol::acp::schema::v1::ContentBlock>,
) -> Result<String, String> {
    prompt_with(endpoint, key, thread, blocks, Value::Null).await
}

/// [`prompt`], with `meta` as its `_meta`, as a client of any making may
/// send it: none when null.
pub async fn prompt_with(
    endpoint: &str,
    key: &str,
    thread: &str,
    blocks: Vec<link_agent_state_protocol::acp::schema::v1::ContentBlock>,
    meta: Value,
) -> Result<String, String> {
    use link_agent_state_protocol::acp;
    use link_agent_state_protocol::acp::schema::v1::{
        ContentBlock, InitializeRequest, PromptRequest, ResumeSessionRequest, SessionId,
        SessionNotification, SessionUpdate,
    };
    let said = Arc::new(Mutex::new(String::new()));
    let heard = said.clone();
    let session = SessionId::new(thread);
    let answered = acp::Client
        .builder()
        .on_receive_notification(
            async move |update: SessionNotification, _cx| {
                if let SessionUpdate::AgentMessageChunk(chunk) = update.update
                    && let ContentBlock::Text(text) = chunk.content
                {
                    heard.lock().unwrap().push_str(&text.text);
                }
                Ok(())
            },
            acp::on_receive_notification!(),
        )
        .connect_with(
            acp_lines(endpoint, key).await,
            async |cx: acp::ConnectionTo<acp::Agent>| {
                cx.send_request(InitializeRequest::new(acp::schema::ProtocolVersion::V1))
                    .block_task()
                    .await?;
                cx.send_request(ResumeSessionRequest::new(session.clone(), "/"))
                    .block_task()
                    .await?;
                let mut asked = PromptRequest::new(session, blocks);
                asked.meta = meta.as_object().cloned();
                cx.send_request(asked).block_task().await
            },
        )
        .await;
    match answered {
        Ok(_) => Ok(said.lock().unwrap().clone()),
        Err(err) => Err(format!("{err:?}")),
    }
}

/// How many ports an agent of these tests takes: the Link Harness's one
/// and the one after it, for its tools. Its settings say so (`block`), or
/// its start would fit it to a block of its own elsewhere, one no test
/// holds.
pub const BLOCK: u32 = 2;

/// A base port whose block of [`BLOCK`] is free here, held by this test across
/// every test program on the machine until it's dropped: the agent the
/// test starts binds the block itself, so seeing it free doesn't keep
/// another run of this program from taking it first. From a range below
/// the ports the system hands out (from 32768) and Link's own (from
/// 18789), apart from the kit's (12000 to 15999) and Ollama's (11434).
pub fn free_block() -> link_test_kit::HeldNumber {
    let free = |p: u32| std::net::TcpListener::bind(("127.0.0.1", p as u16)).is_ok();
    // Spread by this process, so runs at once seldom try the same first.
    let start = (std::process::id() % 2000) * 8;
    let blocks: Vec<u32> = (9000..11000).step_by(BLOCK as usize).collect();
    let (passed, from) = blocks.split_at(start as usize % blocks.len());
    link_test_kit::HeldNumber::first("ports", from.iter().chain(passed).copied(), |base| {
        (base..base + BLOCK).all(free)
    })
}

/// Whether a test of the harness in Link's sandbox can run here: bubblewrap
/// and `curl`, through the kit's gates. Link's programs it runs come first
/// (`cargo build --workspace --bins`), so one not built fails the test.
#[cfg(target_os = "linux")]
pub fn sandboxed(built: &[&std::path::Path]) -> bool {
    for program in built {
        link_test_kit::programs::require(program, link_test_kit::programs::BUILD);
    }
    link_test_kit::gate::sandbox_with_landlock()
        && link_test_kit::gate::program("curl", &["--version"])
}

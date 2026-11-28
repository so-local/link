//! The hub against a fake harness: an ACP agent on in-process pipes that
//! echoes each prompt, and keeps what it was asked.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
    ListSessionsRequest, ListSessionsResponse, LoadSessionRequest, LoadSessionResponse,
    McpCapabilities, McpServer, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities, SessionId,
    SessionInfo, SessionListCapabilities, SessionNotification, SessionResumeCapabilities,
    SessionUpdate, StopReason, TextContent,
};
use agent_client_protocol::{self as sdk, ConnectionTo, Lines};
use futures_util::{SinkExt, StreamExt};
use link_connection_contract::acp::{Meta, Said, Taken};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

use super::*;
use link_harness_contract::asp;

mod continuity;
mod lifecycle;
#[cfg(target_os = "linux")]
mod native_lifetime;
mod recovery;

const KEY: &str = "the-endpoint-key";
const AGENT: &str = "hub-agent";

/// What the fake harness was asked, in order: `new`, `resume <id>`, and
/// each prompt as `<session>: <text>`; a session given MCP servers says
/// them after, `new with kit at <url>`.
type Asked = Arc<Mutex<Vec<String>>>;

/// A fake harness on in-process pipes: sessions `s1`, `s2`... numbered
/// from `first`, each prompt answered `echo: <text>`, taking MCP servers
/// over HTTP when `http`.
fn fake(asked: Asked, first: usize, http: bool) -> Pipes {
    fake_delayed(asked, first, http, None)
}

fn fake_delayed(
    asked: Asked,
    first: usize,
    http: bool,
    commands: Option<Arc<tokio::sync::Notify>>,
) -> Pipes {
    fake_options(asked, first, http, commands, false)
}

fn fake_options(
    asked: Asked,
    first: usize,
    http: bool,
    commands: Option<Arc<tokio::sync::Notify>>,
    refuse_load: bool,
) -> Pipes {
    let (hub_end, fake_end) = tokio::io::duplex(1 << 16);
    let (read, write) = tokio::io::split(fake_end);
    let made = Arc::new(AtomicUsize::new(first));
    let listed = made.clone();
    let transport = harness::lines::<sdk::Agent>(Box::new(read), Box::new(write), None);
    let keep = move |what: String| asked.lock().unwrap().push(what);
    let (k1, k2, k3, cancelled) = (keep.clone(), keep.clone(), keep.clone(), keep);
    tokio::spawn(async move {
        let _ = sdk::Agent
            .builder()
            .on_receive_request(
                async move |_asked: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(
                            AgentCapabilities::new()
                                .load_session(true)
                                .mcp_capabilities(McpCapabilities::new().http(http))
                                .session_capabilities(
                                    SessionCapabilities::new()
                                        .resume(SessionResumeCapabilities::new())
                                        .list(SessionListCapabilities::new()),
                                ),
                        ),
                    )
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async move |asked: NewSessionRequest, responder, cx: ConnectionTo<sdk::Client>| {
                    k1(format!("new{}", given(&asked.mcp_servers)));
                    let n = made.fetch_add(1, Ordering::SeqCst);
                    // Its commands, as Claude Code's adapter names one.
                    let offered = serde_json::from_value(serde_json::json!({
                        "availableCommands": [{"name": "clear", "description": "Clear it"}]
                    }))
                    .expect("an available_commands_update");
                    responder.respond(NewSessionResponse::new(SessionId::new(format!("s{n}"))))?;
                    let commands = commands.clone();
                    tokio::spawn(async move {
                        if let Some(commands) = commands {
                            commands.notified().await;
                        }
                        cx.send_notification(SessionNotification::new(
                            SessionId::new(format!("s{n}")),
                            SessionUpdate::AvailableCommandsUpdate(offered),
                        ))
                    });
                    Ok(())
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async move |asked: ResumeSessionRequest, responder, _cx| {
                    let servers = given(&asked.mcp_servers);
                    k2(format!("resume {}{servers}", asked.session_id.0));
                    responder.respond(ResumeSessionResponse::new())
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async move |_asked: ListSessionsRequest, responder, _cx| {
                    let sessions = (first..listed.load(Ordering::SeqCst))
                        .map(|n| SessionInfo::new(SessionId::new(format!("s{n}")), "/agent"))
                        .collect();
                    responder.respond(ListSessionsResponse::new(sessions))
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async move |asked: LoadSessionRequest, responder, cx: ConnectionTo<sdk::Client>| {
                    if refuse_load {
                        return responder.respond_with_result(Err(
                            sdk::Error::invalid_params().data("load refused")
                        ));
                    }
                    let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(format!(
                        "replayed {}",
                        asked.session_id.0
                    ))));
                    cx.send_notification(SessionNotification::new(
                        asked.session_id,
                        SessionUpdate::AgentMessageChunk(chunk),
                    ))?;
                    responder.respond(LoadSessionResponse::new())
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async move |asked: PromptRequest, responder, cx: ConnectionTo<sdk::Client>| {
                    let text = text_of(&asked.prompt);
                    k3(format!("{}: {text}", asked.session_id.0));
                    let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(format!(
                        "echo: {text}"
                    ))));
                    cx.send_notification(SessionNotification::new(
                        asked.session_id,
                        SessionUpdate::AgentMessageChunk(chunk),
                    ))?;
                    responder.respond(PromptResponse::new(StopReason::EndTurn))
                },
                sdk::on_receive_request!(),
            )
            .on_receive_notification(
                async move |asked: agent_client_protocol::schema::v1::CancelNotification, _cx| {
                    cancelled(format!("cancel {}", asked.session_id.0));
                    Ok(())
                },
                sdk::on_receive_notification!(),
            )
            .connect_to(transport)
            .await;
    });
    let (read, write) = tokio::io::split(hub_end);
    Pipes {
        read: Box::new(read),
        write: Box::new(write),
        child: None,
    }
}

/// ` with <name> at <url>, ...` for each server given, else nothing.
fn given(servers: &[McpServer]) -> String {
    let each: Vec<String> = servers
        .iter()
        .map(|server| match server {
            McpServer::Http(http) => format!("{} at {}", http.name, http.url),
            other => format!("{other:?}"),
        })
        .collect();
    match each.is_empty() {
        true => String::new(),
        false => format!(" with {}", each.join(", ")),
    }
}

fn text_of(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

/// A hub on a fake harness, its sessions kept in `dir`, counting starts.
fn hub(dir: &std::path::Path, asked: &Asked, first: usize) -> (Arc<Hub>, Arc<AtomicUsize>) {
    hub_with(dir, asked, first, Tools::default(), true)
}

/// [`hub`], giving `tools` to a harness taking them over HTTP when `http`.
fn hub_with(
    dir: &std::path::Path,
    asked: &Asked,
    first: usize,
    tools: Tools,
    http: bool,
) -> (Arc<Hub>, Arc<AtomicUsize>) {
    let starts = Arc::new(AtomicUsize::new(0));
    let (asked, counted) = (asked.clone(), starts.clone());
    let hub = Hub::new(Config {
        start: Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(fake(asked.clone(), first, http))
        }),
        cwd: "/agent".into(),
        key: Some(link_secretive::Secret::from(KEY.to_string())),
        agent: Some(AGENT.into()),
        sessions: dir.join("sessions.json"),
        state: None,
        tools,
    });
    (hub.with_calls(Duration::from_secs(10)), starts)
}

async fn served(hub: Arc<Hub>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, hub.router()).await });
    format!("127.0.0.1:{}", at.port())
}

/// What a bridge was told of one prompt: `taken` when it was, each piece
/// of its answer, and how it ended.
#[derive(Debug, Default)]
struct Heard {
    taken: bool,
    pieces: Vec<String>,
    sessions: Vec<String>,
}

/// Says `text` on `thread` as a bridge does, as `said`: its own
/// connection, `initialize`, `session/resume`, then the prompt.
async fn say(at: &str, thread: &str, text: &str, said: Said) -> (Heard, sdk::Result<()>) {
    let mut request = format!("ws://{at}/acp").into_client_request().unwrap();
    let headers = request.headers_mut();
    headers.insert("authorization", format!("Bearer {KEY}").parse().unwrap());
    headers.insert(
        link_connection_contract::AGENT_HEADER,
        AGENT.parse().unwrap(),
    );
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let (sink, stream) = socket.split();
    let outgoing = sink
        .sink_map_err(std::io::Error::other)
        .with(|line: String| async move { Ok::<_, std::io::Error>(Message::Text(line.into())) });
    let incoming = stream.filter_map(|frame| async move {
        match frame {
            Ok(Message::Text(text)) => Some(Ok(text.to_string())),
            Ok(_) => None,
            Err(err) => Some(Err(std::io::Error::other(err))),
        }
    });
    let heard = Arc::new(Mutex::new(Heard::default()));
    let (h1, h2) = (heard.clone(), heard.clone());
    let session = SessionId::new(thread);
    let prompt = PromptRequest::new(
        session.clone(),
        vec![ContentBlock::Text(TextContent::new(text))],
    )
    .meta(said.into_meta(Meta::new()));
    let ended = sdk::Client
        .builder()
        .on_receive_notification(
            async move |said: SessionNotification, _cx| {
                let mut heard = h1.lock().unwrap();
                heard.sessions.push(said.session_id.0.to_string());
                if let SessionUpdate::AgentMessageChunk(chunk) = said.update {
                    heard.pieces.push(text_of(&[chunk.content]));
                }
                Ok(())
            },
            sdk::on_receive_notification!(),
        )
        .on_receive_notification(
            async move |taken: Taken, _cx| {
                let mut heard = h2.lock().unwrap();
                heard.taken = true;
                heard.sessions.push(taken.session_id.0.to_string());
                Ok(())
            },
            sdk::on_receive_notification!(),
        )
        .connect_with(
            Lines::new(outgoing, incoming),
            async |cx: ConnectionTo<sdk::Agent>| {
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                cx.send_request(ResumeSessionRequest::new(session, "/"))
                    .block_task()
                    .await?;
                cx.send_request(prompt).block_task().await.map(drop)
            },
        )
        .await;
    let heard = std::mem::take(&mut *heard.lock().unwrap());
    (heard, ended)
}

fn temp(name: &str) -> link_test_kit::TempDir {
    link_test_kit::TempDir::new(name)
}

#[tokio::test]
async fn each_thread_is_a_session_kept_across_restarts() {
    let dir = temp("hub-sessions");
    let asked = Asked::default();
    let (one, starts) = hub(&dir, &asked, 1);
    let at = served(one).await;
    let (heard, ended) = say(&at, "telegram:42", "Hello", Said::default()).await;
    ended.unwrap();
    assert!(heard.taken, "{heard:?}");
    assert_eq!(heard.pieces, ["echo: Hello"]);
    assert!(
        heard.sessions.iter().all(|s| s == "telegram:42"),
        "{heard:?}"
    );
    say(&at, "telegram:42", "Again", Said::default())
        .await
        .1
        .unwrap();
    say(&at, "slack:C1", "Other", Said::default())
        .await
        .1
        .unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(
        *asked.lock().unwrap(),
        ["new", "s1: Hello", "s1: Again", "new", "s2: Other"]
    );

    // A new hub, as after a restart: the thread's session taken up.
    asked.lock().unwrap().clear();
    let (two, _) = hub(&dir, &asked, 10);
    let at = served(two).await;
    say(&at, "telegram:42", "Back", Said::default())
        .await
        .1
        .unwrap();
    assert_eq!(*asked.lock().unwrap(), ["resume s1", "s1: Back"]);
}

/// Link's tools, as an agent's settings give them: one tool and memory.
fn link_tools() -> Tools {
    let mut settings = contract::Settings::default();
    settings
        .tools
        .insert("kit".into(), "http://127.0.0.1:9/mcp/kit".into());
    settings.memory = Some("http://127.0.0.1:9/mcp/memory".into());
    Tools::of(&settings)
}

/// Link's tools go with each session the harness makes or takes up, and
/// what `configure` kept is what `serve` gives.
#[tokio::test]
async fn each_session_is_given_link_s_tools_as_its_mcp_servers() {
    let dir = temp("hub-tools");
    let kept = dir.join("tools.json");
    link_tools().keep(&kept).unwrap();
    let tools = Tools::kept(&kept).unwrap();
    assert_eq!(tools, link_tools());
    let asked = Asked::default();
    let at = served(hub_with(&dir, &asked, 1, tools.clone(), true).0).await;
    say(&at, "telegram:42", "Hello", Said::default())
        .await
        .1
        .unwrap();
    let with = " with kit at http://127.0.0.1:9/mcp/kit, memory at http://127.0.0.1:9/mcp/memory";
    assert_eq!(asked.lock().unwrap()[0], format!("new{with}"));

    asked.lock().unwrap().clear();
    let at = served(hub_with(&dir, &asked, 10, tools, true).0).await;
    say(&at, "telegram:42", "Back", Said::default())
        .await
        .1
        .unwrap();
    assert_eq!(asked.lock().unwrap()[0], format!("resume s1{with}"));
    assert_eq!(
        Tools::kept(&dir.join("none.json")).unwrap(),
        Tools::default()
    );
}

/// A harness that takes no MCP server over HTTP is given none.
#[tokio::test]
async fn a_harness_without_http_mcp_is_given_no_servers() {
    let dir = temp("hub-no-tools");
    let asked = Asked::default();
    let at = served(hub_with(&dir, &asked, 1, link_tools(), false).0).await;
    say(&at, "telegram:42", "Hello", Said::default())
        .await
        .1
        .unwrap();
    assert_eq!(*asked.lock().unwrap(), ["new", "s1: Hello"]);
}

#[tokio::test]
async fn a_sender_who_isn_t_the_owner_is_written_into_the_text() {
    let dir = temp("hub-from");
    let asked = Asked::default();
    let at = served(hub(&dir, &asked, 1).0).await;
    let said = Said {
        from: Some("member:slack:T0ACME/U0ALICE".into()),
        from_name: Some("Zoë Ng".into()),
        hops: 2,
        ..Said::default()
    };
    say(&at, "slack:C1", "Book the room.\nToday.", said)
        .await
        .1
        .unwrap();
    assert_eq!(
        asked.lock().unwrap().last().unwrap(),
        "s1: (from \"Zoë Ng\" (member:slack:T0ACME/U0ALICE), 2 messages on, not your owner:)\n\
         > Book the room.\n> Today."
    );
}

#[tokio::test]
async fn a_command_goes_to_the_harness_by_the_name_it_offers() {
    let dir = temp("hub-crossing");
    let asked = Asked::default();
    let at = served(hub(&dir, &asked, 1).0).await;
    for typed in ["/chat:create", "/new"] {
        say(&at, "slack:C1", typed, Said::default())
            .await
            .1
            .unwrap();
        assert_eq!(asked.lock().unwrap().last().unwrap(), "s1: /clear");
    }
    say(&at, "slack:C1", "/deploy", Said::default())
        .await
        .1
        .unwrap();
    assert_eq!(asked.lock().unwrap().last().unwrap(), "s1: /deploy");
}

#[tokio::test]
async fn a_sender_that_isn_t_one_is_refused_before_the_harness_sees_it() {
    let dir = temp("hub-not-from");
    let asked = Asked::default();
    let at = served(hub(&dir, &asked, 1).0).await;
    let said = Said {
        from: Some("owner:me".into()),
        ..Said::default()
    };
    let (heard, ended) = say(&at, "slack:C1", "Hi", said).await;
    assert!(ended.is_err() && !heard.taken, "{heard:?}");
    assert!(!asked.lock().unwrap().iter().any(|a| a.contains("Hi")));
}

#[tokio::test]
async fn a_connection_needs_the_key_and_the_agent() {
    let dir = temp("hub-key");
    let at = served(hub(&dir, &Asked::default(), 1).0).await;
    let open = async |key: Option<&str>, agent: &str| {
        let mut request = format!("ws://{at}/acp").into_client_request().unwrap();
        let headers = request.headers_mut();
        if let Some(key) = key {
            headers.insert("authorization", format!("Bearer {key}").parse().unwrap());
        }
        headers.insert(
            link_connection_contract::AGENT_HEADER,
            agent.parse().unwrap(),
        );
        match tokio_tungstenite::connect_async(request).await {
            Ok(_) => 101,
            Err(tokio_tungstenite::tungstenite::Error::Http(answer)) => answer.status().as_u16(),
            Err(err) => panic!("{err}"),
        }
    };
    assert_eq!(open(None, AGENT).await, 401);
    assert_eq!(open(Some("another"), AGENT).await, 401);
    assert_eq!(open(Some(KEY), "another-agent").await, 404);
    assert_eq!(open(Some(KEY), AGENT).await, 101);
}

/// An endpoint without its key serves nothing: 503, whatever is sent.
#[tokio::test]
async fn an_endpoint_without_its_key_serves_nothing() {
    let dir = temp("hub-no-key");
    let hub = Hub::new(Config {
        start: Box::new(|| Err(std::io::Error::other("never started"))),
        cwd: "/agent".into(),
        key: None,
        agent: Some(AGENT.into()),
        sessions: dir.join("sessions.json"),
        state: None,
        tools: Tools::default(),
    });
    let at = served(hub).await;
    for key in [None, Some(KEY)] {
        let mut request = format!("ws://{at}/acp").into_client_request().unwrap();
        if let Some(key) = key {
            let value = format!("Bearer {key}").parse().unwrap();
            request.headers_mut().insert("authorization", value);
        }
        match tokio_tungstenite::connect_async(request).await {
            Err(tokio_tungstenite::tungstenite::Error::Http(answer)) => {
                assert_eq!(answer.status().as_u16(), 503);
            }
            other => panic!("served without a key: {:?}", other.map(|_| ())),
        }
    }
}

#[tokio::test]
#[expect(
    clippy::disallowed_methods,
    reason = "the harness's end is seen on a task of the hub's: a retry waits for it"
)]
async fn a_stopped_harness_is_started_again() {
    let dir = temp("hub-restart");
    let asked = Asked::default();
    let starts = Arc::new(AtomicUsize::new(0));
    let counted = starts.clone();
    // Each start's harness ends after its first prompt.
    let hub = Hub::new(Config {
        start: Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            let pipes = fake(asked.clone(), 1, true);
            Ok(Pipes {
                read: Box::new(EndsAfter(pipes.read, 4)),
                ..pipes
            })
        }),
        cwd: "/agent".into(),
        key: Some(link_secretive::Secret::from(KEY.to_string())),
        agent: None,
        sessions: dir.join("sessions.json"),
        state: None,
        tools: Tools::default(),
    });
    let at = served(hub).await;
    say(&at, "a", "One", Said::default()).await.1.unwrap();
    // The harness's stdout ends once the turn is over.
    let mut waited = 0;
    loop {
        if say(&at, "a", "Two", Said::default()).await.1.is_ok() {
            break;
        }
        waited += 1;
        assert!(waited < 50, "never started again");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(starts.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn a_harness_that_fails_to_start_is_not_started_at_every_check() {
    let starts = Arc::new(AtomicUsize::new(0));
    let counted = starts.clone();
    let harness = crate::harness::Harness::new(
        Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Err(std::io::Error::other("no harness here"))
        }),
        &Tools::default().actions,
    );
    for _ in 0..5 {
        let Err(err) = harness.up_within(Duration::from_secs(1)).await else {
            panic!("a harness that can't start answered");
        };
        assert!(err.to_string().contains("no harness here"), "{err:#}");
    }
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[expect(
    clippy::disallowed_methods,
    reason = "each harness's end is seen on a task of the hub's: the next call waits for it"
)]
async fn a_harness_that_keeps_ending_is_given_up_on_and_says_why() {
    let starts = Arc::new(AtomicUsize::new(0));
    let counted = starts.clone();
    // Each start agrees on ACP, then its stdout ends: a harness that
    // refuses to run past its start.
    let harness = crate::harness::Harness::new(
        Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            let pipes = fake(Asked::default(), 1, true);
            Ok(Pipes {
                read: Box::new(EndsAfter(pipes.read, 1)),
                ..pipes
            })
        }),
        &Tools::default().actions,
    );
    let most = crate::harness::RESTARTS.most_in_a_row as usize;
    let mut refused = None;
    for _ in 0..500 {
        match harness.up_within(Duration::from_secs(5)).await {
            Err(err) if err.to_string().contains("stopped starting it") => {
                refused = Some(err.to_string());
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
    let refused = refused.expect("the hub kept starting a harness that keeps ending");
    assert!(refused.contains("ended"), "{refused}");
    assert_eq!(starts.load(Ordering::SeqCst), most);
    // Given up, it stays given up: no more starts.
    assert!(harness.up_within(Duration::from_secs(1)).await.is_err());
    assert_eq!(starts.load(Ordering::SeqCst), most);
}

/// A harness's stdout that ends after `.1` lines.
struct EndsAfter(Box<dyn tokio::io::AsyncRead + Unpin + Send>, usize);

impl tokio::io::AsyncRead for EndsAfter {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.1 == 0 {
            return std::task::Poll::Ready(Ok(()));
        }
        let before = buf.filled().len();
        let polled = std::pin::Pin::new(&mut self.0).poll_read(cx, buf);
        let lines = buf.filled()[before..]
            .iter()
            .filter(|b| **b == b'\n')
            .count();
        self.1 = self.1.saturating_sub(lines);
        polled
    }
}

#[test]
fn threads_kept_at_once_are_all_kept() {
    let dir = temp("hub-at-once");
    let sessions = Arc::new(sessions::Sessions::at(dir.join("sessions.json")));
    let threads: Vec<String> = (0..16).map(|n| format!("signal:g-{n:02}")).collect();
    let kept: Vec<_> = threads
        .iter()
        .map(|thread| {
            let (sessions, thread) = (sessions.clone(), thread.clone());
            std::thread::spawn(move || sessions.keep(&thread, &format!("s-{thread}")))
        })
        .collect();
    for keeping in kept {
        keeping.join().unwrap().unwrap();
    }
    let file = hub_sessions(&dir);
    assert_eq!(file.keys().cloned().collect::<Vec<_>>(), threads);
}

/// A harness's own store, in memory: its sessions by the harness's ids,
/// each import's sessions made `n1`, `n2`...
#[derive(Default)]
struct Kept(Mutex<Vec<asp::forms::Session>>);

impl Store for Kept {
    fn kinds(&self) -> asp::AgentState {
        asp::AgentState {
            export: vec![asp::Kind::Sessions],
            import: vec![asp::Kind::Sessions],
        }
    }

    fn export(&self, _kinds: &[asp::Kind]) -> anyhow::Result<asp::State> {
        Ok(asp::State {
            sessions: Some(self.0.lock().unwrap().clone()),
            ..asp::State::default()
        })
    }

    fn import(&self, state: asp::State) -> anyhow::Result<asp::methods::Imported> {
        let mut kept = self.0.lock().unwrap();
        let mut imported = asp::methods::Imported::default();
        for mut session in state.sessions.unwrap_or_default() {
            let old = session.session_id.clone();
            session.session_id = SessionId::new(format!("n{}", kept.len() + 1));
            imported.sessions.insert(old, session.session_id.clone());
            kept.push(session);
        }
        Ok(imported)
    }
}

/// Runs `asking` on a bridge's connection to the hub at `at`.
async fn asking<T: Send + 'static>(
    at: &str,
    asking: impl AsyncFnOnce(ConnectionTo<sdk::Agent>) -> sdk::Result<T> + Send,
) -> sdk::Result<T> {
    let mut request = format!("ws://{at}/acp").into_client_request().unwrap();
    let headers = request.headers_mut();
    headers.insert("authorization", format!("Bearer {KEY}").parse().unwrap());
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let (sink, stream) = socket.split();
    let outgoing = sink
        .sink_map_err(std::io::Error::other)
        .with(|line: String| async move { Ok::<_, std::io::Error>(Message::Text(line.into())) });
    let incoming = stream.filter_map(|frame| async move {
        match frame {
            Ok(Message::Text(text)) => Some(Ok(text.to_string())),
            _ => None,
        }
    });
    sdk::Client
        .builder()
        .connect_with(Lines::new(outgoing, incoming), asking)
        .await
}

#[tokio::test]
async fn agent_state_names_each_session_by_its_thread_both_ways() {
    use asp::forms::{By, Message as Said_, Session};
    let dir = temp("hub-state");
    let asked = Asked::default();
    let said = |by, text: &str| Said_ {
        by,
        text: text.into(),
    };
    let conversation = [said(By::User, "Hi"), said(By::Agent, "Hello")];
    let kept = Arc::new(Kept::default());
    let (hub, _) = hub(&dir, &asked, 1);
    let mut hub = Arc::into_inner(hub).unwrap();
    hub.state = Some(kept.clone());
    let at = served(Arc::new(hub)).await;
    // `s1` is telegram:42's; `cli-7` was made in the harness's own way.
    say(&at, "telegram:42", "Hi", Said::default())
        .await
        .1
        .unwrap();
    for id in ["s1", "cli-7"] {
        let session = Session::of_messages(SessionId::new(id), None, &conversation);
        kept.0.lock().unwrap().push(session);
    }

    let (advertised, exported) = asking(&at, async |cx| {
        let up = cx
            .send_request(InitializeRequest::new(ProtocolVersion::V1))
            .block_task()
            .await?;
        let asked = asp::methods::Export {
            kinds: vec![asp::Kind::Sessions],
        };
        let exported = cx.send_request(asked).block_task().await?;
        Ok((asp::AgentState::of(&up.agent_capabilities), exported))
    })
    .await
    .unwrap();
    assert_eq!(advertised, Some(kept.kinds()));
    let sessions = exported.state.sessions.unwrap();
    let names: Vec<&str> = sessions.iter().map(|s| &*s.session_id.0).collect();
    assert_eq!(names, ["telegram:42", "cli-7"]);
    assert_eq!(sessions[0].messages(), conversation);

    // In again: each kept as its thread, telegram:42's chat following
    // the import over the session it had.
    let (imported, listed) = asking(&at, async |cx| {
        let asked = asp::methods::Import {
            state: asp::State {
                sessions: Some(sessions),
                ..asp::State::default()
            },
        };
        let imported = cx.send_request(asked).block_task().await?;
        let listed = cx
            .send_request(agent_client_protocol::schema::v1::ListSessionsRequest::new())
            .block_task()
            .await?;
        Ok((imported, listed))
    })
    .await
    .unwrap();
    let cli = &imported.sessions[&SessionId::new("cli-7")];
    assert_eq!(cli.0.as_ref(), "cli-7");
    let again = &imported.sessions[&SessionId::new("telegram:42")];
    assert_eq!(again.0.as_ref(), "telegram:42", "{imported:?}");
    let listed: Vec<String> = listed
        .sessions
        .iter()
        .map(|s| s.session_id.0.to_string())
        .collect();
    assert!(listed.contains(&"cli-7".to_string()), "{listed:?}");
    assert_eq!(hub_sessions(&dir)["cli-7"], "n4");
    assert_eq!(hub_sessions(&dir)[again.0.as_ref()], "n3");
}

/// The thread-to-session map a hub kept in `dir`.
fn hub_sessions(dir: &std::path::Path) -> std::collections::BTreeMap<String, String> {
    link_files::saved::parse_json::<sessions::Kept>(
        &std::fs::read(dir.join("sessions.json")).unwrap(),
    )
    .unwrap()
    .threads
}

#[tokio::test]
async fn a_harness_that_lists_and_loads_its_sessions_has_them_exported() {
    let dir = temp("hub-replayed");
    let asked = Asked::default();
    let (hub, _) = hub(&dir, &asked, 1);
    let at = served(hub).await;
    say(&at, "telegram:42", "Hi", Said::default())
        .await
        .1
        .unwrap();
    let (advertised, exported, imported) = asking(&at, async |cx| {
        let up = cx
            .send_request(InitializeRequest::new(ProtocolVersion::V1))
            .block_task()
            .await?;
        let asked = asp::methods::Export {
            kinds: vec![asp::Kind::Sessions],
        };
        let exported = cx.send_request(asked).block_task().await?;
        let asked = asp::methods::Import {
            state: exported.state.clone(),
        };
        let imported = cx.send_request(asked).block_task().await?;
        Ok((
            asp::AgentState::of(&up.agent_capabilities),
            exported,
            imported,
        ))
    })
    .await
    .unwrap();
    let advertised = advertised.unwrap();
    assert_eq!(advertised.export, [asp::Kind::Sessions]);
    assert!(advertised.import.is_empty());
    let sessions = exported.state.sessions.unwrap();
    assert_eq!(&*sessions[0].session_id.0, "telegram:42");
    assert_eq!(sessions[0].messages()[0].text, "replayed s1");
    assert_eq!(imported.not_taken[0].kind, asp::Kind::Sessions);
    assert!(imported.sessions.is_empty());
}

/// The list can follow session/new's response. The first command waits
/// for that session's list, even when another session already has one.
#[tokio::test]
async fn a_first_command_waits_for_its_own_delayed_list() {
    let dir = temp("hub-delayed-commands");
    let asked = Asked::default();
    let release = Arc::new(tokio::sync::Notify::new());
    let hub = Hub::new(Config {
        start: Box::new({
            let asked = asked.clone();
            let release = release.clone();
            move || Ok(fake_delayed(asked.clone(), 1, true, Some(release.clone())))
        }),
        cwd: "/agent".into(),
        key: Some(link_secretive::Secret::from(KEY.to_string())),
        agent: Some(AGENT.into()),
        sessions: dir.join("sessions.json"),
        state: None,
        tools: Tools::default(),
    })
    .with_calls(Duration::from_secs(10));
    let at = served(hub.clone()).await;
    let up = hub.up().await.unwrap();
    let pending =
        tokio::spawn(async move { say(&at, "telegram:42", "/chat:create", Said::default()).await });
    link_test_kit::wait::eventually_async(
        "first command waiting for discovery",
        Duration::from_secs(5),
        || {
            hub.sessions
                .of("telegram:42")
                .unwrap()
                .is_some_and(|id| up.waiting_commands(&id))
        },
    )
    .await;
    assert_eq!(*asked.lock().unwrap(), ["new"]);
    release.notify_one();
    pending.await.unwrap().1.unwrap();
    assert_eq!(*asked.lock().unwrap(), ["new", "s1: /clear"]);
}

#[tokio::test]
async fn namespace_commands_do_not_reach_the_native_harness_or_run_for_other_senders() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = format!("http://{}", listener.local_addr().unwrap());
    let app=axum::Router::new().route("/agent/show",axum::routing::get(move||{counted.fetch_add(1,Ordering::SeqCst);async{axum::Json(serde_json::json!({"name":"agent","harness":"fake","model":"qwen3:8b","window":8192,"answer":512}))}}));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let settings = contract::Settings {
        actions: link_harness_contract::sandbox::namespace::Endpoints {
            urls: std::collections::BTreeMap::from([("agent".into(), format!("{root}/agent"))]),
            actions: vec!["agent show".into()],
        },
        ..Default::default()
    };
    let dir = temp("hub-actions");
    let asked = Asked::default();
    let (hub, _) = hub_with(&dir, &asked, 1, Tools::of(&settings), true);
    let at = served(hub).await;
    let (heard, ended) = say(&at, "telegram:42", "/whoami", Said::default()).await;
    ended.unwrap();
    assert!(heard.taken);
    assert!(heard.pieces.concat().contains("qwen3:8b"));
    assert_eq!(*asked.lock().unwrap(), ["new"]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (_, ended) = say(
        &at,
        "telegram:42",
        "/web:fetch {\"url\":\"https://example.com\"}",
        Said::default(),
    )
    .await;
    assert!(ended.is_err());
    assert_eq!(*asked.lock().unwrap(), ["new"]);
    let (_, ended) = say(
        &at,
        "telegram:42",
        "/whoami",
        Said {
            from: Some("schedule:a1b2c3d4e5f6".into()),
            ..Default::default()
        },
    )
    .await;
    ended.unwrap();
    assert!(
        asked
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains("not your owner"))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
}

/// State waits for a running turn to end, rather than being refused.
#[tokio::test]
async fn state_waits_for_the_turn_running() {
    let dir = temp("hub-idle");
    let (hub, _) = hub(&dir, &Asked::default(), 1);
    let operation = hub.activity.enter("telegram:42").await;
    let mut capture = Box::pin(hub.activity.capture());
    assert!(futures_util::poll!(capture.as_mut()).is_pending());
    drop(operation);
    capture.await.unwrap();
}

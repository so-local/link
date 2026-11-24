//! Link Harness as an ACP agent: each connection a WebSocket on its
//! endpoint's port ([`link_connection_contract::acp::ACP_PATH`]), opened
//! with the agent's headers and its endpoint key, one JSON-RPC message
//! to a text frame. A session is a conversation, its id the thread's
//! name (`telegram:42`): `session/new` names one or makes one, `load`
//! replays one's events as ACP's updates, `resume` takes one up, and
//! `list` lists them. `session/prompt` is a message, answered as a turn
//! streamed in `session/update`s; `session/cancel` ends it. ASP's agent
//! state is its conversations, carried both ways ([`state`]).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use link_agent_state_protocol::acp::schema::ProtocolVersion;
use link_agent_state_protocol::acp::schema::v1::{
    AgentCapabilities, AvailableCommandsUpdate, CancelNotification, Implementation,
    InitializeRequest, InitializeResponse, ListSessionsRequest, ListSessionsResponse,
    LoadSessionRequest, LoadSessionResponse, NewSessionRequest, NewSessionResponse,
    PromptCapabilities, ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities,
    SessionId, SessionInfo, SessionListCapabilities, SessionNotification,
    SessionResumeCapabilities, SessionUpdate,
};
use link_agent_state_protocol::acp::{self, Client, ConnectionTo, Lines};
use link_connection_contract::acp::Said;

use crate::agent::{Agent, Thread};
use crate::server::Server;

pub(crate) mod history;
mod prompt;
mod state;
pub use state::{exported_state, kinds as state_kinds};

/// One connection's: the agent it was opened for, and the sessions it
/// opened, the only ones it may prompt or cancel.
struct Connection {
    server: Arc<Server>,
    agent: Agent,
    opened: Mutex<BTreeSet<Thread>>,
}

impl Connection {
    fn open(&self, thread: &Thread) {
        self.opened
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(thread.clone());
    }

    /// The thread of `session`, when this connection opened it.
    fn opened(&self, session: &SessionId) -> acp::Result<Thread> {
        let thread = thread_of(session)?;
        match self
            .opened
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&thread)
        {
            true => Ok(thread),
            false => Err(invalid(format!(
                "session {} isn't open here: session/new, load or resume it first",
                thread.name()
            ))),
        }
    }

    /// The commands its owner may send, as ACP lists them for a client.
    fn commands(&self, cx: &ConnectionTo<Client>, session: &SessionId) -> acp::Result<()> {
        let config = self.server.harness.config(&self.agent).map_err(internal)?;
        // ACP's form of each, built from its JSON: the name and what
        // it's for, and what `/model` takes after it.
        let commands = crate::commands::list(&config)
            .into_iter()
            .map(|command| {
                let mut listed = serde_json::json!({
                    "name": command.name, "description": command.description,
                });
                if command.does == crate::commands::Does::ModelSet {
                    listed["input"] = serde_json::json!({"hint": "a model's name"});
                }
                serde_json::from_value(listed).map_err(internal)
            })
            .collect::<acp::Result<Vec<_>>>()?;
        update(
            cx,
            session,
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands)),
        )
    }
}

/// One ACP connection counted as open while it's served.
struct Open(Arc<std::sync::atomic::AtomicUsize>);

impl Open {
    fn new(count: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(count)
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Serves ACP to `agent` over `socket` until either side closes it.
pub async fn serve(server: Arc<Server>, agent: Agent, socket: WebSocket) {
    let _open = Open::new(server.connections.clone());
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
    let connection = Arc::new(Connection {
        server,
        agent,
        opened: Mutex::default(),
    });
    let served = acp::Agent
        .builder()
        .name("link-harness")
        .on_receive_request(
            {
                let connection = connection.clone();
                async move |asked: InitializeRequest, responder, _cx| {
                    let config = connection
                        .server
                        .harness
                        .config(&connection.agent)
                        .map_err(internal)?;
                    responder.respond(initialized(asked.protocol_version, &config))
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            {
                let connection = connection.clone();
                async move |asked: NewSessionRequest, responder, cx| {
                    let said = Said::of(asked.meta.as_ref()).map_err(invalid)?;
                    let thread = match said.thread {
                        Some(name) => Thread::new(&name).map_err(invalid)?,
                        None => fresh(),
                    };
                    connection.open(&thread);
                    let session = SessionId::new(thread.name());
                    responder.respond(NewSessionResponse::new(session.clone()))?;
                    connection.commands(&cx, &session)
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            {
                let connection = connection.clone();
                async move |asked: LoadSessionRequest, responder, cx| {
                    let thread = thread_of(&asked.session_id)?;
                    let events = connection
                        .server
                        .harness
                        .events(&connection.agent, &thread)
                        .map_err(internal)?;
                    for said in history::replayed(&events) {
                        update(&cx, &asked.session_id, said)?;
                    }
                    connection.open(&thread);
                    responder.respond(LoadSessionResponse::new())?;
                    connection.commands(&cx, &asked.session_id)
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            {
                let connection = connection.clone();
                async move |asked: ResumeSessionRequest, responder, cx| {
                    let thread = thread_of(&asked.session_id)?;
                    connection.open(&thread);
                    responder.respond(ResumeSessionResponse::new())?;
                    connection.commands(&cx, &asked.session_id)
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            {
                let connection = connection.clone();
                async move |_asked: ListSessionsRequest, responder, _cx| {
                    let harness = &connection.server.harness;
                    let threads = harness.threads(&connection.agent).map_err(internal)?;
                    let at = harness.workspace(&connection.agent).unwrap_or_default();
                    let sessions = threads
                        .iter()
                        .map(|thread| SessionInfo::new(SessionId::new(thread.name()), at.clone()))
                        .collect();
                    responder.respond(ListSessionsResponse::new(sessions))
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            {
                let connection = connection.clone();
                async move |asked: link_agent_state_protocol::acp::schema::v1::PromptRequest,
                            responder,
                            cx| {
                    prompt::asked(connection.clone(), asked, responder, cx)
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            {
                let connection = connection.clone();
                async move |asked: link_connection_contract::Recover, responder, _cx| {
                    let answers = recovered(&connection, asked).await;
                    responder.respond(link_connection_contract::acp::RecoveredAnswers { answers })
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            {
                let connection = connection.clone();
                async move |_asked: link_agent_state_protocol::methods::Describe, responder, _cx| {
                    responder.respond(state::describe(&connection).map_err(internal)?)
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            {
                let connection = connection.clone();
                async move |asked: link_agent_state_protocol::methods::Export, responder, _cx| {
                    let state = state::export(&connection, &asked.kinds).map_err(internal)?;
                    responder.respond(link_agent_state_protocol::methods::Exported { state })
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            {
                let connection = connection.clone();
                async move |asked: link_agent_state_protocol::methods::Import, responder, _cx| {
                    responder.respond(state::import(&connection, asked.state).map_err(internal)?)
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let connection = connection.clone();
                async move |cancelled: CancelNotification, _cx| {
                    let thread = connection.opened(&cancelled.session_id)?;
                    connection
                        .server
                        .harness
                        .interrupt(&connection.agent, &thread)
                        .map_err(internal)
                }
            },
            acp::on_receive_notification!(),
        )
        .connect_to(Lines::new(outgoing, incoming))
        .await;
    if let Err(err) = served {
        crate::warn!("an ACP connection ended: {err}");
    }
}

/// The answers of the agent's turns finished after a crash, which nobody
/// waited for, for its bridge to send to the chat
/// ([`link_connection_contract::acp::RECOVER`]): any turn whose lease
/// lapsed since is finished first. Each handed over once; or, asked in
/// two phases, each with its `id`, until `done` says it was sent.
async fn recovered(
    connection: &Connection,
    asked: link_connection_contract::Recover,
) -> Vec<link_connection_contract::Recovered> {
    let (harness, agent) = (&connection.server.harness, &connection.agent);
    if !asked.done.is_empty() {
        harness.delivered(agent, &asked.done);
        return Vec::new();
    }
    if asked.two_phase {
        return harness
            .recovered_kept(agent)
            .await
            .into_iter()
            .map(|(id, thread, text)| link_connection_contract::Recovered {
                thread,
                text,
                id: Some(id),
            })
            .collect();
    }
    harness
        .recovered(agent)
        .await
        .into_iter()
        .map(|(thread, text)| link_connection_contract::Recovered {
            thread,
            text,
            id: None,
        })
        .collect()
}

/// What it answers `initialize` with: ACP's first version, whatever was
/// asked, since it speaks no other; and of ASP, which of steering's
/// choices a prompt may make, as `config` says.
fn initialized(_asked: ProtocolVersion, config: &crate::config::Config) -> InitializeResponse {
    use link_agent_spec::Busy;
    let busy = crate::turn::takes(config)
        .into_iter()
        .map(|busy| match busy {
            Busy::Steer => link_agent_state_protocol::Busy::Steer,
            Busy::Queue => link_agent_state_protocol::Busy::Queue,
            Busy::Interrupt => link_agent_state_protocol::Busy::Interrupt,
        })
        .collect();
    let asp = link_agent_state_protocol::Advertised {
        agent_state: Some(state::kinds()),
        steering: Some(link_agent_state_protocol::Steering { busy }),
    };
    InitializeResponse::new(ProtocolVersion::V1)
        .agent_capabilities(
            asp.on(AgentCapabilities::new()
                .load_session(true)
                .prompt_capabilities(PromptCapabilities::new().image(true).embedded_context(true))
                .session_capabilities(
                    SessionCapabilities::new()
                        .list(SessionListCapabilities::new())
                        .resume(SessionResumeCapabilities::new()),
                )),
        )
        .agent_info(Implementation::new(
            "link-harness",
            env!("CARGO_PKG_VERSION"),
        ))
}

/// A session's thread, by its id.
fn thread_of(session: &SessionId) -> acp::Result<Thread> {
    Thread::new(&session.0).map_err(invalid)
}

/// A new conversation's thread, for a session nobody named.
fn fresh() -> Thread {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    Thread::new(&format!("acp:{nanos:x}")).unwrap_or_else(|_| fresh())
}

fn update(
    cx: &ConnectionTo<Client>,
    session: &SessionId,
    update: SessionUpdate,
) -> acp::Result<()> {
    cx.send_notification(SessionNotification::new(session.clone(), update))
}

fn invalid(why: impl std::fmt::Display) -> acp::Error {
    acp::Error::invalid_params().data(format!("{why:#}"))
}

fn internal(why: impl std::fmt::Display) -> acp::Error {
    acp::Error::internal_error().data(format!("{why:#}"))
}

/// The non-text content usable after an ASP session import.
pub fn session_content() -> Vec<link_harness_contract::harness::switching::SessionContent> {
    state::content()
}

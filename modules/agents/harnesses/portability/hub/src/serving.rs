//! One bridge's connection, served as an ACP agent: a session is a
//! thread, its id the thread's name (`telegram:42`), as Link Harness
//! names them; the harness's own session behind it is the hub's to keep.
//! `session/new` and `resume` name a thread, `load` replays it if the
//! harness can, `list` lists them; `session/prompt` is forwarded to the
//! thread's session, told taken as it goes, its updates relayed back
//! under the thread's name; `session/cancel` ends it. ASP's agent state
//! is the [`crate::state`]'s.

use link_format::{Note, Sentence, aside, sentence};
use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, InitializeResponse, ListSessionsRequest,
    ListSessionsResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, ResumeSessionRequest, ResumeSessionResponse,
    SessionCapabilities, SessionId, SessionInfo, SessionListCapabilities,
    SessionResumeCapabilities, TextContent,
};
use agent_client_protocol::{self as sdk, ConnectionTo, Lines};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use link_connection_contract::acp::{Meta, Said, Taken};
use link_harness_contract::asp::methods::{Describe, Export, Import};
use link_harness_contract::asp::{Advertised, AgentState};

use crate::Hub;
use crate::harness::{Route, To, Up};

/// Serves ACP over `socket` until either side closes it.
pub(crate) async fn serve(hub: Arc<Hub>, socket: WebSocket) {
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
    let served = sdk::Agent
        .builder()
        .name("link-hub")
        .on_receive_request(
            {
                let hub = hub.clone();
                async move |_asked: InitializeRequest, responder, _cx| {
                    let up = hub.up().await?;
                    let advertised = hub.advertised(&up);
                    let mut answer = initialized(&up, advertised);
                    if hub.continuity.is_some() {
                        answer.agent_capabilities.load_session = true;
                    }
                    responder.respond(answer)
                }
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hub = hub.clone();
                async move |asked: NewSessionRequest, responder, _cx| {
                    let said = Said::of(asked.meta.as_ref()).map_err(invalid)?;
                    let thread = match said.thread {
                        Some(thread) => thread,
                        None => fresh().map_err(internal)?,
                    };
                    thread_of(&SessionId::new(thread.clone()))?;
                    let hub = hub.clone();
                    tokio::spawn(async move {
                        let opened = hub
                            .open(&thread)
                            .await
                            .map(|()| NewSessionResponse::new(SessionId::new(thread)));
                        let _ = responder.respond_with_result(opened.map_err(internal));
                    });
                    Ok(())
                }
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hub = hub.clone();
                async move |asked: ResumeSessionRequest, responder, _cx| {
                    let thread = thread_of(&asked.session_id)?;
                    let hub = hub.clone();
                    tokio::spawn(async move {
                        let opened = hub
                            .open(&thread)
                            .await
                            .map(|()| ResumeSessionResponse::new());
                        let _ = responder.respond_with_result(opened.map_err(internal));
                    });
                    Ok(())
                }
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hub = hub.clone();
                async move |asked: LoadSessionRequest, responder, cx| {
                    let thread = thread_of(&asked.session_id)?;
                    let hub = hub.clone();
                    tokio::spawn(async move {
                        let loaded = hub.load(&thread, asked.session_id, cx).await;
                        let _ = responder.respond_with_result(loaded.map_err(internal));
                    });
                    Ok(())
                }
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hub = hub.clone();
                async move |_asked: ListSessionsRequest, responder, _cx| {
                    let mut threads = hub.sessions.threads().map_err(internal)?;
                    if let Some(archive) = &hub.continuity {
                        threads.extend(
                            archive
                                .sessions()
                                .map_err(internal)?
                                .into_iter()
                                .map(|s| s.session_id.0.to_string()),
                        );
                        threads.sort();
                        threads.dedup();
                    }
                    let sessions = threads
                        .into_iter()
                        .map(|thread| SessionInfo::new(SessionId::new(thread), hub.cwd.clone()))
                        .collect();
                    responder.respond(ListSessionsResponse::new(sessions))
                }
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hub = hub.clone();
                async move |asked: PromptRequest, responder, cx| {
                    let thread = thread_of(&asked.session_id)?;
                    let said = Said::of(asked.meta.as_ref()).map_err(invalid)?;
                    let prompt = told(asked, &said);
                    // Its own task, never the connection's: a bridge gone
                    // ends the connection, and the turn runs on.
                    let hub = hub.clone();
                    tokio::spawn(async move {
                        let answered = hub.prompt(&thread, prompt, cx).await;
                        let _ = responder.respond_with_result(answered.map_err(|err| {
                            match err.downcast::<sdk::Error>() {
                                Ok(err) => err,
                                Err(err) => internal(err),
                            }
                        }));
                    });
                    Ok(())
                }
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hub = hub.clone();
                async move |_asked: Describe, responder, _cx| {
                    let hub = hub.clone();
                    tokio::spawn(async move {
                        let described = hub.describe().await;
                        let _ = responder.respond_with_result(described.map_err(internal));
                    });
                    Ok(())
                }
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hub = hub.clone();
                async move |asked: Export, responder, _cx| {
                    let hub = hub.clone();
                    tokio::spawn(async move {
                        let exported = hub.export(asked.kinds).await;
                        let _ = responder.respond_with_result(exported.map_err(internal));
                    });
                    Ok(())
                }
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hub = hub.clone();
                async move |asked: Import, responder, _cx| {
                    let hub = hub.clone();
                    tokio::spawn(async move {
                        let imported = hub.import(asked.state).await;
                        let _ = responder.respond_with_result(imported.map_err(internal));
                    });
                    Ok(())
                }
            },
            sdk::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let hub = hub.clone();
                async move |cancelled: CancelNotification, _cx| {
                    let thread = thread_of(&cancelled.session_id)?;
                    hub.cancel(&thread);
                    Ok(())
                }
            },
            sdk::on_receive_notification!(),
        )
        .connect_to(Lines::new(outgoing, incoming))
        .await;
    if let Err(err) = served {
        crate::warned(sentence!("An ACP connection ended."), &err);
    }
}

impl Hub {
    pub(crate) async fn up(&self) -> sdk::Result<Arc<Up>> {
        // Refuse damaged routing before a runtime can do startup recovery.
        self.sessions.check().map_err(internal)?;
        if let Some(archive) = &self.continuity {
            archive.check().map_err(internal)?;
        }
        self.harness
            .up_within(self.harness.calls)
            .await
            .map_err(internal)
    }

    /// `thread`'s session in `up`: the one it had, taken up again, else a
    /// new one, kept.
    async fn session(&self, up: &Up, thread: &str) -> anyhow::Result<String> {
        if let Some(id) = self.sessions.of(thread)? {
            if up.opened(&id) {
                return Ok(id);
            }
            let caps = &up.initialized.agent_capabilities;
            let session = SessionId::new(id.clone());
            let servers = self.tools.servers(caps);
            let taken_up = if caps.session_capabilities.resume.is_some() {
                let asked =
                    ResumeSessionRequest::new(session, self.cwd.clone()).mcp_servers(servers);
                up.cx.send_request(asked).block_task().await.map(drop)
            } else if caps.load_session {
                // Its replay goes nowhere: no route is set for it yet.
                let asked = LoadSessionRequest::new(session, self.cwd.clone()).mcp_servers(servers);
                up.cx.send_request(asked).block_task().await.map(drop)
            } else {
                Err(sdk::Error::method_not_found())
            };
            taken_up.map_err(|err| {
                anyhow::anyhow!(
                    "couldn't resume thread {thread}'s saved session {id}; its binding is preserved: {err}"
                )
            })?;
            up.open(&id);
            return Ok(id);
        }
        // Named for its thread, for a harness that reads Link's `_meta`.
        let named = Said {
            thread: Some(thread.to_string()),
            ..Said::default()
        };
        let caps = &up.initialized.agent_capabilities;
        if let Some(unserved) = self.tools.unserved(caps) {
            aside(Note(Sentence::adopt(&unserved)));
        }
        let asked = NewSessionRequest::new(self.cwd.clone())
            .mcp_servers(self.tools.servers(caps))
            .meta(named.into_meta(Meta::new()));
        let made = up.cx.send_request(asked).block_task().await?;
        let id = made.session_id.0.to_string();
        self.sessions.keep(thread, &id)?;
        up.open(&id);
        Ok(id)
    }

    /// A successful open acknowledges a persisted native binding, even
    /// when the owner has not sent the first prompt yet.
    async fn open(&self, thread: &str) -> anyhow::Result<()> {
        let _operation = self.activity.enter(thread).await;
        let up = self.up().await?;
        self.session(&up, thread).await?;
        Ok(())
    }

    /// One turn of `thread`, after any before it: `prompt` forwarded to
    /// its session, told taken as it goes, its updates sent to `to`.
    async fn prompt(
        &self,
        thread: &str,
        mut prompt: PromptRequest,
        to: ConnectionTo<sdk::Client>,
    ) -> anyhow::Result<agent_client_protocol::schema::v1::PromptResponse> {
        let _operation = self.activity.enter(thread).await;
        let said = Said::of(prompt.meta.as_ref())?;
        if said.from.is_none()
            && let [ContentBlock::Text(text)] = prompt.prompt.as_slice()
            && let Some((path, arguments)) =
                link_agent_contract::namespace::client::parsed(&self.tools.actions, &text.text)
        {
            use agent_client_protocol::schema::v1::{
                ContentChunk, PromptResponse, SessionNotification, SessionUpdate, StopReason,
            };
            anyhow::ensure!(
                self.tools.actions.actions.contains(&path),
                "action is outside the grant"
            );
            let _ = to.send_notification(Taken {
                session_id: prompt.session_id.clone(),
            });
            let answer = link_agent_contract::namespace::client::command(
                &self.tools.actions,
                &path,
                &arguments,
                thread,
            )
            .await?;
            let _ = to.send_notification(SessionNotification::new(
                prompt.session_id.clone(),
                SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                    TextContent::new(serde_json::to_string(&answer)?),
                ))),
            ));
            return Ok(PromptResponse::new(StopReason::EndTurn));
        }
        let up = self.up().await?;
        let prepared = self
            .continuity
            .as_ref()
            .map(|archive| {
                archive.prepare(
                    thread,
                    serde_json::to_vec(&prompt.prompt)
                        .map(|b| b.len())
                        .unwrap_or(usize::MAX),
                    self.sessions.of(thread)?.is_none(),
                )
            })
            .transpose()?
            .flatten();
        let session = self
            .session(&up, thread)
            .await
            .map_err(|err| anyhow::anyhow!("taking up thread {thread}'s session: {err:#}"))?;
        let named = prompt.session_id.clone();
        up.route(
            &session,
            Route {
                to: match &self.continuity {
                    Some(archive) => To::Archived(to.clone(), archive.clone(), thread.into()),
                    None => To::Bridge(to.clone()),
                },
                session: named.clone(),
            },
        );
        let names = crate::crossing::Names::link();
        if let [ContentBlock::Text(text)] = prompt.prompt.as_slice()
            && names.recognizes(&text.text)
        {
            // ACP may publish the list after session/new has answered.
            // Wait for this session's list, never another session's.
            let offered = up.commands(&session, self.harness.calls).await?;
            names.cross(&mut prompt, &offered);
        }
        if let Some(archive) = &self.continuity {
            archive.admit(thread, &prompt)?;
        }
        if let Some(prepared) = &prepared {
            prompt.prompt.insert(
                0,
                ContentBlock::Text(TextContent::new(prepared.text.clone())),
            );
            aside(Note(Sentence::adopt(&format!(
                "Continuing from portable history: {} updates supplied, {} omitted from context; the complete archive is retained.",
                prepared.selected.len(),
                prepared.omitted.len()
            ))));
        }
        prompt.session_id = SessionId::new(session);
        let _ = to.send_notification(Taken { session_id: named });
        let answer = up.cx.send_request(prompt).block_task().await?;
        if let (Some(archive), Some(prepared)) = (&self.continuity, prepared) {
            archive.supplied(thread, prepared)?;
        }
        Ok(answer)
    }

    /// `thread`'s history replayed to `to`, as the harness keeps it.
    async fn load(
        &self,
        thread: &str,
        named: SessionId,
        to: ConnectionTo<sdk::Client>,
    ) -> anyhow::Result<LoadSessionResponse> {
        let _operation = self.activity.enter(thread).await;
        if let Some(archive) = &self.continuity
            && let Some(session) = archive.session(thread)?
        {
            for update in session.updates {
                to.send_notification(agent_client_protocol::schema::v1::SessionNotification::new(
                    named.clone(),
                    update,
                ))?;
            }
            return Ok(LoadSessionResponse::new());
        }
        let up = self.up().await?;
        let Some(id) = self.sessions.of(thread)? else {
            return Ok(LoadSessionResponse::new());
        };
        if !up.initialized.agent_capabilities.load_session {
            return Ok(LoadSessionResponse::new());
        }
        let to = To::Bridge(to);
        let _routing = up.replay_route(&id, Route { to, session: named });
        let servers = self.tools.servers(&up.initialized.agent_capabilities);
        let asked = LoadSessionRequest::new(SessionId::new(id.clone()), self.cwd.clone())
            .mcp_servers(servers);
        let loaded = up.cx.send_request(asked).block_task().await?;
        up.open(&id);
        Ok(loaded)
    }

    pub(crate) fn cancel(&self, thread: &str) {
        let (Some(up), Ok(Some(id))) = (self.harness.running(), self.sessions.of(thread)) else {
            return;
        };
        if !up.opened(&id) {
            return;
        }
        let _ = up
            .cx
            .send_notification(CancelNotification::new(SessionId::new(id)));
    }
}

/// What it answers `initialize` with: ACP's first version, and the
/// harness's own capabilities, with sessions resumed and listed by
/// thread, as the hub does whatever the harness can, and the agent state
/// it carries.
fn initialized(up: &Up, state: Option<AgentState>) -> InitializeResponse {
    let mut capabilities = up.initialized.agent_capabilities.clone();
    if let Some(state) = state {
        let mut asp = Advertised::of(&capabilities);
        asp.agent_state = Some(state);
        capabilities = asp.on(capabilities);
    }
    capabilities.session_capabilities = SessionCapabilities::new()
        .list(SessionListCapabilities::new())
        .resume(SessionResumeCapabilities::new());
    let mut answer = InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(capabilities);
    answer.agent_info = up.initialized.agent_info.clone();
    answer
}

/// `asked`, its sender written into its text when it isn't the owner: a
/// line of its own saying who, each line of theirs quoted, so none
/// reads as the owner's.
fn told(mut asked: PromptRequest, said: &Said) -> PromptRequest {
    let Some(from) = &said.from else {
        return asked;
    };
    let from = match &said.from_name {
        Some(name) => format!("{name:?} ({from})"),
        None => from.clone(),
    };
    let hops = match said.hops {
        0 | 1 => String::new(),
        hops => format!(", {hops} messages on"),
    };
    let header = format!("(from {from}{hops}, not your owner:)");
    match asked.prompt.iter_mut().find_map(|block| match block {
        ContentBlock::Text(text) => Some(text),
        _ => None,
    }) {
        Some(text) => {
            let quoted: Vec<String> = text
                .text
                .lines()
                .map(|line| match line {
                    "" => ">".to_string(),
                    line => format!("> {line}"),
                })
                .collect();
            text.text = format!("{header}\n{}", quoted.join("\n"));
        }
        None => asked
            .prompt
            .insert(0, ContentBlock::Text(TextContent::new(header))),
    }
    asked
}

/// A session's thread, by its id: refused when it isn't a thread's name.
pub(crate) fn thread_of(session: &SessionId) -> sdk::Result<String> {
    let thread = &*session.0;
    let named = !thread.is_empty() && thread.len() <= 128 && link_format::clean(thread) == thread;
    match named {
        true => Ok(thread.to_string()),
        false => Err(invalid("that session isn't a thread's name")),
    }
}

/// A new conversation's thread, for a session nobody named.
pub(crate) fn fresh() -> anyhow::Result<String> {
    Ok(format!("acp:{}", link_id::candidate()?))
}

fn invalid(why: impl std::fmt::Display) -> sdk::Error {
    sdk::Error::invalid_params().data(format!("{why:#}"))
}

fn internal(why: impl std::fmt::Display) -> sdk::Error {
    sdk::Error::internal_error().data(format!("{why:#}"))
}

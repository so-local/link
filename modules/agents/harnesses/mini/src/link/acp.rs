//! ACP sessions and streamed prompts wrap the same core used by Mini.
//! The reference fixture can vary capabilities through Options; only that
//! fixture reads impersonation controls from the environment.

use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, AvailableCommand, AvailableCommandsUpdate, ContentBlock, ContentChunk,
    EmbeddedResourceResource, InitializeRequest, InitializeResponse, ListSessionsRequest,
    ListSessionsResponse, McpCapabilities, McpServer, NewSessionRequest, NewSessionResponse,
    PromptCapabilities, PromptRequest, PromptResponse, ResumeSessionRequest, ResumeSessionResponse,
    SessionCapabilities, SessionId, SessionInfo, SessionListCapabilities, SessionNotification,
    SessionResumeCapabilities, SessionUpdate, StopReason, TextContent,
};
use agent_client_protocol::{self as sdk, ConnectionTo, Lines};
use futures_util::SinkExt;
use link_connection_contract::acp::Said;
use link_harness_contract::asp::methods::{Describe, Export, Exported, Import};
use link_harness_contract::harness::Model;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

use super::agent::Harness;

pub(crate) struct Options {
    pub model: Model,
    pub model_facts: Option<link_model_contract::Known>,
    /// The window a turn fills, as the agent's spec says, if it says.
    pub window: Option<u32>,
    pub instructions: Option<std::path::PathBuf>,
    pub tools_from_env: bool,
    pub commands: Vec<String>,
    pub state: bool,
    pub media_descriptions: bool,
}

impl Options {
    pub(crate) fn new(model: Model) -> Self {
        Self {
            model,
            model_facts: None,
            window: None,
            instructions: None,
            tools_from_env: false,
            commands: Vec::new(),
            state: true,
            media_descriptions: false,
        }
    }
}

/// Commands the reference fixture asks it to advertise.
fn offered(session: &SessionId, names: &[String]) -> Option<SessionNotification> {
    if names.is_empty() {
        return None;
    }
    let commands = names
        .iter()
        .map(|n| AvailableCommand::new(n.clone(), n.clone()))
        .collect();
    Some(SessionNotification::new(
        session.clone(),
        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands)),
    ))
}

pub(crate) fn serve(options: Options) -> anyhow::Result<()> {
    link_fetch::install_crypto_provider();
    let harness = Arc::new(Harness::new(options.tools_from_env));
    let options = Arc::new(options);
    tokio::runtime::Runtime::new()?.block_on(async move {
        let incoming = futures_util::stream::unfold(
            BufReader::new(tokio::io::stdin()).lines(),
            async |mut lines| match lines.next_line().await {
                Ok(Some(line)) => Some((Ok(line), lines)),
                Ok(None) => None,
                Err(err) => Some((Err(err), lines)),
            },
        );
        let outgoing = futures_util::sink::unfold(
            tokio::fs::File::from_std(link_format::streams::Protocol::file()?),
            async |mut out, line: String| {
                use tokio::io::AsyncWriteExt;
                out.write_all(format!("{line}\n").as_bytes()).await?;
                out.flush().await?;
                Ok::<_, std::io::Error>(out)
            },
        );
        sdk::Agent
            .builder()
            .name("lnk-mini-harness")
            .on_receive_request(
                {
                    let options = options.clone();
                    async move |_asked: InitializeRequest, responder, _cx| {
                        responder.respond(initialized(&options))
                    }
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let harness = harness.clone();
                    let options = options.clone();
                    async move |asked: NewSessionRequest, responder, cx| {
                        let said = Said::of(asked.meta.as_ref()).map_err(invalid)?;
                        let thread = said.thread.unwrap_or_else(|| "default".into());
                        super::threads::check(&thread).map_err(invalid)?;
                        harness.given(&thread, &asked.mcp_servers);
                        let session = SessionId::new(thread);
                        if let Some(offered) = offered(&session, &options.commands) {
                            cx.send_notification(offered)?;
                        }
                        responder.respond(NewSessionResponse::new(session))
                    }
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let harness = harness.clone();
                    async move |asked: ResumeSessionRequest, responder, _cx| {
                        super::threads::check(&asked.session_id.0).map_err(invalid)?;
                        harness.given(&asked.session_id.0, &asked.mcp_servers);
                        responder.respond(ResumeSessionResponse::new())
                    }
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async |_asked: ListSessionsRequest, responder, _cx| {
                    let at = std::env::current_dir()
                        .map(|dir| dir.display().to_string())
                        .unwrap_or_default();
                    let sessions = super::threads::names()
                        .into_iter()
                        .map(|thread| SessionInfo::new(SessionId::new(thread), at.clone()))
                        .collect();
                    responder.respond(ListSessionsResponse::new(sessions))
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async |_asked: Describe, responder, _cx| {
                    responder.respond(super::state::describe())
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async |asked: Export, responder, _cx| {
                    responder.respond(Exported {
                        state: super::state::export(&asked.kinds).map_err(invalid)?,
                    })
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async |asked: Import, responder, _cx| {
                    responder.respond(super::state::import(asked.state).map_err(invalid)?)
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async move |asked: PromptRequest, responder, cx: ConnectionTo<sdk::Client>| {
                    let said = Said::of(asked.meta.as_ref()).map_err(invalid)?;
                    let thread = asked.session_id.0.to_string();
                    super::threads::check(&thread).map_err(invalid)?;
                    let message =
                        message(&asked.prompt, options.media_descriptions).map_err(invalid)?;
                    let (harness, options) = (harness.clone(), options.clone());
                    tokio::spawn(async move {
                        let (pieces, mut received) = mpsc::channel::<String>(64);
                        let session = asked.session_id;
                        let streamed = tokio::spawn(async move {
                            while let Some(piece) = received.recv().await {
                                let chunk =
                                    ContentChunk::new(ContentBlock::Text(TextContent::new(piece)));
                                let _ = cx.send_notification(SessionNotification::new(
                                    session.clone(),
                                    SessionUpdate::AgentMessageChunk(chunk),
                                ));
                            }
                        });
                        let turn = harness
                            .turn(&options, &thread, &message, said, pieces)
                            .await;
                        let _ = streamed.await;
                        let _ = responder.respond_with_result(match turn {
                            Ok(()) => Ok(PromptResponse::new(StopReason::EndTurn)),
                            Err(err) => Err(sdk::Error::internal_error().data(format!("{err:#}"))),
                        });
                    });
                    Ok(())
                },
                sdk::on_receive_request!(),
            )
            .connect_to(Lines::new(
                Box::pin(outgoing.sink_map_err(std::io::Error::other)),
                Box::pin(incoming),
            ))
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))
    })
}

fn initialized(options: &Options) -> InitializeResponse {
    let capabilities = AgentCapabilities::new();
    let capabilities = if !options.state {
        capabilities
    } else {
        super::state::kinds().advertised(capabilities)
    };
    InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(
        capabilities
            .prompt_capabilities(
                PromptCapabilities::new()
                    .embedded_context(true)
                    .image(options.media_descriptions),
            )
            .mcp_capabilities(McpCapabilities::new().http(!options.tools_from_env))
            .session_capabilities(
                SessionCapabilities::new()
                    .list(SessionListCapabilities::new())
                    .resume(SessionResumeCapabilities::new()),
            ),
    )
}

/// A prompt's message: its text, each image or document a line saying
/// what it is (`[image/png, 67 bytes]`, `[a.pdf: application/pdf, 9
/// bytes]`).
fn message(blocks: &[ContentBlock], media_descriptions: bool) -> anyhow::Result<String> {
    if !media_descriptions && blocks.iter().any(|block| matches!(block, ContentBlock::Image(_)) || matches!(block, ContentBlock::Resource(r) if matches!(r.resource, EmbeddedResourceResource::BlobResourceContents(_)))) {
        anyhow::bail!("Mini Harness reads text only");
    }
    Ok(blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.clone()),
            ContentBlock::Image(image) => Some(said(None, &image.mime_type, &image.data)),
            ContentBlock::Resource(resource) => match &resource.resource {
                EmbeddedResourceResource::BlobResourceContents(blob) => Some(said(
                    blob.uri.rsplit('/').next(),
                    blob.mime_type
                        .as_deref()
                        .unwrap_or("application/octet-stream"),
                    &blob.blob,
                )),
                EmbeddedResourceResource::TextResourceContents(text) => Some(text.text.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// A part in base64 as a line: its name, media type and size.
fn said(name: Option<&str>, media: &str, data: &str) -> String {
    let padding = data.bytes().rev().take_while(|b| *b == b'=').count();
    let bytes = data.len() / 4 * 3 - padding;
    match name {
        Some(name) => format!("[{name}: {media}, {bytes} bytes]"),
        None => format!("[{media}, {bytes} bytes]"),
    }
}

/// Each HTTP server of `servers`, by name, at its address.
pub(crate) fn http(servers: &[McpServer]) -> Vec<(String, String)> {
    servers
        .iter()
        .filter_map(|server| match server {
            McpServer::Http(http) => Some((http.name.clone(), http.url.clone())),
            _ => None,
        })
        .collect()
}

fn invalid(why: impl std::fmt::Display) -> sdk::Error {
    sdk::Error::invalid_params().data(format!("{why:#}"))
}

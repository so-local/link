//! `session/prompt`: a message of the owner's, or of whoever Link's
//! `_meta` names, answered as a turn, its text streamed in
//! `agent_message_chunk`s and its calls beside it, and its end the
//! prompt's answer.

use std::sync::Arc;

use link_agent_state_protocol::acp::schema::v1::{
    AudioContent, ContentBlock, ContentChunk, EmbeddedResourceResource, PromptRequest,
    PromptResponse, SessionUpdate, StopReason, TextContent,
};
use link_agent_state_protocol::acp::{self, Client, ConnectionTo, Responder};
use link_connection_contract::acp::{Meta, Said};
use tokio::sync::broadcast::error::RecvError;

use super::{Connection, history, internal, invalid, update};
use crate::event::{Media, PARTS_MAX};
use crate::server::turning::{self, Started};
use crate::turn::Asked;

/// Takes `asked`, refused before anything of it is kept when it isn't
/// a message this connection's agent takes, and answers it once its
/// turn ends, the connection free for others meanwhile.
pub(super) fn asked(
    connection: Arc<Connection>,
    asked: PromptRequest,
    responder: Responder<PromptResponse>,
    cx: ConnectionTo<Client>,
) -> acp::Result<()> {
    let thread = connection.opened(&asked.session_id)?;
    let said = Said::of(asked.meta.as_ref()).map_err(invalid)?;
    let busy = busy(&connection, asked.meta.as_ref())?;
    let said_from = said.from.clone();
    let (text, media) = message(asked.prompt).map_err(invalid)?;
    let (server, agent) = (&connection.server, &connection.agent);
    // Fewer tools from now on (a subagent's): kept with the conversation
    // before its turn, which offers only those.
    if let Some(tools) = &said.tools {
        server
            .harness
            .narrow_tools(agent, &thread, tools)
            .map_err(internal)?;
    }
    // Its model from now on (a subagent's), kept as its conversation's.
    if let Some(model) = &said.model {
        server
            .harness
            .choose_model(agent, &thread, model, said.from.as_deref())
            .map_err(internal)?;
    }
    let session = asked.session_id;
    let streams = cx;
    // Its own task, never the connection's: a client gone ends the
    // connection, and the turn's answer is kept for its bridge then.
    let answering = connection.server.harness.answering();
    tokio::spawn(async move {
        // Counted until it's answered: a harness stopping waits for it.
        let _answering = answering;
        let (server, agent) = (&connection.server, &connection.agent);
        // Its voice notes heard first: their words are its text.
        let message = turning::message(
            server,
            agent,
            &thread,
            text,
            media,
            said.from,
            said.from_name,
            said.hops,
        )
        .await;
        let message = match message {
            Ok(mut message) => {
                message.occurrence = said.occurrence;
                message.standing = said.standing;
                message.parent = said.parent;
                message.delivery = said.delivery;
                message
            }
            Err(err) => return responder.respond_with_error(internal(err)),
        };
        // Watched before it starts, so no call is missed.
        let mut written = server.harness.watch();
        let (mut reply, untaken) = match turning::start(server, agent, &thread, message, busy).await
        {
            Started::Steered => {
                let steered = Said {
                    steered: true,
                    ..Said::default()
                };
                return responder.respond(
                    PromptResponse::new(StopReason::EndTurn).meta(steered.into_meta(Meta::new())),
                );
            }
            Started::Running(reply, untaken) => (reply, untaken),
        };
        // Taken: its asker needn't wait for the answer to know.
        let _ = streams.send_notification(link_connection_contract::acp::Taken {
            session_id: session.clone(),
        });
        // Whether the client is still there to be told, and its chat
        // shows each call, as the conversation chose.
        let mut there = true;
        let verbose = server
            .harness
            .conversing(agent, &thread)
            .ok()
            .and_then(|config| config.verbose);
        let mut tell = |said: SessionUpdate| {
            let summary = match &said {
                SessionUpdate::ToolCall(call) => call
                    .name
                    .as_deref()
                    .and_then(|name| server.harness.summary(agent, name)),
                _ => None,
            };
            let said = history::shown(history::titled(said, summary), verbose);
            there &= update(&streams, &session, said).is_ok()
        };
        loop {
            tokio::select! {
                piece = reply.next() => match piece {
                    Some(piece) => tell(SessionUpdate::AgentMessageChunk(
                        ContentChunk::new(ContentBlock::Text(TextContent::new(piece))),
                    )),
                    None => break,
                },
                wrote = written.recv() => match wrote {
                    Ok(wrote) if wrote.agent == *agent && wrote.thread == thread => {
                        history::beside(&wrote.event).into_iter().for_each(&mut tell);
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => {}
                },
            }
        }
        // The calls written as its last piece came.
        while let Ok(wrote) = written.try_recv() {
            if wrote.agent == *agent && wrote.thread == thread {
                history::beside(&wrote.event)
                    .into_iter()
                    .for_each(&mut tell);
            }
        }
        let ended = reply.end().await;
        let answered = match &ended {
            Ok(Ok(Asked::Answered(answer))) => {
                let stop = match answer.interrupted {
                    true => StopReason::Cancelled,
                    false => StopReason::EndTurn,
                };
                let receipt = if said.receipts && !answer.interrupted {
                    server.harness.untaken(agent, &thread, answer.seq);
                    Some(format!("{}/{}", thread.name(), answer.seq))
                } else {
                    None
                };
                responder.respond(
                    PromptResponse::new(stop).meta(
                        Said {
                            result_receipt: receipt,
                            ..Said::default()
                        }
                        .into_meta(Meta::new()),
                    ),
                )
            }
            Ok(Ok(Asked::Steered)) => responder.respond(PromptResponse::new(StopReason::EndTurn)),
            Ok(Err(err)) => responder.respond_with_error(internal(err)),
            Err(err) => responder.respond_with_error(internal(err)),
        };
        if !there || answered.is_err() {
            untaken.keep(&ended);
        }
        // An owner's turn ended: its chat's goal, if it has one, is
        // pursued on.
        if said_from.is_none() && matches!(&ended, Ok(Ok(Asked::Answered(_)))) {
            tokio::spawn(turning::pursue(
                connection.server.clone(),
                connection.agent.clone(),
                thread.clone(),
            ));
        }
        acp::Result::Ok(())
    });
    Ok(())
}

/// What a prompt's `_meta.asp.busy` asks it to do while another turn of
/// its session runs, refused unless the agent takes it
/// ([`crate::turn::takes`]): none, as its spec says.
fn busy(
    connection: &Connection,
    meta: Option<&link_agent_state_protocol::acp::schema::v1::Meta>,
) -> acp::Result<Option<link_agent_spec::Busy>> {
    use link_agent_spec::Busy;
    let Some(asked) = link_agent_state_protocol::steering::asked(meta).map_err(invalid)? else {
        return Ok(None);
    };
    let busy = match asked {
        link_agent_state_protocol::Busy::Steer => Busy::Steer,
        link_agent_state_protocol::Busy::Queue => Busy::Queue,
        link_agent_state_protocol::Busy::Interrupt => Busy::Interrupt,
    };
    let config = connection
        .server
        .harness
        .config(&connection.agent)
        .map_err(internal)?;
    match crate::turn::takes(&config).contains(&busy) {
        true => Ok(Some(busy)),
        false => Err(invalid(format!(
            "this agent takes no {asked:?} for a prompt sent while it answers"
        ))),
    }
}

/// A prompt's text, and its images, documents and voice notes: text
/// blocks, images, audio, and embedded resources, a blob's in base64 or
/// a text's as it is. A link to a resource isn't taken: refused, never
/// dropped unsaid.
pub(super) fn message(prompt: Vec<ContentBlock>) -> Result<(String, Vec<Media>), String> {
    let (mut texts, mut media) = (Vec::new(), Vec::new());
    for block in prompt {
        match block {
            ContentBlock::Text(text) => texts.push(text.text),
            ContentBlock::Image(image) => {
                let bytes =
                    crate::event::decode(&image.data).map_err(|why| format!("an image: {why}"))?;
                media.push(Media::new(&image.mime_type, None, bytes)?);
            }
            ContentBlock::Audio(AudioContent {
                data, mime_type, ..
            }) => {
                let bytes =
                    crate::event::decode(&data).map_err(|why| format!("a voice note: {why}"))?;
                media.push(Media::new(&mime_type, None, bytes)?);
            }
            ContentBlock::Resource(resource) => match resource.resource {
                EmbeddedResourceResource::BlobResourceContents(blob) => {
                    let name = named(&blob.uri);
                    let bytes =
                        crate::event::decode(&blob.blob).map_err(|why| format!("{name}: {why}"))?;
                    let kind = blob
                        .mime_type
                        .as_deref()
                        .unwrap_or("application/octet-stream");
                    media.push(Media::new(kind, Some(name), bytes)?);
                }
                EmbeddedResourceResource::TextResourceContents(text) => {
                    let kind = text.mime_type.as_deref().unwrap_or("text/plain");
                    media.push(Media::new(
                        kind,
                        Some(named(&text.uri)),
                        text.text.into_bytes(),
                    )?);
                }
                _ => return Err("a resource of that kind isn't taken".into()),
            },
            ContentBlock::ResourceLink(_) => {
                return Err("a resource must be sent embedded, not as a link to it".into());
            }
            _ => return Err("only text, images, audio and embedded resources are taken".into()),
        }
    }
    if media.len() > PARTS_MAX {
        return Err(format!(
            "a message holds at most {PARTS_MAX} images and documents"
        ));
    }
    let text = texts.join("\n");
    if text.len() > crate::server::TEXT_MAX {
        return Err(format!(
            "a message's text is at most {} MiB",
            crate::server::TEXT_MAX >> 20
        ));
    }
    if text.trim().is_empty() && media.is_empty() {
        return Err("no message".into());
    }
    Ok((text, media))
}

/// A resource's name: the last part of its URI's path.
fn named(uri: &str) -> &str {
    uri.rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(uri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_agent_state_protocol::acp::schema::v1::{
        BlobResourceContents, EmbeddedResource, ImageContent, ResourceLink,
    };

    #[test]
    fn a_prompt_is_its_text_and_media_or_refused() {
        let encoded =
            |kind: &str, bytes: &[u8]| Media::new(kind, None, bytes.to_vec()).unwrap().encoded();
        let png = encoded("image/png", &[0x89, b'P', b'N', b'G', 13, 10, 26, 10]);
        let (text, media) = message(vec![
            ContentBlock::Text(TextContent::new("look")),
            ContentBlock::Image(ImageContent::new(png.clone(), "image/png")),
        ])
        .unwrap();
        assert_eq!((text.as_str(), media.len()), ("look", 1));
        let pdf = ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::BlobResourceContents(
                BlobResourceContents::new(
                    encoded("application/pdf", b"%PDF-1.4\n"),
                    "file:///a/b.pdf",
                )
                .mime_type("application/pdf"),
            ),
        ));
        let (_, media) = message(vec![pdf]).unwrap();
        assert_eq!(media[0].named(), "[document: b.pdf]");
        for refused in [
            vec![],
            vec![ContentBlock::Text(TextContent::new("  "))],
            vec![ContentBlock::ResourceLink(ResourceLink::new(
                "a",
                "file:///a",
            ))],
        ] {
            assert!(message(refused).is_err());
        }
    }
}

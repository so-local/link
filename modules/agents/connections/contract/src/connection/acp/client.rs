//! A bridge's side of ACP: a message asked of the harness on a
//! connection of its own, the session its thread, answered as the turn
//! streams; and the answers a crash left, asked for as
//! [`super::RECOVER`].

use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AudioContent, BlobResourceContents, ContentBlock, EmbeddedResource, EmbeddedResourceResource,
    ImageContent, InitializeRequest, PromptRequest, ResumeSessionRequest, SessionId,
    SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{self as sdk, ConnectionTo, Lines};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

use super::{Meta, RecoveredAnswers, Said, Socket, Taken};
use crate::Recover;
use crate::policy::Activity;
use crate::{Bridge, Part, Speaker};

/// What a turn streams to its asker.
pub(in crate::connection) enum Streamed {
    /// A piece of its answer.
    Piece(String),
    /// A tool it called, told only where its activity shows.
    Called(Call),
    /// Anything else of it: a call, its end.
    Moved,
}

/// A tool call as a line shows it: the call (`run command`), and what
/// it works on, where its harness said (`git commit -m 'a fix'`).
#[derive(Clone, Debug, PartialEq)]
pub(in crate::connection) struct Call {
    pub call: String,
    pub detail: Option<String>,
}

impl Call {
    /// A call by its ACP `name` and `title`: with a name, the call is its
    /// words ([`crate::policy::call_words`]) and the detail what the title
    /// says after them, as Link Harness titles a call; with none, or a
    /// title of its own, the title is the call.
    pub fn of(name: Option<&str>, title: &str) -> Self {
        let Some(words) = name.map(crate::policy::call_words) else {
            return Self {
                call: title.to_string(),
                detail: None,
            };
        };
        match title.strip_prefix(&words) {
            Some(rest) if rest.is_empty() || rest.starts_with(' ') => Self {
                call: words,
                detail: Some(rest.trim().to_string()).filter(|d| !d.is_empty()),
            },
            _ => Self {
                call: title.to_string(),
                detail: None,
            },
        }
    }
}

/// How a call's line looks, as the bridge's spec says: what leads it
/// and what stands between the call and its detail. Its spec saying
/// neither, nothing leads it and a space stands between.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::connection) struct Look {
    pub mark: String,
    pub separator: String,
}

impl Look {
    pub fn of(spec: Option<&crate::policy::Connection>) -> Self {
        Self {
            mark: spec
                .and_then(|spec| spec.activity_mark.clone())
                .unwrap_or_default(),
            separator: spec
                .and_then(|spec| spec.activity_separator.clone())
                .unwrap_or_else(|| " ".into()),
        }
    }
}

/// How a turn ended, once it answered its prompt.
pub(in crate::connection) struct Ended {
    pub steered: bool,
    pub receipt: Option<String>,
}

/// A message the harness has taken, its answer still streaming. Dropped,
/// its connection closes: the harness keeps the answer nobody took.
pub(in crate::connection) struct Taking {
    pub streamed: mpsc::UnboundedReceiver<Streamed>,
    /// Whether its calls stay above its finished answer
    /// ([`Activity::Keep`]).
    pub keep: bool,
    /// How each call's line looks.
    pub look: Look,
    pub ended: oneshot::Receiver<anyhow::Result<Ended>>,
    _connection: Closing,
}

/// Ends a connection's task when dropped.
struct Closing(tokio::task::AbortHandle);

impl Drop for Closing {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Asks `text` and `parts` on `thread`, said as `from` with `tools`,
/// once the harness has taken it: refused, and why, when it isn't.
pub(in crate::connection) async fn asked(
    bridge: &Bridge,
    thread: &str,
    text: &str,
    parts: &[Part],
    sent: crate::connection::endpoint::Sent<'_>,
) -> anyhow::Result<Taking> {
    let blocks = blocks(text, parts)?;
    let mut said = said(sent.from, sent.tools)?;
    said.standing = sent.standing;
    said.occurrence = sent.occurrence.cloned();
    said.parent = sent.parent.map(str::to_string);
    said.delivery = sent.delivery.cloned();
    said.receipts = sent.receipts;
    if let Some(model) = sent.model {
        anyhow::ensure!(
            crate::is_model_name(model),
            "{model:?} isn't a model's name"
        );
        said.model = Some(model.into());
    }
    let activity = bridge
        .spec
        .as_ref()
        .and_then(|spec| spec.activity)
        .unwrap_or(Activity::Off);
    let session = SessionId::new(match thread {
        "" => super::DEFAULT_THREAD,
        thread => thread,
    });
    // After the conversations a switch brings, so it goes on in them.
    crate::connection::asking::after_arriving(&bridge.agent).await?;
    let socket = super::super::endpoint::upgraded(bridge).await?;
    let (moved, streamed) = mpsc::unbounded_channel();
    let (taken, taking) = oneshot::channel::<anyhow::Result<()>>();
    let (end, ended) = oneshot::channel();
    let taken = Arc::new(Mutex::new(Some(taken)));
    let tell = taken.clone();
    let prompt = PromptRequest::new(session.clone(), blocks).meta(said.into_meta(Meta::new()));
    let connection = tokio::spawn(async move {
        let answered = sdk::Client
            .builder()
            .on_receive_notification(
                async move |said: SessionNotification, _cx| {
                    let _ = moved.send(match said.update {
                        SessionUpdate::AgentMessageChunk(chunk) => match chunk.content {
                            ContentBlock::Text(text) => Streamed::Piece(text.text),
                            _ => Streamed::Moved,
                        },
                        // As its conversation chose, else as the spec says.
                        SessionUpdate::ToolCall(call)
                            if Said::of(call.meta.as_ref())
                                .ok()
                                .and_then(|said| said.activity)
                                .unwrap_or(activity.shows()) =>
                        {
                            Streamed::Called(Call::of(call.name.as_deref(), &call.title))
                        }
                        _ => Streamed::Moved,
                    });
                    Ok(())
                },
                sdk::on_receive_notification!(),
            )
            .on_receive_notification(
                async move |_taken: Taken, _cx| {
                    if let Some(taken) = tell.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        let _ = taken.send(Ok(()));
                    }
                    Ok(())
                },
                sdk::on_receive_notification!(),
            )
            .connect_with(lines(socket), async |cx: ConnectionTo<sdk::Agent>| {
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                cx.send_request(ResumeSessionRequest::new(session, "/"))
                    .block_task()
                    .await?;
                cx.send_request(prompt).block_task().await
            })
            .await;
        let ended = answered.map_err(refused).and_then(|answered| {
            let said = Said::of(answered.meta.as_ref())?;
            Ok(Ended {
                steered: said.steered,
                receipt: said.result_receipt,
            })
        });
        // Ended before it was taken: steered, a command, or refused.
        if let Some(taken) = taken.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = taken.send(match &ended {
                Ok(_) => Ok(()),
                Err(why) => Err(anyhow::anyhow!("{why:#}")),
            });
        }
        let _ = end.send(ended);
    });
    let closing = Closing(connection.abort_handle());
    match taking.await {
        Ok(Ok(())) => Ok(Taking {
            streamed,
            keep: activity == Activity::Keep,
            look: Look::of(bridge.spec.as_ref()),
            ended,
            _connection: closing,
        }),
        Ok(Err(why)) => Err(why),
        Err(_) => anyhow::bail!("the agent's connection ended before it took the message"),
    }
}

/// The answers a crash left, or forgets those `asked` names, as
/// [`super::RECOVER`] answers.
pub(in crate::connection) async fn recover(
    bridge: &Bridge,
    asked: Recover,
) -> anyhow::Result<RecoveredAnswers> {
    let socket = super::super::endpoint::upgraded(bridge).await?;
    sdk::Client
        .builder()
        .connect_with(lines(socket), async |cx: ConnectionTo<sdk::Agent>| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            cx.send_request(asked).block_task().await
        })
        .await
        .map_err(refused)
}

/// The agent's state of `kinds`, in ASP's portable forms, exported by
/// its harness over [`link_agent_state_protocol::methods::Export`], with what its ASP
/// says it takes in: refused by a harness that speaks no agent state.
pub async fn exported(
    bridge: &Bridge,
    kinds: Vec<link_agent_state_protocol::Kind>,
) -> anyhow::Result<(
    link_agent_state_protocol::AgentState,
    link_agent_state_protocol::State,
)> {
    let socket = super::super::endpoint::upgraded(bridge).await?;
    sdk::Client
        .builder()
        .connect_with(lines(socket), async |cx: ConnectionTo<sdk::Agent>| {
            let up = cx
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let Some(speaks) = link_agent_state_protocol::AgentState::of(&up.agent_capabilities)
            else {
                return Err(
                    sdk::Error::method_not_found().data("this harness speaks no agent state")
                );
            };
            let asked = link_agent_state_protocol::methods::Export { kinds };
            let exported = cx.send_request(asked).block_task().await?;
            Ok((speaks, exported.state))
        })
        .await
        .map_err(refused)
}

/// `state` imported by the agent's harness over
/// [`link_agent_state_protocol::methods::Import`]: what it took, what it couldn't and
/// why, and each session's thread by its id in `state`.
pub async fn imported(
    bridge: &Bridge,
    state: link_agent_state_protocol::State,
) -> anyhow::Result<link_agent_state_protocol::methods::Imported> {
    let socket = super::super::endpoint::upgraded(bridge).await?;
    sdk::Client
        .builder()
        .connect_with(lines(socket), async |cx: ConnectionTo<sdk::Agent>| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let asked = link_agent_state_protocol::methods::Import { state };
            cx.send_request(asked).block_task().await
        })
        .await
        .map_err(refused)
}

/// ACP's lines over `socket`, a text frame each.
fn lines(socket: Socket) -> impl sdk::ConnectTo<sdk::Client> {
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
    Lines::new(outgoing, incoming)
}

/// What the harness refused, in its words.
fn refused(err: sdk::Error) -> anyhow::Error {
    let why = match &err.data {
        Some(serde_json::Value::String(why)) => why.clone(),
        Some(data) => data.to_string(),
        None => err.message.clone(),
    };
    anyhow::anyhow!("the agent answered: {}", link_format::clean(&why))
}

/// A message's blocks: its text, then each part, an image's as an
/// image, a voice note's as audio, a document's as an embedded
/// resource, in base64.
fn blocks(text: &str, parts: &[Part]) -> anyhow::Result<Vec<ContentBlock>> {
    use base64::Engine;
    anyhow::ensure!(
        parts.len() <= crate::PARTS_MAX,
        "a message holds at most {} images and documents",
        crate::PARTS_MAX
    );
    let mut blocks = Vec::with_capacity(parts.len() + 1);
    if !text.is_empty() || parts.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(text)));
    }
    for part in parts {
        let data = base64::engine::general_purpose::STANDARD.encode(&part.bytes);
        blocks.push(match part.media.split('/').next() {
            Some("image") => ContentBlock::Image(ImageContent::new(data, part.media.clone())),
            Some("audio") => ContentBlock::Audio(AudioContent::new(data, part.media.clone())),
            _ => {
                let name = part.name.as_deref().unwrap_or("document");
                ContentBlock::Resource(EmbeddedResource::new(
                    EmbeddedResourceResource::BlobResourceContents(
                        BlobResourceContents::new(data, format!("file:///{name}"))
                            .mime_type(part.media.clone()),
                    ),
                ))
            }
        });
    }
    Ok(blocks)
}

/// What Link says beside a message: who it's from, and the tools its
/// conversation narrows to, checked before anything is sent.
fn said(from: Option<Speaker<'_>>, tools: Option<&[String]>) -> anyhow::Result<Said> {
    if let Some(tools) = tools {
        crate::tools_header(tools)?;
    }
    let mut said = Said {
        tools: tools.map(<[String]>::to_vec),
        ..Said::default()
    };
    if let Some(Speaker { from, name, hops }) = from {
        anyhow::ensure!(
            crate::is_from(from),
            "{from:?} isn't who a message may be from"
        );
        said.from = Some(from.into());
        said.hops = hops;
        said.from_name = name
            .filter(|name| {
                from.parse::<crate::access::ChatMember>().is_ok() && crate::is_person_name(name)
            })
            .map(str::to_string);
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_its_text_then_its_parts() {
        let parts = [
            Part {
                media: "image/png".into(),
                name: None,
                bytes: vec![1],
            },
            Part {
                media: "application/pdf".into(),
                name: Some("a.pdf".into()),
                bytes: vec![2],
            },
        ];
        let wire = serde_json::to_value(blocks("look", &parts).unwrap()).unwrap();
        assert_eq!(wire[0], serde_json::json!({"type": "text", "text": "look"}));
        assert_eq!(wire[1]["type"], "image");
        assert_eq!(wire[1]["mimeType"], "image/png");
        assert_eq!(wire[2]["resource"]["uri"], "file:///a.pdf");
        assert_eq!(wire[2]["resource"]["mimeType"], "application/pdf");
        // A photo needs no caption.
        assert_eq!(blocks("", &parts[..1]).unwrap().len(), 1);
    }

    #[test]
    fn who_said_it_is_checked_before_it_is_sent() {
        let person = Speaker {
            from: "member:telegram:42",
            name: Some("Zoë"),
            hops: 1,
        };
        let said = said(Some(person), Some(&["memory__search".into()])).unwrap();
        assert_eq!(said.from.as_deref(), Some("member:telegram:42"));
        assert_eq!(said.from_name.as_deref(), Some("Zoë"));
        let nobody = Speaker {
            from: "nobody",
            name: None,
            hops: 0,
        };
        assert!(super::said(Some(nobody), None).is_err());
        assert!(super::said(None, Some(&["not a tool".into()])).is_err());
    }
}

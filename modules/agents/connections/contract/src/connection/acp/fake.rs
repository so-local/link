//! A harness's ACP, faked for a bridge's tests: on a port of its own,
//! each prompt kept with what Link said beside it, told taken, then
//! answered as its script says, a piece at a time, or joined to a turn
//! already running; the answers a crash left given when asked for.

use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, InitializeRequest, InitializeResponse, PromptRequest,
    PromptResponse, ResumeSessionRequest, ResumeSessionResponse, SessionNotification,
    SessionUpdate, StopReason, TextContent, ToolCall, ToolCallId,
};
use agent_client_protocol::{self as sdk, Lines};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use super::{RecoveredAnswers, Said, Taken};
use crate::{Recover, Recovered};

/// A prompt the fake was asked: its thread, what Link said beside it,
/// and its text.
#[derive(Clone, Debug)]
pub struct Prompted {
    pub thread: String,
    pub said: Said,
    pub text: String,
}

/// A fake harness, serving until its runtime ends.
pub struct Fake {
    /// Its endpoint, as a bridge is given it.
    pub endpoint: String,
    /// Each prompt, as it came.
    pub asked: Arc<Mutex<Vec<Prompted>>>,
    /// Each time it was asked for the answers a crash left.
    pub recovering: Arc<Mutex<Vec<Recover>>>,
}

/// How a prompt's turn goes.
pub enum Turn {
    /// Taken, then answered with each piece as it comes, ended when
    /// they end.
    Answer(futures_util::stream::BoxStream<'static, String>),
    /// Taken, each tool call begun with its name and title, then
    /// answered as [`Turn::Answer`].
    Working(
        Vec<(String, String)>,
        futures_util::stream::BoxStream<'static, String>,
    ),
    /// Joined to a turn already running: answered at once, steered.
    Steered,
}

/// What the fake was asked, as it comes.
pub enum Seen<'a> {
    Prompted(&'a Prompted),
    Recover(&'a Recover),
}

type Answer = Arc<dyn Fn(&Prompted) -> Turn + Send + Sync>;
type Recovering = Arc<dyn Fn(&Recover) -> Vec<Recovered> + Send + Sync>;
type See = Arc<dyn Fn(Seen<'_>) + Send + Sync>;

/// A fake harness on a port of its own, answering each prompt
/// `answer(text)`, and the answers a crash left `recovered`, when asked
/// in two phases. A look whether it's up is answered.
pub async fn serve(
    answer: impl Fn(&str) -> String + Send + Sync + 'static,
    recovered: Vec<Recovered>,
) -> Fake {
    serve_seen(answer, recovered, |_| {}).await
}

/// [`serve`], each thing asked told `see` as it comes too.
pub async fn serve_seen(
    answer: impl Fn(&str) -> String + Send + Sync + 'static,
    recovered: Vec<Recovered>,
    see: impl Fn(Seen<'_>) + Send + Sync + 'static,
) -> Fake {
    serve_script(
        move |asked| {
            Turn::Answer(Box::pin(futures_util::stream::once(std::future::ready(
                answer(&asked.text),
            ))))
        },
        move |asked| match asked.two_phase {
            true => recovered.clone(),
            false => Vec::new(),
        },
        see,
    )
    .await
}

/// A fake harness whose turns go as `answer` says, and whose answers a
/// crash left are `recover`'s, each thing asked told `see` as it comes.
pub async fn serve_script(
    answer: impl Fn(&Prompted) -> Turn + Send + Sync + 'static,
    recover: impl Fn(&Recover) -> Vec<Recovered> + Send + Sync + 'static,
    see: impl Fn(Seen<'_>) + Send + Sync + 'static,
) -> Fake {
    let (answer, recover, see): (Answer, Recovering, See) =
        (Arc::new(answer), Arc::new(recover), Arc::new(see));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let fake = Fake {
        endpoint: format!("http://127.0.0.1:{port}{}", super::ACP_PATH),
        asked: Arc::default(),
        recovering: Arc::default(),
    };
    let (asked, recovering) = (fake.asked.clone(), fake.recovering.clone());
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut head = [0u8; 4];
            if stream.peek(&mut head).await.is_ok() && &head == b"HEAD" {
                use tokio::io::AsyncWriteExt;
                let _ = stream.write_all(b"HTTP/1.0 200 OK\r\n\r\n").await;
                continue;
            }
            let Ok(socket) = tokio_tungstenite::accept_async(stream).await else {
                continue;
            };
            let script = Script {
                answer: answer.clone(),
                recover: recover.clone(),
                see: see.clone(),
                asked: asked.clone(),
                recovering: recovering.clone(),
            };
            tokio::spawn(connection(socket, script));
        }
    });
    fake
}

/// What a connection answers with, and keeps.
#[derive(Clone)]
struct Script {
    answer: Answer,
    recover: Recovering,
    see: See,
    asked: Arc<Mutex<Vec<Prompted>>>,
    recovering: Arc<Mutex<Vec<Recover>>>,
}

async fn connection(
    socket: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    script: Script,
) {
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
    let recovering = script.clone();
    let _ = sdk::Agent
        .builder()
        .on_receive_request(
            async |_: InitializeRequest, responder, _cx| {
                responder.respond(InitializeResponse::new(ProtocolVersion::V1))
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            async |_: ResumeSessionRequest, responder, _cx| {
                responder.respond(ResumeSessionResponse::new())
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            async move |prompted: PromptRequest, responder, cx| {
                let said = Said::of(prompted.meta.as_ref())
                    .map_err(|err| sdk::Error::invalid_params().data(format!("{err:#}")))?;
                let text: String = prompted
                    .prompt
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect();
                let session = prompted.session_id;
                let prompted = Prompted {
                    thread: session.0.to_string(),
                    said,
                    text,
                };
                (script.see)(Seen::Prompted(&prompted));
                let turn = (script.answer)(&prompted);
                script.asked.lock().unwrap().push(prompted);
                let pieces = match turn {
                    Turn::Steered => {
                        let steered = Said {
                            steered: true,
                            ..Said::default()
                        };
                        return responder.respond(
                            PromptResponse::new(StopReason::EndTurn)
                                .meta(steered.into_meta(super::Meta::new())),
                        );
                    }
                    Turn::Answer(pieces) => (Vec::new(), pieces),
                    Turn::Working(calls, pieces) => (calls, pieces),
                };
                let (calls, mut pieces) = pieces;
                cx.send_notification(Taken {
                    session_id: session.clone(),
                })?;
                // Its own task: the connection takes more meanwhile.
                tokio::spawn(async move {
                    for ((name, title), id) in calls.into_iter().zip(0..) {
                        let call =
                            ToolCall::new(ToolCallId::new(format!("call-{id}")), title).name(name);
                        let _ = cx.send_notification(SessionNotification::new(
                            session.clone(),
                            SessionUpdate::ToolCall(call),
                        ));
                    }
                    while let Some(piece) = pieces.next().await {
                        let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(piece)));
                        let _ = cx.send_notification(SessionNotification::new(
                            session.clone(),
                            SessionUpdate::AgentMessageChunk(chunk),
                        ));
                    }
                    let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                });
                Ok(())
            },
            sdk::on_receive_request!(),
        )
        .on_receive_request(
            async move |asked: Recover, responder, _cx| {
                (recovering.see)(Seen::Recover(&asked));
                let answers = (recovering.recover)(&asked);
                recovering.recovering.lock().unwrap().push(asked);
                responder.respond(RecoveredAnswers { answers })
            },
            sdk::on_receive_request!(),
        )
        .connect_to(Lines::new(outgoing, incoming))
        .await;
}

//! A message said to the harness as Link's clients say it, on ACP: raw,
//! so a check can say what no client of Link's would, and see it refused.

use super::*;

/// One message, and what's said beside it.
#[derive(Clone, Copy, Default)]
pub(super) struct Saying<'a> {
    pub agent: Option<&'a str>,
    pub key: Option<&'a str>,
    pub thread: &'a str,
    pub text: &'a str,
    pub from: Option<&'a str>,
    pub from_name: Option<&'a str>,
    pub hops: u32,
    /// The tools its thread narrows to, comma-separated (`Said::tools`).
    pub tools: Option<&'a str>,
}

/// How it was answered: a success's 200 and its text; else a refusal's
/// status (a connection refused, or 400 for a request refused on ACP)
/// and why.
pub(super) struct Said {
    pub status: u16,
    pub text: String,
}

impl Said {
    pub fn ok(&self) -> bool {
        self.status == 200
    }
}

/// Says `saying` to the harness at `url`.
pub(super) async fn say(url: &str, saying: Saying<'_>) -> anyhow::Result<Said> {
    said_on_acp(url, saying).await
}

/// A WebSocket to the harness's ACP at `url`, opened with `agent` and
/// `key`; or the status it was refused with, and why.
pub(super) async fn opened(
    url: &str,
    agent: Option<&str>,
    key: Option<&str>,
) -> anyhow::Result<Result<impl acp::ConnectTo<acp::Client>, Said>> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
    let mut request = url.replacen("http://", "ws://", 1).into_client_request()?;
    let headers = request.headers_mut();
    if let Some(agent) = agent {
        headers.insert(link_connection_contract::AGENT_HEADER, agent.parse()?);
    }
    if let Some(key) = key {
        headers.insert("authorization", format!("Bearer {key}").parse()?);
    }
    let socket = match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => socket,
        Err(tungstenite::Error::Http(response)) => {
            let status = response.status().as_u16();
            let text = String::from_utf8_lossy(response.body().as_deref().unwrap_or_default());
            return Ok(Err(Said {
                status,
                text: text.into(),
            }));
        }
        Err(err) => return Err(err.into()),
    };
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
    Ok(Ok(acp::Lines::new(outgoing, incoming)))
}

/// A request refused on ACP, as a status: 400 for what it said, 500 for
/// what the harness couldn't do.
fn refused(err: acp::Error) -> Said {
    let status = match err.code == acp::ErrorCode::InvalidParams {
        true => 400,
        false => 500,
    };
    Said {
        status,
        text: format!("{} {}", err.message, err.data.unwrap_or_default()),
    }
}

async fn said_on_acp(url: &str, saying: Saying<'_>) -> anyhow::Result<Said> {
    use acp::schema::v1::{
        ContentBlock, InitializeRequest, PromptRequest, ResumeSessionRequest, SessionId,
        SessionNotification, SessionUpdate, TextContent,
    };
    let lines = match opened(url, saying.agent, saying.key).await? {
        Ok(lines) => lines,
        Err(said) => return Ok(said),
    };
    // Only what's said: none of Link's when nothing is.
    let mut link = serde_json::Map::new();
    if let Some(from) = saying.from {
        link.insert("from".into(), from.into());
    }
    if let Some(name) = saying.from_name {
        link.insert("fromName".into(), name.into());
    }
    if saying.hops > 0 {
        link.insert("hops".into(), saying.hops.into());
    }
    if let Some(tools) = saying.tools {
        let tools: Vec<&str> = tools.split(',').filter(|t| !t.is_empty()).collect();
        link.insert("tools".into(), tools.into());
    }
    let session = SessionId::new(saying.thread);
    let mut prompt = PromptRequest::new(
        session.clone(),
        vec![ContentBlock::Text(TextContent::new(saying.text))],
    );
    if !link.is_empty() {
        prompt.meta = Some(serde_json::Map::from_iter([(
            "link".into(),
            Value::Object(link),
        )]));
    }
    let text = Arc::new(std::sync::Mutex::new(String::new()));
    let heard = text.clone();
    let answered = acp::Client
        .builder()
        .on_receive_notification(
            async move |update: SessionNotification, _cx| {
                if let SessionUpdate::AgentMessageChunk(chunk) = update.update
                    && let ContentBlock::Text(piece) = chunk.content
                {
                    heard
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push_str(&piece.text);
                }
                Ok(())
            },
            acp::on_receive_notification!(),
        )
        .connect_with(lines, async |cx: acp::ConnectionTo<acp::Agent>| {
            cx.send_request(InitializeRequest::new(acp::schema::ProtocolVersion::V1))
                .block_task()
                .await?;
            cx.send_request(ResumeSessionRequest::new(session, "/"))
                .block_task()
                .await?;
            cx.send_request(prompt).block_task().await
        })
        .await;
    Ok(match answered {
        Ok(_) => Said {
            status: 200,
            text: text.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        },
        Err(err) => refused(err),
    })
}

/// Asks the harness at `url` for the answers a crash left, as
/// `recover` says (`{}` once, `{"two_phase": true}`, or `{"done": [...]}`):
/// its answer's JSON, or a refusal.
pub(super) async fn recover(
    url: &str,
    key: Option<&str>,
    asked: Value,
) -> anyhow::Result<(u16, Value)> {
    let lines = match opened(url, Some(AGENT), key).await? {
        Ok(lines) => lines,
        Err(said) => return Ok((said.status, Value::Null)),
    };
    let asked: link_connection_contract::Recover = serde_json::from_value(asked)?;
    let answered = acp::Client
        .builder()
        .connect_with(lines, async |cx: acp::ConnectionTo<acp::Agent>| {
            cx.send_request(acp::schema::v1::InitializeRequest::new(
                acp::schema::ProtocolVersion::V1,
            ))
            .block_task()
            .await?;
            cx.send_request(asked).block_task().await
        })
        .await;
    Ok(match answered {
        Ok(answers) => (200, serde_json::to_value(answers)?),
        Err(err) => (refused(err).status, Value::Null),
    })
}

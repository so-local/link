//! Socket Mode transport shared by pairing and the running bridge.
use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

pub(crate) type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub(crate) enum Event {
    Connected,
    Message(Value),
}

pub(crate) async fn open(token: &str) -> anyhow::Result<Socket> {
    let reply = crate::call(token, "apps.connections.open", &[]).await?;
    let url = reply["url"]
        .as_str()
        .context("Slack sent no Socket Mode address")?;
    link_fetch::install_crypto_provider();
    // The URL holds a ticket, so it is never included in an error.
    tokio_tungstenite::connect_async(url)
        .await
        .map(|(socket, _)| socket)
        .map_err(|_| anyhow::anyhow!("couldn't connect to Slack's Socket Mode"))
}

pub(crate) async fn event(socket: &mut Socket) -> anyhow::Result<Option<Event>> {
    while let Some(frame) = socket.next().await {
        let text = match frame.context("reading Slack's Socket Mode")? {
            Message::Text(text) => text,
            Message::Close(_) => return Ok(None),
            Message::Ping(_) => {
                socket.flush().await.context("answering Slack's ping")?;
                continue;
            }
            _ => continue,
        };
        let Ok(envelope) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(id) = envelope["envelope_id"].as_str() {
            socket
                .send(Message::text(json!({"envelope_id":id}).to_string()))
                .await
                .context("acknowledging Slack's event")?;
        }
        match envelope["type"].as_str() {
            Some("hello") => return Ok(Some(Event::Connected)),
            Some("disconnect") => return Ok(None),
            Some("events_api") => return Ok(Some(Event::Message(envelope["payload"].clone()))),
            _ => {}
        }
    }
    Ok(None)
}

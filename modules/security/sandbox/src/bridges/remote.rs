//! A tool that's an MCP server at a URL (`lnk sandbox tool add <name>
//! --url`): `sandbox mcp-remote <url>` is its command, run outside the
//! sandbox like any tool's. It speaks MCP over stdio to the proxy's side
//! ([`crate::bridges::tools`], which asks you and logs each call as for any tool)
//! and streamable HTTP to the server: each message a POST, its answer JSON
//! or an event stream, the server's session id kept. Its headers, a token
//! among them, come from its environment (`LNK_MCP_HEADER_<n>`, `Name:
//! value`), which the tool's file keeps outside every sandbox.

use std::sync::Arc;

use anyhow::ensure;
use link_format::WithinExt;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// The variables holding a remote tool's headers: `LNK_MCP_HEADER_0`, ...
pub const HEADER_ENV: &str = "LNK_MCP_HEADER_";

/// A server's URL, or why it isn't one: `https://`, or `http://` to this
/// machine's loopback only, so a token never crosses a network in the
/// clear. Its host is read from its address whole, with no user in it,
/// so `http://localhost:80@example.com` is no URL on this machine.
pub fn check_url(url: &str) -> anyhow::Result<()> {
    let refused = || {
        anyhow::anyhow!("{url:?} isn't an MCP server's URL: https://, or http:// on this machine")
    };
    let plain = !url.is_empty() && url.bytes().all(|b| b.is_ascii_graphic());
    let (after, https) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(after), _) => (after, true),
        (None, Some(after)) => (after, false),
        (None, None) => return Err(refused()),
    };
    let authority = &after[..after.find(['/', '?', '#']).unwrap_or(after.len())];
    ensure!(plain && !authority.contains('@'), refused());
    let (host, _) = link_sandbox_route::http::host_port(authority, Some(80)).ok_or_else(refused)?;
    ensure!(
        https || ["127.0.0.1", "localhost", "::1"].contains(&host.as_str()),
        refused()
    );
    Ok(())
}

/// A header as `--header` takes it, `Name: value`, checked: refused
/// without the line, which may hold a secret (a token with its colon
/// left out), so no error shows it.
pub fn check_header(header: &str) -> anyhow::Result<(String, String)> {
    let parsed = header.split_once(':').and_then(|(name, value)| {
        let name = name.trim();
        let value = value.trim();
        let ok = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            && !value.chars().any(char::is_control);
        ok.then(|| (name.to_string(), value.to_string()))
    });
    parsed.within("a line isn't a header, Name: value")
}

/// The longest event read from a server: past it, the answer is refused
/// rather than held. Room for the largest message a tool may pass on.
const MAX_EVENT: usize = 16 * 1024 * 1024;

/// An event stream, read as it arrives: each whole event's `data` lines,
/// joined. Each byte is looked at once, however many pieces a long event
/// comes in, and an event is decoded only when it is whole, so a
/// character split between two pieces stays whole.
#[derive(Default)]
pub struct Events {
    partial: Vec<u8>,
    /// Where the line being read starts in `partial`.
    line: usize,
    /// How much of `partial` is known to hold no line break past `line`.
    scanned: usize,
}

impl Events {
    /// The messages of the events `bytes` completes.
    pub fn push(&mut self, bytes: &[u8]) -> anyhow::Result<Vec<String>> {
        self.partial.extend_from_slice(bytes);
        let mut data = Vec::new();
        let mut event = 0;
        let mut from = self.scanned;
        while let Some(at) = self.partial[from..].iter().position(|b| *b == b'\n') {
            let end = from + at;
            let line = &self.partial[self.line..end];
            // A blank line ends the event.
            if line.is_empty() || line == b"\r" {
                data.extend(message(&self.partial[event..self.line]));
                event = end + 1;
            }
            self.line = end + 1;
            from = end + 1;
        }
        if event > 0 {
            self.partial.drain(..event);
            self.line -= event;
        }
        self.scanned = self.partial.len();
        ensure!(
            self.partial.len() <= MAX_EVENT,
            "the server sent an event over {} MiB",
            MAX_EVENT >> 20
        );
        Ok(data)
    }
}

/// One event's `data` lines, joined, if it has any.
fn message(event: &[u8]) -> Option<String> {
    let event = String::from_utf8_lossy(event);
    let lines: Vec<&str> = event
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(|d| d.strip_prefix(' ').unwrap_or(d))
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// `sandbox mcp-remote <url>`: MCP over stdio, to the server at `url`.
pub async fn bridge(url: &str) -> anyhow::Result<()> {
    check_url(url)?;
    link_fetch::install_crypto_provider();
    let mut headers = reqwest::header::HeaderMap::new();
    for (key, value) in link_secretive::process::variables(|k| k.starts_with(HEADER_ENV)) {
        let (name, value) = check_header(&value).within(format!("${key}"))?;
        headers.insert(
            reqwest::header::HeaderName::from_bytes(name.as_bytes())?,
            reqwest::header::HeaderValue::from_str(&value)?,
        );
    }
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;
    let session: Arc<std::sync::Mutex<Option<String>>> = Default::default();
    let out = Arc::new(tokio::sync::Mutex::new(tokio::fs::File::from_std(
        link_format::streams::Protocol::file()?,
    )));
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let (client, url, session, out) = (
            client.clone(),
            url.to_string(),
            session.clone(),
            out.clone(),
        );
        // Each on its own: a long call holds up no other.
        let initialize = message.get("method").and_then(Value::as_str) == Some("initialize");
        let task = tokio::spawn(async move {
            let id = message.get("id").cloned();
            if let Err(err) = post(&client, &url, &message, &session, &out).await
                && let Some(id) = id
            {
                let error = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32000, "message": format!("the server at {url}: {err:#}")},
                });
                let _ = write(&out, &error).await;
            }
        });
        // Its answer names the session the rest go on.
        if initialize {
            let _ = task.await;
        }
    }
    let id = session.lock().ok().and_then(|s| s.clone());
    if let Some(id) = id {
        let _ = client.delete(url).header("Mcp-Session-Id", id).send().await;
    }
    Ok(())
}

/// One message to the server, and each message its answer holds, written
/// out as a line.
async fn post(
    client: &reqwest::Client,
    url: &str,
    message: &Value,
    session: &std::sync::Mutex<Option<String>>,
    out: &tokio::sync::Mutex<tokio::fs::File>,
) -> anyhow::Result<()> {
    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(serde_json::to_vec(message)?);
    if let Some(id) = session.lock().ok().and_then(|s| s.clone()) {
        request = request.header("Mcp-Session-Id", id);
    }
    let mut response = request.send().await?;
    if let Some(id) = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        && let Ok(mut session) = session.lock()
    {
        *session = Some(id.to_string());
    }
    let status = response.status();
    ensure!(status.is_success(), "{status}");
    let stream = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.starts_with("text/event-stream"));
    if !stream {
        let body = response.bytes().await?;
        if let Ok(answer) = serde_json::from_slice::<Value>(&body) {
            write(out, &answer).await?;
        }
        return Ok(());
    }
    let mut events = Events::default();
    while let Some(chunk) = response.chunk().await? {
        for data in events.push(&chunk)? {
            if let Ok(answer) = serde_json::from_str::<Value>(&data) {
                write(out, &answer).await?;
            }
        }
    }
    Ok(())
}

async fn write(out: &tokio::sync::Mutex<tokio::fs::File>, message: &Value) -> anyhow::Result<()> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    let mut out = out.lock().await;
    out.write_all(&line).await?;
    out.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_is_https_or_on_this_machine() {
        for good in [
            "https://api.githubcopilot.com/mcp/",
            "http://127.0.0.1:8080/mcp",
            "http://localhost/mcp",
        ] {
            check_url(good).unwrap();
        }
        for bad in [
            "http://example.com/mcp",
            "http://localhost.evil.com/mcp",
            // A user in the address: the host is after the @.
            "http://localhost@evil.example/mcp",
            "http://localhost:80@evil.example/",
            "http://127.0.0.1:x@evil.example/",
            "https://user:token@example.com/mcp",
            "ftp://x",
            "https://a b",
            "",
        ] {
            assert!(check_url(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn headers_are_a_name_and_a_value() {
        assert_eq!(
            check_header("Authorization: Bearer ghp_x").unwrap(),
            ("Authorization".into(), "Bearer ghp_x".into())
        );
        for bad in ["Authorization", ": x", "A b: c", "X: a\r\nY: b"] {
            assert!(check_header(bad).is_err(), "{bad:?}");
        }
        // A token with its colon left out is refused without it.
        let refused = check_header("Authorization Bearer ghp_secret").unwrap_err();
        assert!(
            !format!("{refused:#}").contains("ghp_secret"),
            "{refused:#}"
        );
    }

    #[test]
    fn an_event_streams_messages_come_whole() {
        let mut events = Events::default();
        let data = events
            .push(b"event: message\ndata: {\"id\":1}\n\ndata: {\"id\"")
            .unwrap();
        assert_eq!(data, ["{\"id\":1}"]);
        let data = events.push(b":2}\r\n\r\n: a comment\n\n").unwrap();
        assert_eq!(data, ["{\"id\":2}"]);
        assert!(events.partial.is_empty());
    }

    #[test]
    fn an_event_in_many_pieces_is_read_once_and_whole() {
        let mut events = Events::default();
        // "é" is two bytes: split between two pieces, it stays one character.
        let event = format!("data: {{\"text\":\"{}\"}}\n\n", "é".repeat(100_000));
        let mut data = Vec::new();
        for piece in event.as_bytes().chunks(7) {
            data.extend(events.push(piece).unwrap());
        }
        assert_eq!(data.len(), 1);
        assert_eq!(data[0], event[6..event.len() - 2]);
        // Lines of one event are joined, however the pieces fall.
        let mut data = Vec::new();
        for piece in b"data: a\r\ndata: b\r\n\r\n".chunks(1) {
            data.extend(events.push(piece).unwrap());
        }
        assert_eq!(data, ["a\nb"]);
    }

    #[test]
    fn an_event_too_long_is_refused() {
        let mut events = Events::default();
        let piece = vec![b'x'; 1024 * 1024];
        let refused = (0..20).any(|_| events.push(&piece).is_err());
        assert!(refused);
    }
}

//! The secret in a WebSocket's first message ([`SecretSocket`]): a
//! service that takes its secret there (Discord's Gateway) is opened by
//! the bridge at its rule's socket host, the program's first message has
//! the placeholder at the rule's pointer replaced with the secret, and
//! every frame after it crosses as it is, both ways, unread.
//!
//! The opening handshake crosses whole: the program's
//! `Sec-WebSocket-Key` goes to the service and its `Sec-WebSocket-Accept`
//! comes back, so the bridge reads frames only from the program, and
//! only its first.

use anyhow::{Context, bail};
use link_sandbox_route::http::RequestHead;
use link_sandbox_route::{Verdict, Verdicts};
use link_secret_contract::route::{Sealed, SecretSocket};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The longest first message it reads: a login is far smaller.
const FIRST_MAX: usize = 64 * 1024;

/// The longest answer head it reads from the service.
const HEAD_MAX: usize = 16 * 1024;

/// Whether `head` asks to open a WebSocket.
pub fn is_upgrade(head: &[u8]) -> bool {
    RequestHead::parse(head).is_ok_and(|request| {
        request
            .headers
            .iter()
            .any(|line| line.name == "upgrade" && line.value.eq_ignore_ascii_case("websocket"))
    })
}

/// The head to send the service for the program's `head`, or why not:
/// a `GET` of the rule's socket path on the proxy, opened at `/` on the
/// socket's host with its query kept, its handshake's headers as they
/// are but for those Link sets.
pub fn prepare(head: &[u8], secret: &Sealed) -> Result<Vec<u8>, String> {
    let Some(socket) = &secret.rule.socket else {
        return Err("no socket on this service's rule".into());
    };
    let request = RequestHead::parse(head)?;
    if request.method != "GET" {
        return Err(format!("{} doesn't open a socket", request.method));
    }
    let Some((name, rest)) = crate::bridge::named(request.target) else {
        return Err(format!("{} isn't on the wire", request.target));
    };
    if name != secret.name {
        return Err(format!("no secret {name:?} on this sandbox's proxy"));
    }
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    if path != socket.path {
        return Err(format!(
            "{path:?} isn't the socket this service's rule allows"
        ));
    }
    let target = match query {
        Some(query) => format!("/?{query}"),
        None => "/".into(),
    };
    let mut out = format!("GET {target} HTTP/1.1\r\nHost: {}\r\n", socket.host);
    for line in &request.headers {
        match line.name.as_str() {
            "host" | "authorization" | "keep-alive" | "te" | "trailer" | "expect" => {}
            other if other.starts_with("proxy-") => {}
            _ => {
                out.push_str(line.line);
                out.push_str("\r\n");
            }
        }
    }
    out.push_str("\r\n");
    Ok(out.into_bytes())
}

/// One socket on the route: its handshake's `head` (already read)
/// prepared and sent with `connect`'s connection to the socket's host,
/// the service's answer copied back, then the program's first message
/// sent with the secret in it, and every frame after it carried both
/// ways until either side ends. A first message without the placeholder
/// where the rule says ends the socket, the secret unsent.
pub async fn relay<S, O, F, Fut>(
    mut client: S,
    head: &[u8],
    secret: &Sealed,
    connect: F,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Verdicts + Unpin,
    O: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<O>>,
{
    let socket = secret.rule.socket.as_ref().context("no socket")?;
    let to = format!("{}:443", socket.host);
    let prepared = match prepare(head, secret) {
        Ok(prepared) => prepared,
        Err(why) => {
            client.say(&Verdict::refused(to)).await?;
            let body = format!("Link's sandbox: {why}\n");
            let _ = client
                .write_all(
                    format!(
                        "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;
            bail!("refused: {why}");
        }
    };
    let mut origin = match connect().await {
        Ok(origin) => origin,
        Err(err) => {
            client.say(&Verdict::failed(to)).await?;
            let _ = client
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            return Err(err);
        }
    };
    client.say(&Verdict::allowed(to)).await?;
    origin.write_all(&prepared).await?;
    origin.flush().await?;
    // The service's answer, and whatever came after it, to the program.
    let answered = answer_head(&mut origin).await?;
    client.write_all(&answered).await?;
    client.flush().await?;
    if !answered.starts_with(b"HTTP/1.1 101") {
        return Ok(());
    }
    let (mut client_in, mut client_out) = tokio::io::split(client);
    let (mut origin_in, mut origin_out) = tokio::io::split(origin);
    // The service speaks first on some sockets (Discord's hello): what
    // it sends crosses while the program's first message is awaited.
    let down = tokio::io::copy(&mut origin_in, &mut client_out);
    let up = async {
        let first = first_message(&mut client_in, socket, secret).await?;
        origin_out.write_all(&first).await?;
        origin_out.flush().await?;
        tokio::io::copy(&mut client_in, &mut origin_out).await?;
        anyhow::Ok(())
    };
    tokio::select! {
        done = down => { done?; }
        done = up => { done?; }
    }
    Ok(())
}

/// The service's answer to the handshake, read to the end of its head,
/// with whatever came after it.
async fn answer_head<O: AsyncRead + Unpin>(origin: &mut O) -> anyhow::Result<Vec<u8>> {
    let mut got = Vec::new();
    let mut buf = [0u8; 2048];
    while !got.windows(4).any(|w| w == b"\r\n\r\n") {
        anyhow::ensure!(
            got.len() < HEAD_MAX,
            "the service's answer head is too long"
        );
        let n = origin.read(&mut buf).await?;
        anyhow::ensure!(n > 0, "the service closed before it answered");
        got.extend_from_slice(&buf[..n]);
    }
    Ok(got)
}

/// The program's first frame, read whole, its text's placeholder at the
/// rule's pointer replaced with the secret, framed again as it came.
async fn first_message<R: AsyncRead + Unpin>(
    client: &mut R,
    socket: &SecretSocket,
    secret: &Sealed,
) -> anyhow::Result<Vec<u8>> {
    let mut start = [0u8; 2];
    client.read_exact(&mut start).await?;
    anyhow::ensure!(
        start[0] == 0x81,
        "the first message isn't one whole text frame"
    );
    anyhow::ensure!(start[1] & 0x80 != 0, "a program's frame is masked");
    let length = match start[1] & 0x7f {
        126 => {
            let mut at = [0u8; 2];
            client.read_exact(&mut at).await?;
            usize::from(u16::from_be_bytes(at))
        }
        127 => {
            let mut at = [0u8; 8];
            client.read_exact(&mut at).await?;
            usize::try_from(u64::from_be_bytes(at)).unwrap_or(usize::MAX)
        }
        short => usize::from(short),
    };
    anyhow::ensure!(length <= FIRST_MAX, "the first message is too long");
    let mut mask = [0u8; 4];
    client.read_exact(&mut mask).await?;
    let mut payload = vec![0u8; length];
    client.read_exact(&mut payload).await?;
    masked(&mut payload, mask);
    let text = filled(&payload, socket, secret)?;
    Ok(frame(text.into_bytes(), mask))
}

/// `payload` (the first message's text, unmasked) with the secret in
/// place of the placeholder at the rule's pointer; an error unless the
/// placeholder is there.
pub fn filled(payload: &[u8], socket: &SecretSocket, secret: &Sealed) -> anyhow::Result<String> {
    let mut message: serde_json::Value =
        serde_json::from_slice(payload).context("the first message isn't JSON")?;
    let Some(serde_json::Value::String(at)) = message.pointer_mut(&socket.pointer) else {
        bail!("the first message has no placeholder at {}", socket.pointer);
    };
    anyhow::ensure!(
        link_secretive::equal(at.as_bytes(), secret.placeholder.expose().as_bytes()),
        "not the placeholder this sandbox was given"
    );
    *at = secret.value.expose_owned();
    Ok(message.to_string())
}

/// `payload` as one masked text frame, masked with `mask`.
fn frame(mut payload: Vec<u8>, mask: [u8; 4]) -> Vec<u8> {
    let mut out = vec![0x81];
    match payload.len() {
        n if n < 126 => out.push(0x80 | n as u8),
        n if n <= usize::from(u16::MAX) => {
            out.push(0x80 | 126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(0x80 | 127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(&mask);
    masked(&mut payload, mask);
    out.extend_from_slice(&payload);
    out
}

/// Masks (or unmasks) `bytes` with `mask`, as RFC 6455 says.
fn masked(bytes: &mut [u8], mask: [u8; 4]) {
    for (n, byte) in bytes.iter_mut().enumerate() {
        *byte ^= mask[n % 4];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_secret_contract::route::SecretRule;

    const TOKEN: &str = "MTIzNDU2Nzg5MDEyMzQ1Njc4.GabcDe.real-secret";

    fn gateway() -> Sealed {
        let rule = SecretRule {
            host: "discord.com".into(),
            paths: vec![link_secret_contract::route::SecretPath {
                prefix: "api/v10/".into(),
                rest: link_secret_contract::route::SecretRest::Path,
            }],
            header: Some(link_secret_contract::route::SecretHeader::prefixed(
                "Authorization",
                "Bot ",
            )),
            socket: Some(SecretSocket {
                host: "gateway.discord.gg".into(),
                path: "gateway".into(),
                pointer: "/d/token".into(),
            }),
            ..SecretRule::default()
        };
        rule.check().unwrap();
        Sealed::new(
            &crate::bridge::tests::RUN,
            "discord",
            TOKEN.to_string().into(),
            rule,
        )
    }

    #[test]
    fn a_socket_opens_at_its_host_with_its_query() {
        let secret = gateway();
        let head = b"GET /_link/secret/discord/gateway?v=10&encoding=json HTTP/1.1\r\n\
                     Host: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                     Sec-WebSocket-Key: a2V5\r\nSec-WebSocket-Version: 13\r\n\r\n";
        assert!(is_upgrade(head));
        let sent = String::from_utf8(prepare(head, &secret).unwrap()).unwrap();
        assert!(
            sent.starts_with("GET /?v=10&encoding=json HTTP/1.1\r\nHost: gateway.discord.gg\r\n"),
            "{sent}"
        );
        assert!(sent.contains("Sec-WebSocket-Key: a2V5\r\n"));
        assert!(!sent.contains("127.0.0.1"));
        let other = b"GET /_link/secret/discord/elsewhere HTTP/1.1\r\nUpgrade: websocket\r\n\r\n";
        assert!(prepare(other, &secret).is_err());
    }

    #[test]
    fn the_first_message_takes_the_secret_at_its_pointer_alone() {
        let secret = gateway();
        let socket = secret.rule.socket.clone().unwrap();
        let placeholder = secret.placeholder.expose().clone();
        let login = serde_json::json!({"op": 2, "d": {"token": placeholder, "intents": 1}});
        let sent = filled(login.to_string().as_bytes(), &socket, &secret).unwrap();
        let sent: serde_json::Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(sent["d"]["token"], TOKEN);
        assert_eq!(sent["d"]["intents"], 1);
        let wrong = serde_json::json!({"op": 2, "d": {"token": "guessed"}});
        assert!(filled(wrong.to_string().as_bytes(), &socket, &secret).is_err());
        let elsewhere = serde_json::json!({"op": 2, "token": placeholder});
        assert!(filled(elsewhere.to_string().as_bytes(), &socket, &secret).is_err());
    }

    /// The service's hello crosses before the program logs in; the
    /// login reaches the service with the secret, framed as a program's;
    /// the program never sees the secret.
    #[tokio::test]
    async fn a_socket_carries_the_secret_in_its_first_message() {
        let secret = gateway();
        let placeholder = secret.placeholder.expose().clone();
        let (mut program, proxy_end) = tokio::io::duplex(64 * 1024);
        let (service_end, mut service) = tokio::io::duplex(64 * 1024);
        let head = b"GET /_link/secret/discord/gateway?v=10 HTTP/1.1\r\n\
                     Upgrade: websocket\r\nConnection: Upgrade\r\n\
                     Sec-WebSocket-Key: a2V5\r\nSec-WebSocket-Version: 13\r\n\r\n"
            .to_vec();
        let relayed = tokio::spawn(async move {
            relay(
                link_sandbox_route::Unsaid(proxy_end),
                &head,
                &secret,
                || async { Ok(service_end) },
            )
            .await
        });
        macro_rules! within {
            ($what:expr) => {
                tokio::time::timeout(std::time::Duration::from_secs(5), $what)
                    .await
                    .expect("in time")
            };
        }
        // The service: the handshake, answered, then its hello.
        let asked = within!(answer_head(&mut service)).unwrap();
        assert!(asked.starts_with(b"GET /?v=10 HTTP/1.1\r\nHost: gateway.discord.gg\r\n"));
        let hello = br#"{"op":10}"#;
        let mut answer = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                           Connection: Upgrade\r\nSec-WebSocket-Accept: x\r\n\r\n"
            .to_vec();
        answer.extend_from_slice(&[0x81, hello.len() as u8]);
        answer.extend_from_slice(hello);
        service.write_all(&answer).await.unwrap();
        // The program reads it all before it logs in.
        let mut seen = Vec::new();
        let mut buf = [0u8; 1024];
        while !seen.ends_with(hello) {
            let n = within!(program.read(&mut buf)).unwrap();
            assert!(n > 0);
            seen.extend_from_slice(&buf[..n]);
        }
        let login = serde_json::json!({"op": 2, "d": {"token": placeholder}}).to_string();
        program
            .write_all(&frame(login.into_bytes(), [1, 2, 3, 4]))
            .await
            .unwrap();
        // The service gets one masked text frame, the secret in it.
        let mut start = [0u8; 2];
        within!(service.read_exact(&mut start)).unwrap();
        assert_eq!(start[0], 0x81);
        let length = usize::from(start[1] & 0x7f);
        let mut mask = [0u8; 4];
        service.read_exact(&mut mask).await.unwrap();
        let mut payload = vec![0u8; length];
        service.read_exact(&mut payload).await.unwrap();
        masked(&mut payload, mask);
        let got: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(got["d"]["token"], TOKEN);
        assert!(!String::from_utf8_lossy(&seen).contains("real-secret"));
        drop(service);
        drop(program);
        let _ = within!(relayed);
    }
}

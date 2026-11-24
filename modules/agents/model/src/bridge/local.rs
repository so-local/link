//! The local model route: a model runtime's port of this machine,
//! reached from a sandbox for inference only.
//!
//! A runtime's API is more than chat. Ollama's also pulls models from the
//! internet, creates, copies, pushes and deletes them: writes outside the
//! sandbox, and requests made from this machine. So a model port the
//! policy names is not relayed as raw TCP: each request is read here,
//! outside, and passed on only if its method and path are inference
//! (chat, generate, embeddings, the models' list), Ollama's own or the
//! OpenAI-compatible `/v1`. Anything else gets a 403 from Link and never
//! reaches the runtime. The program in the sandbox keeps the runtime's
//! address: nothing in it changes.

use anyhow::bail;
use link_format::WithinExt;
use link_sandbox_route::Verdicts;
use link_sandbox_route::http::AnswerHead;
use link_sandbox_route::http::HEAD_TIMEOUT;
use link_sandbox_route::http::RequestHead;
use link_sandbox_route::http::read_head;
use link_sandbox_route::{Hangup, Verdict};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

/// Inference requests, by method and exact path (before any `?query`).
const ALLOWED: &[(&str, &str)] = &[
    // Ollama: is it running, which version, which models (a model's own
    // details are Link's, at `/_link/model/show`, answered by `show`).
    // Ollama: inference.
    ("POST", "/api/chat"),
    ("POST", "/api/generate"),
    ("POST", "/api/embed"),
    ("POST", "/api/embeddings"),
    // OpenAI-compatible (Ollama, LM Studio, llama.cpp, vLLM).
    ("POST", "/v1/chat/completions"),
    ("POST", "/v1/completions"),
    ("POST", "/v1/embeddings"),
    // A voice note heard (whisper.cpp's server, LocalAI, vLLM).
    ("POST", "/v1/audio/transcriptions"),
];

/// Whether `method` on `target` (a request's target, `/path?query`) is
/// inference. Paths are matched as sent: an encoded or relative one
/// (`/api/%70ull`, `/api/chat/../pull`) is refused, not decoded.
pub fn allowed(method: &str, target: &str) -> bool {
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    ALLOWED.contains(&(method, path))
}

/// One connection from the sandbox to the runtime on `port`: its first
/// request is checked, rewritten to close the connection after its
/// response (so the next request comes on a new connection, and is
/// checked too), and passed on with the rest of the stream.
pub async fn relay<S>(
    mut client: S,
    port: u16,
    counter: &crate::bridge::usage::Counter,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Verdicts + Hangup + Unpin,
{
    let (head, rest) = tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut client))
        .await
        .within("no request in time")??;
    relay_request(client, &head, rest, port, counter).await
}

/// [`relay`], its request's head already read (and `rest`, what came
/// after it); its verdict said first; what a model's answer counts added
/// to `counter` ([`crate::bridge::usage`]).
pub async fn relay_request<S>(
    mut client: S,
    head: &[u8],
    rest: Vec<u8>,
    port: u16,
    counter: &crate::bridge::usage::Counter,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Verdicts + Hangup + Unpin,
{
    let to = format!("127.0.0.1:{port}");
    if let Ok(request) = RequestHead::parse(head)
        && link_model_contract::program_route(request.path()).is_some()
    {
        let grant = link_model_contract::route::actions::Api {
            transcribe: None,
            window: None,
            answer: None,
            backend: link_model_contract::route::actions::Backend::Local { port },
            name: String::new(),
            allow: None,
            fallback: false,
        };
        return super::actions::serve(
            client,
            head,
            rest,
            &grant,
            std::path::Path::new(""),
            "",
            "",
            "direct",
        )
        .await;
    }
    if let Ok(request) = RequestHead::parse(head)
        && matches!(request.method, "GET" | "HEAD")
        && matches!(
            request.path(),
            "/" | "/health" | "/api/version" | "/api/tags" | "/api/ps"
        )
    {
        return answered(
            client,
            to,
            crate::bridge::show::native(port, request.path()).await,
        )
        .await;
    }
    // A model's details: asked of the runtime here, answered in Link's
    // shape, never relayed.
    if let Ok(request) = RequestHead::parse(head)
        && request.method == "GET"
        && let Some(name) = link_model_contract::show::name_of(request.target)
    {
        let name = name.to_string();
        return answered(client, to, crate::bridge::show::ask(port, &name).await).await;
    }
    // The models' list, the bridge's too: plain ids alone.
    if let Ok(request) = RequestHead::parse(head)
        && request.method == "GET"
        && request.path() == "/v1/models"
    {
        return answered(client, to, crate::bridge::show::list(port).await).await;
    }
    let head = match rewrite(head) {
        Ok(head) => head,
        Err(why) => return refuse(&mut client, to, &why).await,
    };
    client.say(&Verdict::allowed(to)).await?;
    let mut runtime = match link_sandbox_route::io::connect_tcp(("127.0.0.1", port)).await {
        Ok(runtime) => runtime,
        // Said to the client, which would otherwise see its connection
        // close with nothing, and know no more than that.
        Err(err) => {
            let why = format!("reaching the model on port {port}: {err:#}");
            answer(&mut client, "502 Bad Gateway", &why).await;
            bail!(why);
        }
    };
    // Its body, as much as its head says, sent with it in one go: a
    // runtime that answers once it has read the head, and closes, would
    // otherwise reset the connection on the body still coming, its
    // answer lost. A larger one streams after it.
    let format = metered(&head);
    let length = RequestHead::parse(head.as_bytes())
        .ok()
        .and_then(|request| request.length().ok())
        .unwrap_or(0) as usize;
    // What a model's request asks, taken once it's whole ([`taken`]).
    let api = RequestHead::parse(head.as_bytes())
        .ok()
        .filter(|request| request.method == "POST")
        .and_then(|request| link_model_contract::context::Api::of(request.path()));
    let head_len = head.len();
    let mut request = head.into_bytes();
    request.extend_from_slice(&rest);
    if length <= MAX_WHOLE {
        use tokio::io::AsyncReadExt;
        let reading = async {
            let mut left = length.saturating_sub(rest.len());
            let mut chunk = vec![0u8; 64 * 1024];
            while left > 0 {
                let want = left.min(chunk.len());
                let n = client.read(&mut chunk[..want]).await?;
                anyhow::ensure!(n > 0, "the request's body didn't arrive whole");
                request.extend_from_slice(&chunk[..n]);
                left -= n;
            }
            anyhow::Ok(())
        };
        tokio::time::timeout(BODY_TIMEOUT, reading)
            .await
            .map_err(|_| anyhow::anyhow!("the request's body didn't arrive in time"))??;
        if let Some(api) = api {
            taken(api, &request[head_len..], counter).await;
        }
    }
    runtime.write_all(&request).await?;
    // Its answer says the connection closes after it, as it does: a
    // client pooling it would send its next request here, onto a closed
    // runtime, and fail with nothing answered.
    let runtime = Closing::new(runtime);
    let client = crate::bridge::usage::Tapped::new(client, format, counter);
    // Ended once nothing crosses either way for a while, as a tunnel is.
    let idle = link_sandbox_route::io::idle();
    link_sandbox_route::io::joined_until_idle(client, runtime, idle).await
}

/// What `body`, a whole request in `api`, asks, given to `counter`; a
/// long one parsed off the async runtime. One that isn't JSON asks
/// nothing: it's relayed as it is.
async fn taken(
    api: link_model_contract::context::Api,
    body: &[u8],
    counter: &crate::bridge::usage::Counter,
) {
    let take = move |body: &[u8]| {
        let json: serde_json::Value = serde_json::from_slice(body).ok()?;
        crate::bridge::context::Asked::of(api, &json, None)
    };
    let asked = match body.len() > OFF_RUNTIME {
        true => {
            let body = body.to_vec();
            tokio::task::spawn_blocking(move || take(&body))
                .await
                .ok()
                .flatten()
        }
        false => take(body),
    };
    if let Some(asked) = asked {
        counter.asked(asked);
    }
}

/// A body past it is parsed off the async runtime.
const OFF_RUNTIME: usize = 1024 * 1024;

/// The largest body sent whole with its head; a larger one streams after.
const MAX_WHOLE: usize = 32 * 1024 * 1024;

/// How long a body sent whole may take to arrive.
const BODY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Refuses the call, `why`: its verdict, to `to`, then a 403 saying why.
pub async fn refuse<S>(client: &mut S, to: String, why: &str) -> anyhow::Result<()>
where
    S: AsyncWrite + Verdicts + Unpin,
{
    client.say(&Verdict::refused(to)).await?;
    answer(client, "403 Forbidden", why).await;
    bail!("refused: {why}")
}

/// A connection to a runtime whose answer's head is read with
/// `Connection: close` in place of any `Connection` or `Keep-Alive` it
/// had (each head, through an interim `1xx`): the request was sent with
/// `Connection: close` ([`rewrite`]), so the runtime closes after it,
/// and the client is told so. Without it, a runtime that keeps its
/// connections (the stand-in, Ollama's) answers as if this one stays
/// open, and the client sends its next request on it. Writes pass
/// through.
struct Closing<S> {
    inner: S,
    /// The head being read, until its blank line.
    head: Vec<u8>,
    /// What's read and rewritten, not yet handed out.
    out: Vec<u8>,
    /// Past the last head: everything passes through.
    done: bool,
}

impl<S> Closing<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            head: Vec::new(),
            out: Vec::new(),
            done: false,
        }
    }

    /// `bytes`, read from the runtime: into a head while one is being
    /// read, rewritten when it ends; after it, as they are.
    fn take(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            if self.done {
                self.out.extend_from_slice(bytes);
                return;
            }
            let before = self.head.len();
            self.head.extend_from_slice(bytes);
            let end = self.head.windows(4).position(|w| w == b"\r\n\r\n");
            let Some(end) = end else {
                if self.head.len() > link_sandbox_route::http::MAX_HEAD {
                    // Not a head it can read: passed on as it is.
                    self.out.append(&mut self.head);
                    self.done = true;
                }
                return;
            };
            let used = end + 4 - before;
            self.head.truncate(end + 4);
            let head = std::mem::take(&mut self.head);
            let interim = AnswerHead::parse(&head).is_ok_and(|answer| answer.informational());
            self.out.extend_from_slice(&closing_head(&head));
            self.done = !interim;
            bytes = &bytes[used..];
        }
    }
}

/// An answer's `head` (to its blank line) with `Connection: close` in
/// place of the headers that say whether the connection stays.
fn closing_head(head: &[u8]) -> Vec<u8> {
    let Ok(answer) = AnswerHead::parse(head) else {
        // Not an answer: passed on closed, as it came.
        let mut out = head.strip_suffix(b"\r\n\r\n").unwrap_or(head).to_vec();
        out.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
        return out;
    };
    let mut out = format!("{}\r\n", answer.status);
    for header in &answer.headers {
        if !["connection", "keep-alive", "proxy-connection"].contains(&header.name.as_str()) {
            out.push_str(header.line);
            out.push_str("\r\n");
        }
    }
    out.push_str("Connection: close\r\n\r\n");
    out.into_bytes()
}

impl<S: AsyncRead + Unpin> AsyncRead for Closing<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::task::Poll;
        loop {
            if !self.out.is_empty() {
                let n = self.out.len().min(buf.remaining());
                buf.put_slice(&self.out[..n]);
                self.out.drain(..n);
                return Poll::Ready(Ok(()));
            }
            let mut chunk = [0u8; 8192];
            let mut read = tokio::io::ReadBuf::new(&mut chunk);
            match std::pin::Pin::new(&mut self.inner).poll_read(cx, &mut read) {
                Poll::Ready(Ok(())) if read.filled().is_empty() => {
                    // Its end: a head it never finished goes as it came.
                    let rest = std::mem::take(&mut self.head);
                    if rest.is_empty() {
                        return Poll::Ready(Ok(()));
                    }
                    self.out = rest;
                    self.done = true;
                }
                Poll::Ready(Ok(())) => {
                    let got = read.filled().to_vec();
                    self.take(&got);
                }
                other => return other,
            }
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Closing<S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Answers the client `status`, `why` in its body after "Link's
/// sandbox:", and closes.
async fn answer<S: AsyncWrite + Unpin>(client: &mut S, status: &str, why: &str) {
    let body = format!("Link's sandbox: {why}\n");
    let _ = client
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await;
}

/// What the bridge answered itself: `answer`, JSON, allowed; its error,
/// refused.
async fn answered<S, T>(mut client: S, to: String, answer: anyhow::Result<T>) -> anyhow::Result<()>
where
    S: AsyncWrite + Verdicts + Unpin,
    T: serde::Serialize,
{
    match answer {
        Ok(answer) => {
            client.say(&Verdict::allowed(to)).await?;
            answer_json(&mut client, &serde_json::to_vec(&answer)?).await;
            Ok(())
        }
        Err(err) => refuse(&mut client, to, &format!("{err:#}")).await,
    }
}

/// Answers `body`, JSON, and closes.
async fn answer_json<S: AsyncWrite + Unpin>(client: &mut S, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = client.write_all(head.as_bytes()).await;
    let _ = client.write_all(body).await;
}

/// How a request's answer gives its usage, if it's a request for a
/// model's answer: by its head's method and path.
fn metered(head: &str) -> Option<crate::bridge::usage::Format> {
    let mut parts = head.split(' ');
    let (method, target) = (parts.next()?, parts.next()?);
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    crate::bridge::usage::Format::of(method, path)
}

/// A request to the proxy on the `model` verb for a local runtime: its
/// port, and its head as the runtime expects it (its own path, `Host`
/// its own address), or why not.
pub fn unprefixed(head: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let request = RequestHead::parse(head)?;
    let rest = request
        .target
        .strip_prefix(link_model_contract::route::LOCAL)
        .ok_or("not the local model route")?;
    let (port, path) = rest.split_at(rest.find(['/', '?']).unwrap_or(rest.len()));
    let port: u16 = port.parse().map_err(|_| "no model's port in the path")?;
    let path = match path {
        "" => "/".to_string(),
        p if p.starts_with('?') => format!("/{p}"),
        p => p.to_string(),
    };
    let (method, version) = (request.method, request.version);
    let mut out = format!("{method} {path} {version}\r\nHost: 127.0.0.1:{port}\r\n");
    for header in request.headers.iter().filter(|h| h.name != "host") {
        out.push_str(header.line);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    Ok((port, out.into_bytes()))
}

/// The request head to pass on, or why not: inference only, one request
/// per connection, no protocol switch, one way to tell the body's length;
/// a model's answer asked for uncompressed, for its usage to be read.
pub fn rewrite(head: &[u8]) -> Result<String, String> {
    let request = RequestHead::parse(head)?;
    let (method, target) = (request.method, request.target);
    if !allowed(method, target) {
        return Err(format!(
            "{method} {} isn't inference (chat, generate, embeddings, the models' list)",
            request.path()
        ));
    }
    let mut out = format!("{method} {target} {}\r\n", request.version);
    let metered = crate::bridge::usage::Format::of(method, request.path()).is_some();
    let (mut length, mut chunked) = (false, false);
    for header in &request.headers {
        let value = header.value;
        match header.name.as_str() {
            "upgrade" => return Err("no switching protocols".into()),
            "connection" | "keep-alive" | "proxy-connection" => continue,
            "accept-encoding" if metered => continue,
            "content-length" if length => return Err("two lengths".into()),
            "content-length" => length = true,
            "transfer-encoding" if chunked => return Err("two transfer-encodings".into()),
            "transfer-encoding" if value.eq_ignore_ascii_case("chunked") => chunked = true,
            "transfer-encoding" => return Err(format!("transfer-encoding {value}")),
            _ => {}
        }
        out.push_str(header.line);
        out.push_str("\r\n");
    }
    if length && chunked {
        return Err("both a length and chunks".into());
    }
    if metered {
        out.push_str("Accept-Encoding: identity\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    /// Where the tests that aren't about tokens count them.
    static UNCOUNTED: crate::bridge::usage::Counter = crate::bridge::usage::Counter::new();
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    #[test]
    fn only_inference_passes() {
        for (method, target) in [
            ("POST", "/api/chat"),
            ("POST", "/api/generate"),
            ("POST", "/v1/chat/completions"),
            ("POST", "/v1/audio/transcriptions"),
            ("POST", "/api/chat?stream=true"),
        ] {
            assert!(allowed(method, target), "{method} {target}");
        }
        for (method, target) in [
            ("GET", "/api/tags"),
            ("GET", "/api/ps"),
            ("GET", "/api/version"),
            ("GET", "/health"),
            ("POST", "/api/pull"),
            ("POST", "/api/push"),
            ("POST", "/api/create"),
            ("DELETE", "/api/delete"),
            ("POST", "/api/copy"),
            ("POST", "/api/blobs/sha256:x"),
            ("GET", "/api/chat"),
            ("POST", "/api/%70ull"),
            ("POST", "/api/chat/../pull"),
            ("POST", "//api/chat"),
            ("GET", "/v1/models/../../api/pull"),
            ("GET", "/v1/models/"),
            // Each runtime's own details of a model: Link's
            // `/_link/model/show` answers them, in one shape.
            ("POST", "/api/show"),
            ("GET", "/props"),
            ("GET", "/api/v0/models/qwen3-8b"),
            ("GET", "/v1/models/qwen3:8b"),
            ("GET", "/v1/models/library/llama3"),
            ("GET", "/_link/model/show?name=a"),
            // Answered by the bridge, never relayed.
            ("GET", "/v1/models"),
            ("POST", "/API/SHOW"),
            ("POST", "/api/%73how"),
            ("POST", "/api//show"),
            ("POST", "/api/show/"),
            ("GET", "/PROPS"),
            ("POST", "/props"),
            ("HEAD", "/props"),
            ("GET", "/props/x"),
            ("GET", "/props/../api/pull"),
            ("GET", "/api/v0/models/"),
            ("GET", "/api/v0/models/../../api/pull"),
            ("GET", "/api/v0/models/%2e%2e/x"),
            ("GET", "/api/v0/models/a/b"),
            ("GET", "/api/v0/models/a//b"),
            ("POST", "/api/v0/models/x"),
        ] {
            assert!(!allowed(method, target), "{method} {target}");
        }
    }

    #[test]
    fn the_verb_names_the_port_and_keeps_the_path() {
        let (port, head) = unprefixed(
            b"POST /_link/here/11434/api/chat?x=1 HTTP/1.1\r\nHost: 127.0.0.1:40781\r\n\
              Content-Length: 2\r\n\r\n",
        )
        .unwrap();
        assert_eq!(port, 11434);
        assert_eq!(
            String::from_utf8(head).unwrap(),
            "POST /api/chat?x=1 HTTP/1.1\r\nHost: 127.0.0.1:11434\r\nContent-Length: 2\r\n\r\n"
        );
        let (_, head) = unprefixed(b"GET /_link/here/11434 HTTP/1.1\r\n\r\n").unwrap();
        assert!(head.starts_with(b"GET / HTTP/1.1\r\n"));
        for bad in [
            &b"GET /_link/here/x/api/tags HTTP/1.1\r\n\r\n"[..],
            b"GET /_link/here//api/tags HTTP/1.1\r\n\r\n",
            b"GET /_link/here/99999/api/tags HTTP/1.1\r\n\r\n",
        ] {
            assert!(unprefixed(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn a_request_is_one_request_on_its_connection() {
        let head = rewrite(
            b"POST /api/chat HTTP/1.1\r\nHost: localhost:11434\r\nConnection: keep-alive\r\n\
              Accept-Encoding: gzip\r\nContent-Length: 2\r\n\r\n",
        )
        .unwrap();
        assert_eq!(
            head,
            "POST /api/chat HTTP/1.1\r\nHost: localhost:11434\r\nContent-Length: 2\r\n\
             Accept-Encoding: identity\r\nConnection: close\r\n\r\n"
        );
        for bad in [
            &b"GET / HTTP/1.1\r\nUpgrade: websocket\r\n\r\n"[..],
            b"POST /api/chat HTTP/1.1\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"POST /api/chat HTTP/1.1\r\nTransfer-Encoding: gzip, chunked\r\n\r\n",
            b"PRI * HTTP/2.0\r\n\r\n",
            b"POST /api/pull HTTP/1.1\r\n\r\n",
            b"POST /api/chat HTTP/1.1 extra\r\n\r\n",
            // A second request hidden where a server that ends a line at
            // a bare \n would read it, and headers hidden the same way.
            b"POST /api/chat HTTP/1.1\r\nX: a\n\nPOST /api/pull HTTP/1.1\r\n\r\n",
            b"POST /api/chat HTTP/1.1\r\nX: a\nConnection: keep-alive\r\n\r\n",
            b"POST /api/chat HTTP/1.1\r\nX: a\rTransfer-Encoding: chunked\r\n\r\n",
            b"POST /api/chat HTTP/1.1\r\nX: a\r\n Transfer-Encoding: chunked\r\n\r\n",
            b"POST /api/chat HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 9\r\n\r\n",
        ] {
            assert!(rewrite(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }

    /// A runtime that answers every request with its own request line,
    /// and counts them.
    async fn runtime() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let log = log.clone();
                tokio::spawn(async move {
                    let (head, _) = read_head(&mut s).await.unwrap();
                    let line = String::from_utf8_lossy(&head)
                        .lines()
                        .next()
                        .unwrap()
                        .to_string();
                    log.lock().unwrap().push(line.clone());
                    let body = format!("saw {line}");
                    let _ = s
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                                body.len()
                            )
                            .as_bytes(),
                        )
                        .await;
                });
            }
        });
        (port, seen)
    }

    async fn ask(port: u16, request: &str) -> String {
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        tokio::spawn(relay(link_sandbox_route::Unsaid(server), port, &UNCOUNTED));
        client.write_all(request.as_bytes()).await.unwrap();
        let mut got = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.read_to_string(&mut got),
        )
        .await
        .unwrap()
        .unwrap();
        got
    }

    /// A runtime that never answers holds no connection past the idle
    /// time.
    #[tokio::test]
    async fn a_silent_runtime_ends_when_idle() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
            drop(s);
        });
        let (_client, server) = tokio::io::duplex(64 * 1024);
        let idle = std::time::Duration::from_millis(300);
        let relayed = link_sandbox_route::io::TEST_IDLE.scope(
            idle,
            relay_request(
                link_sandbox_route::Unsaid(server),
                b"POST /api/chat HTTP/1.1\r\n\r\n",
                Vec::new(),
                port,
                &UNCOUNTED,
            ),
        );
        let ended = tokio::time::timeout(std::time::Duration::from_secs(5), relayed)
            .await
            .expect("a silent runtime held the connection open");
        assert!(format!("{:#}", ended.unwrap_err()).contains("nothing crossed"));
    }

    /// The runtime answers as if its connection stays open (no
    /// `Connection` header, or `keep-alive`), but closes it after the
    /// one request the proxy sends: the client is told so, every head
    /// through an interim one, the body as it was, however it's split.
    #[test]
    fn an_answer_says_its_connection_closes() {
        let answer: &[u8] =
            b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\
            Connection: keep-alive\r\nKeep-Alive: timeout=5\r\n\r\nok";
        let want: &[u8] = b"HTTP/1.1 100 Continue\r\nConnection: close\r\n\r\nHTTP/1.1 200 OK\r\n\
            Content-Length: 2\r\nConnection: close\r\n\r\nok";
        for split in 1..answer.len() {
            let mut closing = Closing::new(());
            for piece in answer.chunks(split) {
                closing.take(piece);
            }
            assert_eq!(
                String::from_utf8_lossy(&closing.out),
                String::from_utf8_lossy(want),
                "in pieces of {split}"
            );
        }
    }

    #[tokio::test]
    async fn chat_reaches_the_runtime_and_pull_never_does() {
        let (port, seen) = runtime().await;
        let chat = ask(
            port,
            "POST /api/chat HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}",
        )
        .await;
        assert!(chat.starts_with("HTTP/1.1 200"), "{chat}");
        assert!(chat.contains("\r\nConnection: close\r\n\r\n"), "{chat}");
        assert!(chat.ends_with("saw POST /api/chat HTTP/1.1"), "{chat}");
        let pull = ask(
            port,
            "POST /api/pull HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}",
        )
        .await;
        assert!(pull.starts_with("HTTP/1.1 403"), "{pull}");
        assert!(pull.contains("isn't inference"), "{pull}");
        assert_eq!(*seen.lock().unwrap(), ["POST /api/chat HTTP/1.1"]);

        // No model there: the client is told, not left with a closed
        // connection.
        let free = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let none = free.local_addr().unwrap().port();
        drop(free);
        let gone = ask(
            none,
            "POST /api/chat HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}",
        )
        .await;
        assert!(gone.starts_with("HTTP/1.1 502"), "{gone}");
        assert!(gone.contains(&format!("port {none}")), "{gone}");
    }

    /// A model's request to a runtime here is taken part by part as it
    /// goes; one that isn't JSON is relayed as it is, taking nothing.
    #[tokio::test]
    async fn a_models_request_is_taken_part_by_part() {
        static TAKING: crate::bridge::usage::Counter = crate::bridge::usage::Counter::new();
        let (port, _) = runtime().await;
        let body = r#"{"model":"qwen3:8b","messages":[{"role":"user","content":"hi"}]}"#;
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        tokio::spawn(relay(link_sandbox_route::Unsaid(server), port, &TAKING));
        let request = format!(
            "POST /api/chat HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        client.write_all(request.as_bytes()).await.unwrap();
        let mut got = String::new();
        client.read_to_string(&mut got).await.unwrap();
        assert!(got.starts_with("HTTP/1.1 200"), "{got}");
        let asked = TAKING.take_asked().expect("taken");
        assert_eq!(asked.model.as_deref(), Some("qwen3:8b"));
        let paths: Vec<&str> = asked.parts.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(paths, ["conversation/user"]);
        let plain = ask(
            port,
            "POST /api/chat HTTP/1.1\r\nContent-Length: 3\r\n\r\na=b",
        )
        .await;
        assert!(plain.starts_with("HTTP/1.1 200"), "{plain}");
        assert!(UNCOUNTED.take_asked().is_none());
    }

    /// A model's details are answered by the route in Link's shape, the
    /// runtime's own answer never relayed; one it can't map, refused.
    #[tokio::test]
    async fn a_models_details_are_links_never_the_runtimes() {
        use axum::{Json, Router, routing::post};
        let app = Router::new().route(
            "/api/show",
            post(|| async {
                Json(serde_json::json!({"modelfile": "FROM /home/sam/x.gguf",
                    "details": {"family": "llama"}}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let shown = async |port: u16, target: &str| {
            let (mut client, server) = tokio::io::duplex(64 * 1024);
            let head = format!("GET {target} HTTP/1.1\r\n\r\n");
            let relayed = relay_request(
                link_sandbox_route::Unsaid(server),
                head.as_bytes(),
                Vec::new(),
                port,
                &UNCOUNTED,
            );
            let _ = relayed.await;
            let mut answer = String::new();
            tokio::io::AsyncReadExt::read_to_string(&mut client, &mut answer)
                .await
                .unwrap();
            answer
        };
        let answer = shown(port, "/_link/model/show?name=llama3:8b").await;
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        assert!(
            answer.ends_with(
                r#"{"name":"llama3:8b","family":"llama","endpoint":{"ready":false,"loaded":[]}}"#
            ),
            "{answer}"
        );
        assert!(!answer.contains("/home/sam"), "{answer}");
        // Ollama's own route, asked from the sandbox: refused.
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let refused = relay_request(
            link_sandbox_route::Unsaid(server),
            b"POST /api/show HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            Vec::new(),
            port,
            &UNCOUNTED,
        )
        .await;
        assert!(refused.is_err());
        let mut answer = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut client, &mut answer)
            .await
            .unwrap();
        assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
        // A runtime that says nothing Link reads of it: refused.
        let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let quiet = silent.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(silent, Router::new()).await.unwrap() });
        let answer = shown(quiet, "/_link/model/show?name=other/model").await;
        assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    }
}

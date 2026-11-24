//! One call on the route relayed: its request read, checked and sent to
//! the provider over TLS on a pooled or fresh connection, its answer
//! copied back and counted.

use super::*;

/// One call on the route: its request (the head already read, and `rest`
/// of what came after it) checked, prepared and sent to the provider over
/// TLS, on a connection kept in `pool` from an earlier request (`via` the
/// upstream it left through) if there's one, else made with `connect`,
/// and the answer copied back as it comes; `listed` as for [`prepare`].
/// Its verdict is said first, the provider's its `to`; what its answer
/// counts is added to `counter`.
#[expect(
    clippy::too_many_arguments,
    reason = "the request, where it goes and how it's counted, each set apart by the callers"
)]
pub async fn relay_to<S, O, F, Fut>(
    mut client: S,
    head: &[u8],
    rest: Vec<u8>,
    key: &ModelKey,
    listed: bool,
    pool: &Pool<O>,
    via: &str,
    counter: &crate::bridge::usage::Counter,
    connect: F,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Verdicts + Unpin,
    O: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<O>>,
{
    let to = key.provider.clone();
    let parsed = match parse(head, key.at.is_some()) {
        Ok(parsed) => (
            parsed.length,
            crate::bridge::usage::Format::of(parsed.method, parsed.path),
        ),
        Err(why) => return refuse(&mut client, &to, &why).await,
    };
    let (length, format) = parsed;
    let body = match tokio::time::timeout(BODY_TIMEOUT, read_body(&mut client, rest, length)).await
    {
        Ok(Ok(body)) => body,
        _ => return refuse(&mut client, &to, "the request's body didn't arrive whole").await,
    };
    // A long request is parsed and written again off the async runtime.
    let prepared = if body.len() > OFF_RUNTIME {
        let (key, head) = (key.duplicate(), head.to_vec());
        tokio::task::spawn_blocking(move || prepare(&key, &head, &body, listed))
            .await
            .unwrap_or_else(|_| Err("checking the request failed".into()))
    } else {
        prepare(key, head, &body, listed)
    };
    let mut prepared = match prepared {
        Ok(prepared) => prepared,
        Err(why) => return refuse(&mut client, &to, &why).await,
    };
    if let Some(asked) = prepared.asked.take() {
        counter.asked(asked);
    }
    client.say(&Verdict::allowed(to)).await?;
    // From here, what goes back to the program is the provider's answer.
    let mut client = crate::bridge::usage::Tapped::new(client, format, counter);
    let mut connect = Some(connect);
    let kept = Pool::<O>::key(&prepared.host, via);
    let (mut origin, reused) = match pool.take(&kept) {
        Some(origin) => (origin, true),
        None => (
            reach(&mut client, &mut connect, &prepared.host).await?,
            false,
        ),
    };
    // Ended once nothing crosses either way for a while, as a tunnel is.
    let activity = link_sandbox_route::io::Activity::new();
    let call = async {
        let mut first = ask(&mut activity.watch(&mut origin), &prepared.bytes).await;
        // A connection that waited in the pool may have been closed by the
        // provider just as it was taken: it took no request, or closed at
        // once with nothing back, so the request goes once more, on a new
        // one. One closed later may have been run, and isn't sent again.
        if reused && first.unsent() {
            origin = reach(&mut client, &mut connect, &prepared.host).await?;
            first = ask(&mut activity.watch(&mut origin), &prepared.bytes).await;
        }
        let mut origin = activity.watch(&mut origin);
        let mut client = activity.watch(&mut client);
        match first.byte() {
            Some(byte) => {
                let byte = [byte];
                let mut answered = tokio::io::AsyncReadExt::chain(&byte[..], &mut origin);
                link_sandbox_route::pool::answer(&mut answered, &mut client, &prepared.host).await
            }
            // Says it didn't answer.
            None => {
                link_sandbox_route::pool::answer(&mut origin, &mut client, &prepared.host).await
            }
        }
    };
    let whole = link_sandbox_route::io::until_idle(&activity, call).await?;
    if whole {
        pool.put(kept, origin);
    }
    client.shutdown().await?;
    Ok(())
}

/// Request bodies longer than this are checked off the async runtime.
const OFF_RUNTIME: usize = 1024 * 1024;

/// A new connection to `host` with `connect`, which is used once; or
/// the program is told it couldn't be reached.
async fn reach<S, O, F, Fut>(
    client: &mut S,
    connect: &mut Option<F>,
    host: &str,
) -> anyhow::Result<O>
where
    S: AsyncWrite + Unpin,
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<O>>,
{
    let connected = match connect.take() {
        Some(connect) => connect(host.to_string()).await,
        None => Err(anyhow::anyhow!("no second connection")),
    };
    if let Err(err) = &connected {
        let body = format!("Link's sandbox: couldn't reach {host}: {err:#}\n");
        let _ = client
            .write_all(
                format!(
                    "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await;
    }
    connected
}

/// Sends `request` on `origin` and waits for its answer's first byte:
/// none when the connection closed first.
async fn ask<O: AsyncRead + AsyncWrite + Unpin>(origin: &mut O, request: &[u8]) -> Asked {
    let written = async {
        origin.write_all(request).await?;
        origin.flush().await
    };
    if written.await.is_err() {
        return Asked::Unsent;
    }
    let sent = std::time::Instant::now();
    let mut byte = [0u8];
    match origin.read(&mut byte).await {
        Ok(1..) => Asked::Answered(byte[0]),
        _ if sent.elapsed() < CLOSED_AT_ONCE => Asked::Unsent,
        _ => Asked::Closed,
    }
}

/// A connection that closes this soon after a request was written, with
/// nothing back, was closing as it was taken: the provider didn't run it.
/// One that closes later may have, and the request isn't sent again.
const CLOSED_AT_ONCE: std::time::Duration = std::time::Duration::from_millis(500);

/// What came of [`ask`].
enum Asked {
    /// The answer's first byte.
    Answered(u8),
    /// The request wasn't written, or the connection closed at once.
    Unsent,
    /// The connection closed with no answer, after a while.
    Closed,
}

impl Asked {
    fn unsent(&self) -> bool {
        matches!(self, Self::Unsent)
    }

    fn byte(&self) -> Option<u8> {
        match self {
            Self::Answered(byte) => Some(*byte),
            _ => None,
        }
    }
}

/// `length` bytes of body: `rest` and what follows it.
async fn read_body<S: AsyncRead + Unpin>(
    client: &mut S,
    mut body: Vec<u8>,
    length: usize,
) -> anyhow::Result<Vec<u8>> {
    if body.len() > length {
        bail!("more than one request on the connection");
    }
    body.reserve(length - body.len());
    let mut chunk = vec![0u8; 64 * 1024];
    while body.len() < length {
        let want = (length - body.len()).min(chunk.len());
        let n = client.read(&mut chunk[..want]).await?;
        if n == 0 {
            bail!("the connection closed mid-body");
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(body)
}

/// Refuses the call, `why`: its verdict, to `to`, then a 403 saying why.
async fn refuse<S: AsyncWrite + Verdicts + Unpin>(
    client: &mut S,
    to: &str,
    why: &str,
) -> anyhow::Result<()> {
    let verdict = Verdict::refused(to);
    client.say(&verdict).await?;
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
    bail!("refused: {why}")
}

#[cfg(test)]
mod tests {
    use super::super::tests::{key, wired};
    use super::*;
    use link_format::WithinExt;
    use serde_json::json;

    /// Where the tests that aren't about tokens count them.
    static UNCOUNTED: crate::bridge::usage::Counter = crate::bridge::usage::Counter::new();

    #[tokio::test]
    async fn a_request_reaches_the_provider_with_its_key_and_the_answer_comes_back() {
        let (mut program, wire) = tokio::io::duplex(1 << 20);
        let (provider_side, mut provider) = tokio::io::duplex(1 << 20);
        let body = br#"{"model":"m","tools":[{"type":"web_search_20260209","name":"web_search"}]}"#;
        let head = format!(
            "POST /_link/model/v1/messages HTTP/1.1\r\nx-api-key: sk-ant-api03-link-wire\r\n\
             Content-Length: {}\r\n\r\n",
            body.len()
        );
        // The first bytes of the body come with the head; the rest later.
        let (first, later) = body.split_at(10);
        let key = key("anthropic");
        let head_bytes = wired(head.as_bytes());
        let wire = tokio::spawn(async move {
            let pool = Pool::new();
            relay_to(
                link_sandbox_route::Unsaid(wire),
                &head_bytes,
                first.to_vec(),
                &key,
                false,
                &pool,
                "direct",
                &UNCOUNTED,
                |host| async move {
                    assert_eq!(host, "api.anthropic.com");
                    Ok(provider_side)
                },
            )
            .await
        });
        program.write_all(later).await.unwrap();
        let mut got = vec![0u8; 4096];
        let n = provider.read(&mut got).await.unwrap();
        let got = String::from_utf8_lossy(&got[..n]).to_string();
        assert!(got.contains("x-api-key: sk-real-key\r\n"), "{got}");
        assert!(!got.contains("link-wire"), "{got}");
        assert!(got.ends_with(r#"{"model":"m"}"#), "{got}");
        provider
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .await
            .unwrap();
        drop(provider);
        let mut answer = String::new();
        program.read_to_string(&mut answer).await.unwrap();
        assert!(answer.ends_with("\r\n\r\nok"), "{answer}");
        wire.await.unwrap().unwrap();
    }

    /// A model's answer through the verb: asked uncompressed, passed on
    /// as it came, its usage counted, and not a word of it kept.
    #[tokio::test]
    async fn a_models_answer_is_counted_as_it_passes() {
        let counter = crate::bridge::usage::Counter::new();
        let (mut program, wire) = tokio::io::duplex(1 << 20);
        let (provider_side, mut provider) = tokio::io::duplex(1 << 20);
        let phrase = "the purple walrus sings at noon";
        let body = format!(
            r#"{{"model":"m","stream":true,"messages":[{{"role":"user","content":"{phrase}"}}]}}"#
        );
        let head = format!(
            "POST /_link/model/v1/messages HTTP/1.1\r\nAccept-Encoding: gzip, br\r\n\
             Content-Length: {}\r\n\r\n",
            body.len()
        );
        let head = wired(head.as_bytes());
        let key = key("anthropic");
        let events = [
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 12,
                "cache_read_input_tokens": 30, "output_tokens": 1}}}),
            json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": phrase}}),
            json!({"type": "message_delta", "usage": {"output_tokens": 8}}),
        ];
        let sse: String = events.iter().map(|e| format!("data: {e}\n\n")).collect();
        let answer = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n\
             {:x}\r\n{sse}\r\n0\r\n\r\n",
            sse.len()
        );
        let provider_answers = async {
            // The request as the proxy writes it again, read by its
            // framing (its head, then its Content-Length), never by how
            // its body's keys happen to be ordered, and given ten seconds.
            let mut seen = Vec::new();
            let mut buf = vec![0u8; 4096];
            let read = async {
                loop {
                    if let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&seen[..end]).to_lowercase();
                        let length: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse().ok())
                            .expect("the request's length");
                        if seen.len() >= end + 4 + length {
                            break;
                        }
                    }
                    let n = provider.read(&mut buf).await.unwrap();
                    assert!(n > 0, "the request ended early");
                    seen.extend_from_slice(&buf[..n]);
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(10), read)
                .await
                .expect("the whole request within ten seconds");
            let end = seen.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
            let sent: serde_json::Value = serde_json::from_slice(&seen[end + 4..]).unwrap();
            assert_eq!(sent["stream"], true, "{sent}");
            assert_eq!(sent["messages"][0]["content"], phrase, "{sent}");
            // Split mid-event, as a stream comes.
            let (a, b) = answer.as_bytes().split_at(answer.len() / 2);
            provider.write_all(a).await.unwrap();
            provider.write_all(b).await.unwrap();
            String::from_utf8(seen).unwrap()
        };
        let pool = Pool::new();
        let relayed = relay_to(
            link_sandbox_route::Unsaid(wire),
            &head,
            body.as_bytes().to_vec(),
            &key,
            false,
            &pool,
            "direct",
            &counter,
            |_| async { Ok(provider_side) },
        );
        let (sent, relayed) = tokio::join!(provider_answers, relayed);
        relayed.unwrap();
        assert!(sent.contains("Accept-Encoding: identity\r\n"), "{sent}");
        assert!(!sent.contains("gzip"), "{sent}");
        let mut got = String::new();
        program.read_to_string(&mut got).await.unwrap();
        assert!(
            got.ends_with(&answer[answer.find("\r\n\r\n").unwrap() + 4..]),
            "{got}"
        );
        let counted = counter.now();
        assert_eq!(
            counted,
            link_measure_contract::client::Tokens {
                requests: 1,
                unmetered: 0,
                input: 42,
                output: 8,
                cached: 30,
                written: 0,
            }
        );
        assert!(!format!("{counted:?}").contains("walrus"));
    }

    /// A provider that takes the request and never answers holds no
    /// connection past the idle time.
    #[tokio::test]
    async fn a_silent_provider_ends_when_idle() {
        let (_program, wire) = tokio::io::duplex(1 << 20);
        let (origin, _provider) = tokio::io::duplex(1 << 20);
        let head = wired(b"POST /_link/model/v1/messages HTTP/1.1\r\nContent-Length: 2\r\n\r\n");
        let pool = Pool::new();
        let key = key("anthropic");
        let relayed = link_sandbox_route::io::TEST_IDLE.scope(
            std::time::Duration::from_millis(300),
            relay_to(
                link_sandbox_route::Unsaid(wire),
                &head,
                b"{}".to_vec(),
                &key,
                false,
                &pool,
                "direct",
                &UNCOUNTED,
                |_| async { Ok(origin) },
            ),
        );
        let ended = tokio::time::timeout(std::time::Duration::from_secs(5), relayed)
            .await
            .expect("a silent provider held the request open");
        assert!(format!("{:#}", ended.unwrap_err()).contains("nothing crossed"));
    }

    #[tokio::test]
    async fn the_next_request_goes_on_the_same_connection() {
        let (origin, mut provider) = tokio::io::duplex(1 << 20);
        let origin = std::sync::Mutex::new(Some(origin));
        let pool = Pool::new();
        // The provider answers each request on its one connection, the
        // second in chunks, as a stream comes.
        let answering = tokio::spawn(async move {
            let mut seen = Vec::new();
            let mut buf = vec![0u8; 4096];
            for answer in [
                &b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\none"[..],
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\ntwo\r\n0\r\n\r\n",
            ] {
                while !seen.ends_with(b"{}") {
                    let n = provider.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    seen.extend_from_slice(&buf[..n]);
                }
                seen.clear();
                provider.write_all(answer).await.unwrap();
            }
            provider
        });
        let head = wired(b"POST /_link/model/v1/messages HTTP/1.1\r\nContent-Length: 2\r\n\r\n");
        for want in ["one", "3\r\ntwo\r\n0\r\n\r\n"] {
            let (mut program, wire) = tokio::io::duplex(1 << 20);
            let connect = |_| async { origin.lock().unwrap().take().within("dialed twice") };
            relay_to(
                link_sandbox_route::Unsaid(wire),
                &head,
                b"{}".to_vec(),
                &key("anthropic"),
                false,
                &pool,
                "direct",
                &UNCOUNTED,
                connect,
            )
            .await
            .unwrap();
            let mut answer = String::new();
            program.read_to_string(&mut answer).await.unwrap();
            assert!(answer.contains("Connection: close\r\n"), "{answer}");
            assert!(answer.ends_with(want), "{answer}");
        }
        let _provider = answering.await.unwrap();
    }

    /// A pooled connection the provider closed as the request went out
    /// (it waited too long) costs nothing: the request goes again, once,
    /// on a new connection.
    #[tokio::test]
    async fn a_pooled_connection_that_closed_is_tried_again_fresh() {
        let (first, mut dying) = tokio::io::duplex(1 << 20);
        let (second, mut fresh) = tokio::io::duplex(1 << 20);
        let origins = std::sync::Mutex::new(vec![second, first]);
        let pool = Pool::new();
        let read_request = |seen: &mut Vec<u8>, n: usize, buf: &[u8]| {
            seen.extend_from_slice(&buf[..n]);
            seen.ends_with(b"{}")
        };
        let answering = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            // The first connection answers once, reads the next request,
            // and closes.
            for answer in [&b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\none"[..], b""] {
                let mut seen = Vec::new();
                loop {
                    let n = dying.read(&mut buf).await.unwrap();
                    if read_request(&mut seen, n, &buf) {
                        break;
                    }
                }
                dying.write_all(answer).await.unwrap();
            }
            drop(dying);
            // The new one answers the request again.
            let mut seen = Vec::new();
            loop {
                let n = fresh.read(&mut buf).await.unwrap();
                if read_request(&mut seen, n, &buf) {
                    break;
                }
            }
            fresh
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\ntwo")
                .await
                .unwrap();
            fresh
        });
        let head = wired(b"POST /_link/model/v1/messages HTTP/1.1\r\nContent-Length: 2\r\n\r\n");
        for want in ["one", "two"] {
            let (mut program, wire) = tokio::io::duplex(1 << 20);
            let connect = |_| async { origins.lock().unwrap().pop().within("dialed too often") };
            relay_to(
                link_sandbox_route::Unsaid(wire),
                &head,
                b"{}".to_vec(),
                &key("anthropic"),
                false,
                &pool,
                "direct",
                &UNCOUNTED,
                connect,
            )
            .await
            .unwrap();
            let mut answer = String::new();
            program.read_to_string(&mut answer).await.unwrap();
            assert!(answer.ends_with(want), "{answer}");
        }
        let _provider = answering.await.unwrap();
    }

    /// A pooled connection that took the request and closed a while
    /// later, with no answer, may have run it: it isn't sent again.
    #[tokio::test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    async fn a_request_the_provider_may_have_run_isnt_sent_again() {
        let (first, mut dying) = tokio::io::duplex(1 << 20);
        let origins = std::sync::Mutex::new(vec![first]);
        let dials = std::sync::atomic::AtomicUsize::new(0);
        let pool = Pool::new();
        let answering = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            let mut requests = 0;
            for answer in [&b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\none"[..], b""] {
                let mut seen = Vec::new();
                while !seen.ends_with(b"{}") {
                    let n = dying.read(&mut buf).await.unwrap();
                    seen.extend_from_slice(&buf[..n]);
                }
                requests += 1;
                if answer.is_empty() {
                    tokio::time::sleep(CLOSED_AT_ONCE * 2).await;
                }
                dying.write_all(answer).await.unwrap();
            }
            requests
        });
        let head = wired(b"POST /_link/model/v1/messages HTTP/1.1\r\nContent-Length: 2\r\n\r\n");
        for _ in 0..2 {
            let (mut program, wire) = tokio::io::duplex(1 << 20);
            let connect = |_| async {
                dials.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                origins.lock().unwrap().pop().within("dialed again")
            };
            let _ = relay_to(
                link_sandbox_route::Unsaid(wire),
                &head,
                b"{}".to_vec(),
                &key("anthropic"),
                false,
                &pool,
                "direct",
                &UNCOUNTED,
                connect,
            )
            .await;
            let mut answer = String::new();
            let _ = program.read_to_string(&mut answer).await;
        }
        // Two requests on the one connection, and no second dialed.
        assert_eq!(answering.await.unwrap(), 2);
        assert_eq!(dials.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
}

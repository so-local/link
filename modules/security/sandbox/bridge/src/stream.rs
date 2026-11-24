//! A call that is a stream: for a bridge whose answer is bytes as they
//! come (a model's answer, a page's), not one line.
//!
//! It starts as every call does: the bridge's challenge, the caller's
//! [`Proven`](crate::Proven) and its action, each a JSON line. From then
//! on the caller sends what the call carries (a request's body) as it
//! comes, without waiting; the bridge answers one JSON line, its
//! verdict (its own type, or the error envelope, [`Failure`]), then
//! what it answers, as it comes. Either side ends the call by closing.
//!
//! The bridge's side is [`serve_streams`](crate::serve_streams): its
//! handler is given a [`Stream`], which writes nothing before its
//! verdict. The caller's is [`open`], then [`verdict`].

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncWrite, BufReader, ReadBuf};

use crate::serve::{Places, busy, envelope, peer_of};
use crate::take::{line_async, prove, write_async};
use crate::{Binding, Failure, Identity, Keys, Why, parsed};

/// The most a verdict's line is.
pub const VERDICT_MAX: u64 = 64 * 1024;

/// A call's stream, on the bridge's side: what the caller sends after
/// its action, and where the bridge answers, once it has written its
/// verdict ([`Stream::verdict`]).
pub struct Stream {
    /// What was read past its action, read before the rest.
    early: Vec<u8>,
    conn: tokio::net::UnixStream,
    /// Its verdict is written: from then on what it writes is its answer,
    /// and a failure is no longer answered with the envelope.
    said: Arc<AtomicBool>,
}

impl Stream {
    /// Writes `verdict`, the bridge's own answer to the call, before
    /// anything else it answers.
    pub async fn verdict(&mut self, verdict: &impl Serialize) -> std::io::Result<()> {
        if self.said.swap(true, Ordering::SeqCst) {
            return Err(std::io::Error::other(
                "the call's verdict is written already",
            ));
        }
        write_async(&mut self.conn, verdict).await
    }

    /// Whether its verdict is written.
    pub fn said(&self) -> bool {
        self.said.load(Ordering::SeqCst)
    }

    /// Its socket, to ask whether its caller hung up.
    pub fn socket(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.conn.as_raw_fd()
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if !self.early.is_empty() {
            let n = self.early.len().min(buf.remaining());
            buf.put_slice(&self.early[..n]);
            self.early.drain(..n);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.conn).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if !self.said() {
            return Poll::Ready(Err(std::io::Error::other(
                "a stream's answer comes after its verdict",
            )));
        }
        Pin::new(&mut self.conn).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.conn).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.conn).poll_shutdown(cx)
    }
}

/// One stream call on `stream`, proved, held and handed to `handle`.
pub(crate) async fn one<A, F, Fut>(
    stream: tokio::net::UnixStream,
    keys: &Arc<dyn Keys>,
    places: &Places,
    handle: &F,
) where
    A: DeserializeOwned,
    F: Fn(u32, Identity, A, Stream) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let peer = peer_of(&stream);
    let keys: Arc<dyn Keys> = Arc::new(crate::PeerKeys {
        keys: keys.clone(),
        peer,
    });
    let mut stream = stream;
    let limits = &places.limits;
    let (read, mut write) = stream.split();
    let mut read = BufReader::new(read);
    let taken = async {
        let waiting = places.wait(peer).ok_or_else(|| busy(limits))?;
        let who = tokio::select! {
            who = prove(&mut read, &mut write, &keys) => who?,
            () = waiting.closed() => return Err(busy(limits)),
        };
        drop(waiting);
        let held = places.hold(&who).ok_or_else(|| busy(limits))?;
        let action = tokio::time::timeout(
            limits.action_wait,
            line_async(&mut read, limits.action_max, "the call's action"),
        )
        .await
        .map_err(|_| Failure::new(Why::Refused, "the call's action didn't come in time"))??;
        let action: A = parsed(&action, "the call's action")?;
        Ok((who, action, held))
    };
    let (who, action, _held) = match taken.await {
        Ok(taken) => taken,
        Err(failure) => {
            let _ = tokio::time::timeout(
                limits.answer_wait,
                write_async(&mut write, &envelope(&failure)),
            )
            .await;
            return;
        }
    };
    let early = read.buffer().to_vec();
    // A second handle on the connection, for the envelope of a failure
    // before its verdict, once the handler has let its own go.
    let spare = {
        use std::os::fd::AsFd;
        stream
            .as_fd()
            .try_clone_to_owned()
            .ok()
            .map(std::os::unix::net::UnixStream::from)
    };
    let said = Arc::new(AtomicBool::new(false));
    let stream = Stream {
        early,
        conn: stream,
        said: said.clone(),
    };
    if let Err(err) = handle(peer, who, action, stream).await
        && !said.load(Ordering::SeqCst)
        && let Some(mut spare) = spare
    {
        use std::io::Write;
        let mut line = serde_json::to_vec(&envelope(&Failure::of(err))).unwrap_or_default();
        line.push(b'\n');
        // Small, on a connection nothing else was written to: it fits.
        let _ = spare.write_all(&line);
    }
}

/// Answers a stream call `failure`, the error envelope, as its verdict:
/// the call refused, nothing done.
pub async fn refuse(stream: &mut Stream, failure: &Failure) -> std::io::Result<()> {
    stream.verdict(&envelope(failure)).await
}

/// The caller's side: proves who calls with `binding` and sends
/// `action` on `stream`; what the call carries then follows, as it
/// comes, and the bridge's verdict is read with [`verdict`].
pub async fn open<A: Serialize>(
    stream: &mut tokio::net::UnixStream,
    binding: &Binding,
    action: &A,
) -> anyhow::Result<()> {
    crate::call::call_async(stream, binding, action).await
}

/// A stream call's verdict on `read`, the bridge's side of it: its own,
/// or its failure as an error. Read a byte at a time, so what it answers
/// after is left on `read`.
pub async fn verdict<R, S>(read: &mut S) -> anyhow::Result<R>
where
    R: DeserializeOwned,
    S: AsyncRead + Unpin,
{
    use anyhow::Context;
    use tokio::io::AsyncReadExt;
    let mut line = Vec::new();
    let mut byte = [0u8];
    while (line.len() as u64) < VERDICT_MAX {
        if read
            .read(&mut byte)
            .await
            .context("reading the bridge's verdict")?
            == 0
            || byte[0] == b'\n'
        {
            break;
        }
        line.push(byte[0]);
    }
    crate::call::answer_of(&line)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::Limits;

    const LIMITS: Limits = Limits {
        name: "the test's stream bridge",
        calls: 4,
        per_agent: 2,
        per_owner: 2,
        action_max: 1 << 20,
        action_wait: Duration::from_secs(3),
        answer_wait: Duration::from_secs(3),
    };

    fn agent(n: usize) -> Binding {
        Binding {
            identity: Identity {
                agent: format!("agent{n:07}"),
                service: String::new(),
                owner: format!("acct-{n}"),
                teams: Vec::new(),
            },
            key: format!("{n:0>32}").into(),
        }
    }

    struct Bound(Vec<Binding>);

    impl Keys for Bound {
        fn key_of(&self, identity: &Identity) -> Option<crate::Secret> {
            self.0.iter().find_map(|b| b.key_of(identity))
        }
    }

    /// A stream bridge on its own thread at `socket`: its verdict says who
    /// called and what it asked; an action `"echo"` then answers what's
    /// sent, upper-cased, until the caller stops sending, and `"fail"`
    /// fails before any verdict.
    fn bridge(socket: &std::path::Path, agents: &[Binding]) -> Arc<AtomicUsize> {
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let keys: Arc<dyn Keys> = Arc::new(Bound(agents.iter().map(Binding::duplicate).collect()));
        let ran = Arc::new(AtomicUsize::new(0));
        let running = ran.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::UnixListener::from_std(listener).unwrap();
                let stop = AtomicBool::new(false);
                crate::serve_streams(
                    listener,
                    keys,
                    &LIMITS,
                    &stop,
                    |who, action: serde_json::Value, mut stream| {
                        let running = running.clone();
                        async move {
                            running.fetch_add(1, Ordering::SeqCst);
                            if action == "fail" {
                                anyhow::bail!("it failed, as asked");
                            }
                            let said = serde_json::json!({"who": who.agent, "asked": action});
                            stream.verdict(&said).await?;
                            if action == "echo" {
                                let mut sent = Vec::new();
                                stream.read_to_end(&mut sent).await?;
                                stream.write_all(&sent.to_ascii_uppercase()).await?;
                                stream.shutdown().await?;
                            }
                            Ok(())
                        }
                    },
                )
                .await;
            });
        });
        ran
    }

    fn socket(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lnk-stream-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("bridge.sock")
    }

    #[test]
    fn a_stream_bridge_passes_the_test_every_bridge_passes() {
        let socket = socket("conformance");
        let agents: Vec<Binding> = (1..=4).map(agent).collect();
        let ran = bridge(&socket, &agents);
        crate::conformance::check(&crate::conformance::Subject {
            socket: socket.clone(),
            agents,
            action: serde_json::json!({"say": "hello"}),
            ran: Arc::new(move || ran.load(Ordering::SeqCst)),
            big: None,
            limits: LIMITS,
        });
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    #[tokio::test]
    async fn a_stream_carries_both_ways_after_its_verdict() {
        let socket = socket("echo");
        bridge(&socket, &[agent(1)]);
        let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
        open(&mut stream, &agent(1), &"echo").await.unwrap();
        // Sent before its verdict is read: a call's body comes as it comes.
        stream.write_all(b"a request's body").await.unwrap();
        stream.shutdown().await.unwrap();
        let said: serde_json::Value = verdict(&mut stream).await.unwrap();
        assert_eq!(
            said,
            serde_json::json!({"who": "agent0000001", "asked": "echo"})
        );
        let mut answered = String::new();
        stream.read_to_string(&mut answered).await.unwrap();
        assert_eq!(answered, "A REQUEST'S BODY");

        // A failure before its verdict is the envelope, as for any call.
        let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
        open(&mut stream, &agent(1), &"fail").await.unwrap();
        let err = verdict::<serde_json::Value, _>(&mut stream)
            .await
            .unwrap_err();
        let failure = err
            .downcast_ref::<Failure>()
            .unwrap_or_else(|| panic!("{err:#}"));
        assert!(failure.error.contains("it failed, as asked"), "{failure:?}");
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }
}

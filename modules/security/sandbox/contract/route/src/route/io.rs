//! Bytes across a call: joined both ways, watched for when they last
//! crossed, and ended when nothing has for a while.

use anyhow::bail;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long a call may carry nothing either way before the proxy ends
/// it: a tunnel (`CONNECT`), a plain `http://` request, a hosted or
/// local model's, or a call to a connection's service.
pub const IDLE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

tokio::task_local! {
    /// A test's own [`IDLE`], for the calls it runs within its scope:
    /// set only by tests, here and in the routes' programs.
    pub static TEST_IDLE: std::time::Duration;
}

/// [`IDLE`]; in a test, its own within [`TEST_IDLE`]'s scope.
pub fn idle() -> std::time::Duration {
    if let Ok(idle) = TEST_IDLE.try_with(|idle| *idle) {
        return idle;
    }
    IDLE
}

/// `call`, over streams `activity` watches, ended once nothing crosses
/// them for [`idle`]: a slow or silent server, or client, holds none of
/// the proxy's connections for long.
pub async fn until_idle<T>(
    activity: &Activity,
    call: impl std::future::Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    let idle = idle();
    tokio::select! {
        done = call => done,
        () = activity.idle(idle) => bail!("ended: nothing crossed for {}s", idle.as_secs()),
    }
}

/// When anything last crossed a call's streams, as each [`Watched`] one
/// sees it: how long after `since`, in milliseconds. One timer watches
/// it, not one per read.
pub struct Activity {
    since: std::time::Instant,
    last: std::sync::atomic::AtomicU64,
}

impl Default for Activity {
    fn default() -> Self {
        Self::new()
    }
}

impl Activity {
    pub fn new() -> Self {
        Self {
            since: std::time::Instant::now(),
            last: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// `stream`, noting here each time bytes cross it.
    pub fn watch<'a, S>(&'a self, stream: &'a mut S) -> Watched<'a, S> {
        Watched {
            stream,
            activity: self,
        }
    }

    fn now(&self) -> u64 {
        self.since.elapsed().as_millis() as u64
    }

    fn crossed(&self) {
        self.last
            .store(self.now(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns once nothing has crossed for `idle`.
    #[expect(clippy::disallowed_methods, reason = "the idle watch's own period")]
    pub async fn idle(&self, idle: std::time::Duration) {
        let tick = idle.min(std::time::Duration::from_secs(5));
        let idle = idle.as_millis() as u64;
        loop {
            tokio::time::sleep(tick).await;
            let last = self.last.load(std::sync::atomic::Ordering::Relaxed);
            if self.now().saturating_sub(last) >= idle {
                return;
            }
        }
    }
}

/// A stream whose reads and writes an [`Activity`] notes.
pub struct Watched<'a, S> {
    stream: &'a mut S,
    activity: &'a Activity,
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Watched<'_, S> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let polled = std::pin::Pin::new(&mut *this.stream).poll_read(cx, buf);
        if buf.filled().len() > before {
            this.activity.crossed();
        }
        polled
    }
}

impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for Watched<'_, S> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let polled = std::pin::Pin::new(&mut *this.stream).poll_write(cx, buf);
        if let std::task::Poll::Ready(Ok(n)) = &polled
            && *n > 0
        {
            this.activity.crossed();
        }
        polled
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.get_mut().stream).poll_shutdown(cx)
    }
}

/// Joins `a` and `b` both ways until both ends are done, or until nothing
/// has crossed either way for `idle`.
pub async fn joined_until_idle<A, B>(a: A, b: B, idle: std::time::Duration) -> anyhow::Result<()>
where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let activity = Activity::new();
    let (mut a_in, mut a_out) = tokio::io::split(a);
    let (mut b_in, mut b_out) = tokio::io::split(b);
    let both = async {
        tokio::try_join!(
            pump(&mut a_in, &mut b_out, &activity),
            pump(&mut b_in, &mut a_out, &activity)
        )
    };
    tokio::select! {
        done = both => done.map(|_| ()),
        () = activity.idle(idle) => bail!("ended: nothing crossed for {}s", idle.as_secs()),
    }
}

/// One way of [`joined_until_idle`]: `from` to `to`, each read noted in
/// `activity`.
async fn pump<R, W>(from: &mut R, to: &mut W, activity: &Activity) -> anyhow::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; PIPE_BUFFER];
    loop {
        let n = from.read(&mut buf).await?;
        if n == 0 {
            let _ = to.shutdown().await;
            return Ok(());
        }
        to.write_all(&buf[..n]).await?;
        activity.crossed();
    }
}

/// A TCP connection to `addr`, which sends each write at once: the proxy
/// passes on what it reads as it comes, so Nagle's algorithm would only
/// hold back a request's last bytes, or a streamed answer's.
pub async fn connect_tcp<A: tokio::net::ToSocketAddrs>(addr: A) -> std::io::Result<TcpStream> {
    let stream = TcpStream::connect(addr).await?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

/// What each side of a [`pipe`] reads at once.
const PIPE_BUFFER: usize = 64 * 1024;

/// Copies between `a` and `b` both ways until both are done, 64 KiB at a
/// time: a download crosses two or three of these.
pub async fn pipe<A, B>(a: &mut A, b: &mut B) -> std::io::Result<(u64, u64)>
where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + ?Sized,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + ?Sized,
{
    tokio::io::copy_bidirectional_with_sizes(a, b, PIPE_BUFFER, PIPE_BUFFER).await
}

/// Whether `socket`'s peer hung up (closed it, or shut both its sides),
/// or it failed: never waits. Linux sets a Unix socket's hang-up only
/// once its peer closed it, or shut both its sides: shutting its writing
/// alone only ends what it reads.
pub fn hung_up(socket: std::os::fd::RawFd) -> bool {
    let mut poll = libc::pollfd {
        fd: socket,
        events: 0,
        revents: 0,
    };
    // SAFETY: one pollfd, valid for the call, and a timeout of 0. A
    // hang-up, an error and a closed descriptor are reported unasked.
    let ready = unsafe { libc::poll(&mut poll, 1, 0) };
    ready > 0 && poll.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
}

/// How often a peer that shut its writing is checked for leaving.
pub const HANGUP_CHECK: std::time::Duration = std::time::Duration::from_millis(250);

/// Once `socket`'s peer hung up ([`hung_up`]), checked every
/// [`HANGUP_CHECK`].
#[expect(
    clippy::disallowed_methods,
    reason = "polls a socket's hang-up, which nothing announces"
)]
pub async fn until_hung_up(socket: std::os::fd::RawFd) {
    while !hung_up(socket) {
        tokio::time::sleep(HANGUP_CHECK).await;
    }
}

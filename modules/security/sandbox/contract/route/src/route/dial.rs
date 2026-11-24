//! Dialing out: straight, or through an upstream (a SOCKS exit, an HTTP
//! proxy), each address in turn. The proxy dials by its rules on it;
//! a bridge dials its one host ([`dial_out`]).

use std::net::{IpAddr, SocketAddr};

use crate::net::Upstream;
use anyhow::{Context, bail, ensure};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UnixStream};

use super::http::{AnswerHead, MAX_HEAD};
use super::io::connect_tcp;
use super::resolve::{forget, prefer, resolve};

/// How long connecting to one address may take (through a SOCKS
/// upstream: its whole handshake) before the next is tried.
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The upstream `text` names ([`Upstream::parse`]), as long-lived as the
/// process: each one a bridge's calls name is kept once, and they are
/// few (a machine's exits).
pub fn upstream(text: &str) -> anyhow::Result<&'static Upstream> {
    static KEPT: std::sync::Mutex<Vec<&'static Upstream>> = std::sync::Mutex::new(Vec::new());
    let upstream = Upstream::parse(text)?;
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = kept.iter().find(|k| ***k == upstream) {
        return Ok(found);
    }
    ensure!(
        kept.len() < 64,
        "a bridge's calls named more than 64 upstreams"
    );
    let leaked: &'static Upstream = Box::leak(Box::new(upstream));
    kept.push(leaked);
    Ok(leaked)
}

/// Link's own connection to `host`, one host a bridge reaches for its
/// call (a model's provider, a secret's service): resolved here, never
/// this machine or its network by any address ([`crate::net::forbidden`],
/// [`crate::net::this_machine`]), and reached straight or through
/// `upstream`, the sandbox's, as [`connect_any`] does.
pub async fn dial_out(
    host: &str,
    port: u16,
    upstream: &'static Upstream,
) -> anyhow::Result<Dialed> {
    let allowed = |ip: IpAddr| !crate::net::forbidden(ip) && !crate::net::this_machine(ip);
    if let Upstream::Http(proxy) = upstream
        && host.parse::<IpAddr>().is_err()
    {
        let connecting = async {
            let stream = connect_tcp(*proxy).await.context(UPSTREAM_GONE)?;
            http_connect(stream, host, port).await
        };
        return tokio::time::timeout(CONNECT_TIMEOUT, connecting)
            .await
            .unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "{host} didn't answer within {} seconds",
                    CONNECT_TIMEOUT.as_secs()
                ))
            })
            .map(Dialed::Tcp)
            .with_context(|| format!("connecting to {host}"));
    }
    let addrs = resolve(host, port).await?;
    let allowed: Vec<(SocketAddr, &'static Upstream)> = addrs
        .iter()
        .filter(|a| allowed(a.ip()))
        .map(|a| (*a, upstream))
        .collect();
    ensure!(
        !allowed.is_empty(),
        "refused: {host} is this machine, its network, or nothing reachable"
    );
    match connect_any(&allowed).await {
        Ok((stream, addr)) => {
            prefer(host, addr.ip());
            Ok(stream)
        }
        Err(err) => {
            forget(host);
            Err(err.context(format!("connecting to {host}")))
        }
    }
}

/// How long an address has to answer before the next is tried as well.
const STAGGER: std::time::Duration = std::time::Duration::from_millis(250);

/// The same, through a SOCKS exit: its answer is a round trip to the
/// exit (a home, maybe far) and the exit's own connect, longer than
/// [`STAGGER`], and a second try meanwhile is wasted.
const EXIT_STAGGER: std::time::Duration = std::time::Duration::from_secs(1);

/// The first of `addrs` to answer within [`CONNECT_TIMEOUT`], each
/// through its own upstream, and which it was. Each is tried [`STAGGER`]
/// ([`EXIT_STAGGER`] through an exit) after the one before, IPv6 and
/// IPv4 in turn, while the earlier ones still may answer: a family that
/// goes nowhere costs a quarter second (a second through an exit), not
/// ten.
#[expect(
    clippy::disallowed_methods,
    reason = "staggers its attempts, as Happy Eyeballs does"
)]
pub async fn connect_any(
    addrs: &[(SocketAddr, &'static Upstream)],
) -> anyhow::Result<(Dialed, SocketAddr)> {
    let only: Vec<SocketAddr> = addrs.iter().map(|(a, _)| *a).collect();
    let mut next = interleaved(&only)
        .into_iter()
        .filter_map(|a| addrs.iter().find(|(b, _)| *b == a).copied());
    let stagger = match addrs.iter().all(|(_, up)| matches!(up, Upstream::Direct)) {
        true => STAGGER,
        false => EXIT_STAGGER,
    };
    let mut trying = tokio::task::JoinSet::new();
    let start = |trying: &mut tokio::task::JoinSet<_>,
                 (addr, upstream): (SocketAddr, &'static Upstream)| {
        trying.spawn(async move {
            let connecting = async {
                match upstream {
                    Upstream::Direct => Ok(Dialed::Tcp(connect_tcp(addr).await?)),
                    Upstream::Socks5(proxy) => {
                        let stream = connect_tcp(proxy).await.context(EXIT_GONE)?;
                        Ok(Dialed::Tcp(socks5(stream, addr).await?))
                    }
                    Upstream::Socks5Unix(path) => {
                        let stream = trusted_unix(path).await.context(EXIT_GONE)?;
                        Ok(Dialed::Unix(socks5(stream, addr).await?))
                    }
                    Upstream::Http(proxy) => {
                        let stream = connect_tcp(proxy).await.context(UPSTREAM_GONE)?;
                        let ip = match addr.ip() {
                            IpAddr::V4(ip) => ip.to_string(),
                            IpAddr::V6(ip) => format!("[{ip}]"),
                        };
                        Ok(Dialed::Tcp(http_connect(stream, &ip, addr.port()).await?))
                    }
                }
            };
            let connected = tokio::time::timeout(CONNECT_TIMEOUT, connecting)
                .await
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "{addr} didn't answer within {} seconds",
                        CONNECT_TIMEOUT.as_secs()
                    ))
                });
            (addr, connected)
        });
    };
    let mut failed = None;
    loop {
        if trying.is_empty() {
            match next.next() {
                Some(addr) => start(&mut trying, addr),
                None => break,
            }
        }
        tokio::select! {
            Some(done) = trying.join_next() => match done {
                Ok((addr, Ok(stream))) => return Ok((stream, addr)),
                Ok((_, Err(err))) => failed = Some(err),
                Err(_) => {}
            },
            _ = tokio::time::sleep(stagger) => {
                if let Some(addr) = next.next() {
                    start(&mut trying, addr);
                }
            }
        }
    }
    Err(failed.unwrap_or_else(|| anyhow::anyhow!("no address to connect to")))
}

/// An upstream socket refused for not being this user's or root's: the
/// one failure the wire's log says the reason of, its socket's path
/// being the machine's, never a sandbox's request.
#[derive(Debug)]
pub struct UntrustedUpstream(pub String);

impl std::fmt::Display for UntrustedUpstream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UntrustedUpstream {}

/// A local upstream is this user's or root's socket, and must answer as
/// one of them too. On Linux the opened inode pins the connection across
/// path replacement. The peer check also holds on systems that connect
/// by path, where the name can change between stat and connect.
async fn trusted_unix(path: &std::path::Path) -> anyhow::Result<UnixStream> {
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    let trusted = |uid| uid == me || uid == 0;
    let socket = link_files::nofollow::socket(path, trusted).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => anyhow::Error::new(UntrustedUpstream(format!(
            "{} isn't an upstream socket owned by this user or root",
            path.display()
        ))),
        _ => e.into(),
    })?;
    let stream = UnixStream::connect(&socket.at).await?;
    if !trusted(stream.peer_cred()?.uid()) {
        return Err(UntrustedUpstream(format!(
            "{} isn't served by this user or root",
            path.display()
        ))
        .into());
    }
    Ok(stream)
}

/// `addrs` with the families taking turns, starting with the first's,
/// each family's in its own order.
pub fn interleaved(addrs: &[SocketAddr]) -> Vec<SocketAddr> {
    let first_v6 = addrs.first().is_some_and(SocketAddr::is_ipv6);
    let (mut a, mut b): (Vec<SocketAddr>, Vec<SocketAddr>) =
        addrs.iter().partition(|a| a.is_ipv6() == first_v6);
    a.reverse();
    b.reverse();
    let mut out = Vec::with_capacity(addrs.len());
    while !a.is_empty() || !b.is_empty() {
        out.extend(a.pop());
        out.extend(b.pop());
    }
    out
}

/// Why a SOCKS upstream can't be connected to.
const EXIT_GONE: &str = "the SOCKS upstream isn't there (is the machine offering it on?)";

/// A connection the proxy dialed: straight to its host, or through a
/// SOCKS upstream on a TCP port or a Unix socket.
pub enum Dialed {
    Tcp(TcpStream),
    Unix(UnixStream),
}

impl tokio::io::AsyncRead for Dialed {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Dialed::Tcp(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            Dialed::Unix(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for Dialed {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Dialed::Tcp(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            Dialed::Unix(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    // TLS writes its records in one go.
    fn poll_write_vectored(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Dialed::Tcp(s) => std::pin::Pin::new(s).poll_write_vectored(cx, bufs),
            Dialed::Unix(s) => std::pin::Pin::new(s).poll_write_vectored(cx, bufs),
        }
    }

    fn is_write_vectored(&self) -> bool {
        match self {
            Dialed::Tcp(s) => s.is_write_vectored(),
            Dialed::Unix(s) => s.is_write_vectored(),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Dialed::Tcp(s) => std::pin::Pin::new(s).poll_flush(cx),
            Dialed::Unix(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Dialed::Tcp(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            Dialed::Unix(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Why an HTTP upstream can't be connected to.
pub const UPSTREAM_GONE: &str = "the HTTP upstream isn't there";

/// `CONNECT <host>:<port>` on `stream`, an HTTP upstream's: the stream once
/// it answers 200, its head read a byte at a time so nothing of the
/// tunnel's is taken with it; else why not.
pub async fn http_connect(
    mut stream: TcpStream,
    host: &str,
    port: u16,
) -> anyhow::Result<TcpStream> {
    ensure!(
        !host.is_empty()
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._:[]".contains(&b)),
        "{:?} isn't a host to name to the upstream",
        link_format::clean(host)
    );
    let authority = format!("{host}:{port}");
    stream
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await?;
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        ensure!(head.len() < MAX_HEAD, "the upstream's answer is too large");
        let mut byte = [0u8; 1];
        if stream.read(&mut byte).await? == 0 {
            bail!("the upstream closed the connection to {authority}");
        }
        head.push(byte[0]);
    }
    let status = String::from_utf8_lossy(&head);
    let status = status.lines().next().unwrap_or_default();
    ensure!(
        AnswerHead::parse(&head).is_ok_and(|answer| answer.code == 200),
        "the upstream refused {authority}: {}",
        link_format::clean(status)
    );
    Ok(stream)
}

/// A SOCKS5 CONNECT to `target` on `stream`, a SOCKS proxy's, without
/// authentication: the greeting and the request sent at once, one round
/// trip to the exit rather than two.
pub async fn socks5<S>(mut stream: S, target: SocketAddr) -> anyhow::Result<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut request = vec![5, 1, 0, 5, 1, 0];
    match target.ip() {
        IpAddr::V4(ip) => {
            request.push(1);
            request.extend(ip.octets());
        }
        IpAddr::V6(ip) => {
            request.push(4);
            request.extend(ip.octets());
        }
    }
    request.extend(target.port().to_be_bytes());
    stream.write_all(&request).await?;
    let mut choice = [0u8; 2];
    stream.read_exact(&mut choice).await?;
    ensure!(choice == [5, 0], "the SOCKS upstream wants authentication");
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await?;
    ensure!(
        head[1] == 0,
        "the exit couldn't reach {target} (SOCKS reply {})",
        head[1]
    );
    let skip = match head[3] {
        1 => 4 + 2,
        4 => 16 + 2,
        3 => stream.read_u8().await? as usize + 2,
        other => bail!("a SOCKS reply with address type {other}"),
    };
    let mut bound = vec![0u8; skip];
    stream.read_exact(&mut bound).await?;
    Ok(stream)
}

#[cfg(test)]
mod unix_upstream_tests {
    use super::*;

    #[tokio::test]
    async fn local_upstreams_require_a_trusted_socket_and_peer() {
        let dir = link_test_kit::TempDir::new("upstream-owner");
        let path = dir.join("socks.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let stream = trusted_unix(&path).await.unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        drop(stream);
        let link = dir.join("other.sock");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(trusted_unix(&link).await.is_err());
        let regular = dir.join("not-a-socket");
        std::fs::write(&regular, b"upstream").unwrap();
        assert!(trusted_unix(&regular).await.is_err());
    }
}

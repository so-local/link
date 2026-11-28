//! A login's callback port on this machine's loopback, served into the
//! sandbox (Linux) until its request arrives.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;

use link_sandbox_route::Inbound;

/// How long a login's callback port is served, at most.
pub const CALLBACK_FOR: Duration = if cfg!(test) {
    Duration::from_secs(5)
} else {
    Duration::from_secs(300)
};
/// How long a callback's connection may last once its request arrived.
const CALLBACK_REQUEST: Duration = Duration::from_secs(60);
/// The most connections a callback port carries at once.
const CALLBACK_CONNECTIONS: usize = 8;

/// Listeners on this machine's loopback for `port`: IPv4's and IPv6's,
/// each of which must be free, since a browser may take `localhost` as
/// either. Only a machine with no IPv6 loopback, where nothing else can
/// listen on `::1` either, goes without IPv6's.
pub(super) async fn listen(port: u16) -> anyhow::Result<Vec<TcpListener>> {
    let mut listeners = vec![TcpListener::bind(("127.0.0.1", port)).await?];
    match TcpListener::bind(("::1", port)).await {
        Ok(v6) => listeners.push(v6),
        Err(err) if no_ipv6_loopback(&err) => {}
        Err(err) => return Err(err.into()),
    }
    Ok(listeners)
}

/// Whether binding `::1` failed because this machine has no IPv6
/// loopback, rather than because another program holds the port.
fn no_ipv6_loopback(err: &std::io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(libc::EADDRNOTAVAIL | libc::EAFNOSUPPORT)
    )
}

/// A login's callback port, served into the sandbox (through `inbound`,
/// naming the port) until a request arrived on it or [`CALLBACK_FOR`]
/// passes; logged on the wire as `callback`.
pub(super) async fn callback(
    listeners: Vec<TcpListener>,
    inbound: Inbound,
    port: u16,
    wire: Option<&'static link_sandbox_proxy::wire::Wire>,
) {
    let mut call = link_sandbox_proxy::wire::Call::on(wire);
    call.verb = Some("callback");
    call.to = format!("127.0.0.1:{port}");
    call.allowed();
    let serving = serve_callback(listeners, inbound, port, &call);
    let result = link_sandbox_proxy::serve::watched(wire, serving).await;
    call.end(&result);
}

async fn serve_callback(
    listeners: Vec<TcpListener>,
    inbound: Inbound,
    port: u16,
    call: &link_sandbox_proxy::wire::Call,
) -> anyhow::Result<()> {
    let (accepted, mut incoming) = tokio::sync::mpsc::channel(CALLBACK_CONNECTIONS);
    let acceptors: Vec<_> = listeners
        .into_iter()
        .map(|listener| {
            let accepted = accepted.clone();
            tokio::spawn(async move {
                loop {
                    let stream = link_sandbox_proxy::listen::accept(&listener).await;
                    if accepted.send(stream).await.is_err() {
                        break;
                    }
                }
            })
        })
        .collect();
    let (arrived, mut arrival) = tokio::sync::watch::channel(false);
    let deadline = tokio::time::Instant::now() + CALLBACK_FOR;
    let mut relays = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => break,
            _ = arrival.changed() => break,
            Some(stream) = incoming.recv() => {
                if open(&mut relays) >= CALLBACK_CONNECTIONS {
                    continue;
                }
                let browser = link_sandbox_proxy::wire::Counted::new(stream, call);
                relays.spawn(relay(browser, inbound.clone(), port, arrived.clone(), deadline));
            }
        }
    }
    // No new connection once its request came, or its time is up: those
    // open finish, briefly.
    for acceptor in acceptors {
        acceptor.abort();
    }
    let finishing = async { while relays.join_next().await.is_some() {} };
    let _ = tokio::time::timeout(CALLBACK_REQUEST, finishing).await;
    Ok(())
}

/// How many of `relays` are still open: those finished reaped first, so
/// a browser's connections closed long since never count against the
/// port's [`CALLBACK_CONNECTIONS`].
fn open<T: 'static>(relays: &mut tokio::task::JoinSet<T>) -> usize {
    while relays.try_join_next().is_some() {}
    relays.len()
}

/// One connection from the browser, into the sandbox once it sends its
/// request (a browser opens some it never uses), which ends the port's
/// serving.
async fn relay<S>(
    mut browser: S,
    inbound: Inbound,
    port: u16,
    arrived: tokio::sync::watch::Sender<bool>,
    deadline: tokio::time::Instant,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut first = vec![0u8; 16 * 1024];
    let n = tokio::time::timeout_at(deadline, browser.read(&mut first)).await??;
    if n == 0 {
        return Ok(());
    }
    arrived.send_replace(true);
    let mut link = inbound.connect().await?;
    link.write_u16(port).await?;
    link.write_all(&first[..n]).await?;
    tokio::time::timeout(
        CALLBACK_REQUEST,
        link_sandbox_route::io::pipe(&mut browser, &mut link),
    )
    .await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A callback counts only its open connections: those finished are
    /// reaped before the count, so they never fill its eight.
    #[tokio::test]
    async fn a_callback_counts_only_open_connections() {
        let mut relays = tokio::task::JoinSet::new();
        let (hold, held) = tokio::sync::oneshot::channel::<()>();
        relays.spawn(async move {
            let _ = held.await;
        });
        for _ in 0..CALLBACK_CONNECTIONS {
            relays.spawn(async {});
        }
        // Each finished one ends; the held one stays open.
        link_test_kit::wait::eventually_async(
            "the finished relays end",
            std::time::Duration::from_secs(5),
            || open(&mut relays) == 1,
        )
        .await;
        assert_eq!(open(&mut relays), 1);
        drop(hold);
    }

    /// Only a machine without IPv6 goes without IPv6's listener: a port
    /// another program holds there is a refusal.
    #[test]
    fn only_a_missing_ipv6_loopback_is_passed_over() {
        let error = std::io::Error::from_raw_os_error;
        assert!(no_ipv6_loopback(&error(libc::EADDRNOTAVAIL)));
        assert!(no_ipv6_loopback(&error(libc::EAFNOSUPPORT)));
        assert!(!no_ipv6_loopback(&error(libc::EADDRINUSE)));
        assert!(!no_ipv6_loopback(&error(libc::EACCES)));
    }

    /// Another program listening on `::1` alone, which leaves IPv4's port
    /// free, still keeps a login's callback from being served: a browser
    /// taking `localhost` as `::1` would hand it the login's answer.
    #[tokio::test]
    async fn a_port_held_on_ipv6_loopback_alone_is_refused() {
        let held = link_test_kit::net::HeldNumber::port();
        let port = held.port_number();
        let other = match std::net::TcpListener::bind(("::1", port)) {
            Ok(other) => other,
            Err(err) if no_ipv6_loopback(&err) => {
                link_test_kit::gate::lacks("an IPv6 loopback", err, None);
                return;
            }
            Err(err) => panic!("binding [::1]:{port}: {err}"),
        };
        let refused = listen(port).await;
        assert!(refused.is_err(), "{port} on ::1 is another program's");
        // Its IPv4 listener went with the refusal: nothing serves there.
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
        drop(other);
        let served = listen(port).await.unwrap();
        assert_eq!(served.len(), 2, "IPv4's and IPv6's");
    }
}

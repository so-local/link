//! The proxy's routes: each way out of a sandbox but the network, mounted
//! at a prefix of `/_link/` and served by its bridge outside
//! ([`link_sandbox_route`]). The proxy matches a request's target to a
//! route, checks the URL secret if the route needs it, logs the call by
//! the route's name and hands it to the route's bridge ([`crate::bridged`]).
//! It names no route itself.

use anyhow::{bail, ensure};
use link_sandbox_route::Route;
use tokio::io::{AsyncRead, AsyncWrite};

/// A client's connection, which may tell a client that left from one
/// that only shut its writing (a half-close) after its request, and
/// still waits for the answer.
pub trait Peer {
    /// The socket to ask whether its client hung up entirely; `None` when
    /// the two look alike, and its end of what it sends counts as leaving.
    fn hangup_socket(&self) -> Option<std::os::fd::RawFd> {
        None
    }
}

/// Linux sets a Unix socket's hang-up only once its peer closed it, or
/// shut both its sides: shutting its writing alone only ends what it reads.
impl Peer for tokio::net::UnixStream {
    fn hangup_socket(&self) -> Option<std::os::fd::RawFd> {
        use std::os::fd::AsRawFd;
        cfg!(target_os = "linux").then(|| self.as_raw_fd())
    }
}

/// A TCP peer that closed and one that shut its writing both only send
/// their end (FIN): told apart only by writing to it.
impl Peer for tokio::net::TcpStream {}

impl Peer for tokio::io::DuplexStream {}

impl<P: Peer + ?Sized> Peer for Box<P> {
    fn hangup_socket(&self) -> Option<std::os::fd::RawFd> {
        (**self).hangup_socket()
    }
}

impl<P: Peer + ?Sized> Peer for &mut P {
    fn hangup_socket(&self) -> Option<std::os::fd::RawFd> {
        (**self).hangup_socket()
    }
}

/// A connection a route is handed: the client's, as the proxy took it.
pub trait Client: AsyncRead + AsyncWrite + Peer + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Peer + Unpin + Send> Client for T {}

/// The routes a proxy is bound with ([`link_sandbox_route::Route`]):
/// each way out but the network, served by its bridge.
#[derive(Default)]
pub struct Routes {
    pub routes: Vec<Route>,
}

/// No routes: a proxy of the network alone.
pub static NO_ROUTES: Routes = Routes { routes: Vec::new() };

impl Routes {
    /// The route serving `target`, if one does.
    pub fn find(&self, target: &str) -> Option<&Route> {
        self.routes
            .iter()
            .filter(|route| route.serves(target))
            .max_by_key(|route| route.prefix.len())
    }

    /// The route serving this machine's `port`, if one does.
    pub fn port(&self, port: u16) -> Option<&Route> {
        self.routes.iter().find(|route| route.ports.contains(&port))
    }
}

/// `target` past the URL secret: `/_link/url/<secret>/mcp/gh` is
/// `/_link/mcp/gh`; else as it is.
pub fn unlocked(target: &str) -> std::borrow::Cow<'_, str> {
    match target.strip_prefix(link_sandbox_route::SECRET_PREFIX) {
        Some(rest) => format!("/_link/{}", rest.split_once('/').map_or("", |(_, v)| v)).into(),
        None => target.into(),
    }
}

/// The request `head` for `target`, the URL secret taken out of it, and
/// its target so, with the route serving it (if one does). A route that
/// needs the secret is refused without it, any with another one, and the
/// secret before anything but such a route; other targets pass as they
/// are.
pub fn behind_secret<'r>(
    head: &[u8],
    target: &str,
    routes: &'r Routes,
    secret: Option<&str>,
) -> anyhow::Result<(Vec<u8>, String, Option<&'r Route>)> {
    use link_sandbox_route::SECRET_PREFIX;
    let Some(rest) = target.strip_prefix(SECRET_PREFIX) else {
        let mount = routes.find(target);
        ensure!(
            mount.is_none_or(|m| !m.needs_url_secret),
            "refused: {} without the proxy's secret",
            target.chars().take(40).collect::<String>()
        );
        return Ok((head.to_vec(), target.to_string(), mount));
    };
    let (given, _) = rest.split_once('/').unwrap_or((rest, ""));
    let Some(secret) = secret else {
        bail!("refused: this proxy has nothing behind a secret");
    };
    ensure!(
        link_secretive::equal(given.as_bytes(), secret.as_bytes()),
        "refused: the wrong secret"
    );
    let unlocked = unlocked(target).into_owned();
    let Some(mount) = routes.find(&unlocked).filter(|m| m.needs_url_secret) else {
        bail!(
            "refused: nothing behind the secret at {}",
            unlocked.chars().take(40).collect::<String>()
        );
    };
    // The request line, `METHOD <target> VERSION`, with the target so.
    let text = String::from_utf8_lossy(head);
    let line_end = text.find("\r\n").unwrap_or(text.len());
    let line = text[..line_end].replacen(target, &unlocked, 1);
    let head = format!("{line}{}", &text[line_end..]).into_bytes();
    Ok((head, unlocked, Some(mount)))
}

/// `host:port` as a URL writes it: an IPv6 address in brackets, so its
/// port is never read as part of it.
fn to(host: &str, port: u16) -> String {
    match host.contains(':') {
        true => format!("[{host}]:{port}"),
        false => format!("{host}:{port}"),
    }
}

/// A request's name on the wire, and where to, as the log keeps them:
/// never a path (it can carry data), only a host and port; a route's
/// call is named by its route, and where to by its bridge's verdict.
pub fn logged(method: &str, target: &str, routes: &'static Routes) -> (&'static str, String) {
    // Behind the proxy's secret: the route, never the secret.
    let target = unlocked(target);
    if method.eq_ignore_ascii_case("CONNECT") {
        // A raw protocol to an endpoint (`tcp`), or anything else.
        return match crate::http::host_port(&target, None) {
            Some((host, port)) if port != 443 => ("tcp", to(&host, port)),
            Some((host, port)) => ("connect", to(&host, port)),
            None => ("connect", String::new()),
        };
    }
    if let Some(route) = routes.find(&target) {
        return (route.name.as_str(), String::new());
    }
    match target.strip_prefix("http://") {
        Some(after) => {
            let authority = after.split('/').next().unwrap_or_default();
            let to = crate::http::host_port(authority, Some(80))
                .map(|(host, port)| to(&host, port))
                .unwrap_or_default();
            ("http", to)
        }
        None => ("unknown", String::new()),
    }
}

#[cfg(test)]
mod specificity {
    use super::*;
    #[test]
    fn an_action_route_wins_over_its_carrier_prefix() {
        let routes = Routes {
            routes: vec![
                Route {
                    prefix: "/_link/model".into(),
                    ..Default::default()
                },
                Route {
                    prefix: "/_link/model/chat".into(),
                    needs_url_secret: true,
                    ..Default::default()
                },
            ],
        };
        assert!(routes.find("/_link/model/chat").unwrap().needs_url_secret);
        assert!(
            !routes
                .find("/_link/model/v1/messages")
                .unwrap()
                .needs_url_secret
        );
    }
}

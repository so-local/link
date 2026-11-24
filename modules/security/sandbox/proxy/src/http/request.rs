//! What the proxy makes of a sandbox's request: whether a web page sent
//! it, and a plain request as it goes on to its origin.

use anyhow::{Context, bail, ensure};

use super::RequestHead;

/// What makes a request look like a web page's ([`from_a_browser`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Browser {
    /// A `Host` that isn't this machine's loopback: a name a page pointed
    /// here, or a program naming another host to its proxy.
    Host(String),
    /// An `Origin` of another site.
    Origin(String),
    /// An opaque `Origin` (`null`).
    OpaqueOrigin,
    /// `Sec-Fetch-Site` saying another site sent it.
    AnotherSite,
}

impl Browser {
    /// Why it's refused, for the log and the error.
    pub fn why(&self) -> String {
        match self {
            Browser::Host(host) => format!("Host {host:?} isn't this machine"),
            Browser::Origin(origin) => format!("a request from {origin:?}"),
            Browser::OpaqueOrigin => "a request from an opaque origin".into(),
            Browser::AnotherSite => "a request from another site".into(),
        }
    }

    /// What its sender is told, beside the 403, when anything: for another
    /// host's `Host`, which host and why, since a program in the sandbox
    /// names one by mistake. A page is told nothing more than it sent.
    pub fn told(&self) -> Option<String> {
        match self {
            Browser::Host(host) => Some(format!(
                "Link's sandbox: refused Host {}: this proxy answers only for its own \
                 address, and a sealed route serves only its own host\n",
                link_format::clean(&format!("{host:?}"))
            )),
            _ => None,
        }
    }
}

/// What makes the request with `head` look like a web page's, if
/// anything: a `Host` that isn't this machine's loopback (a name a page
/// pointed here), or a browser saying it comes from another site.
pub fn from_a_browser(head: &RequestHead) -> Option<Browser> {
    let loopback = |host: &str| {
        let name = match host.strip_prefix('[') {
            Some(rest) => rest.split(']').next().unwrap_or_default(),
            None => host.rsplit_once(':').map_or(host, |(name, _)| name),
        };
        matches!(
            name.to_ascii_lowercase().as_str(),
            "127.0.0.1" | "localhost" | "::1"
        )
    };
    for header in &head.headers {
        let value = header.value;
        match header.name.as_str() {
            "host" if !loopback(value) => {
                return Some(Browser::Host(value.to_string()));
            }
            "origin" if value != "null" => {
                let host = value
                    .split_once("://")
                    .map_or(value, |(_, rest)| rest)
                    .split('/')
                    .next()
                    .unwrap_or_default();
                if !loopback(host) {
                    return Some(Browser::Origin(value.to_string()));
                }
            }
            "origin" => return Some(Browser::OpaqueOrigin),
            "sec-fetch-site"
                if value.eq_ignore_ascii_case("cross-site")
                    || value.eq_ignore_ascii_case("same-site") =>
            {
                return Some(Browser::AnotherSite);
            }
            _ => {}
        }
    }
    None
}

/// Whether the request with `head` carries the run's `secret` as its
/// `Proxy-Authorization`: `Basic`, any user, the secret its password,
/// as a client sends it from `http://<user>:<secret>@127.0.0.1:<port>`.
pub fn proxy_authorized(head: &RequestHead, secret: &str) -> bool {
    use base64::Engine;
    head.headers
        .iter()
        .filter(|h| h.name == "proxy-authorization")
        .filter_map(|h| {
            let (scheme, token) = h.value.trim().split_once(' ')?;
            scheme.eq_ignore_ascii_case("basic").then_some(token.trim())
        })
        .filter_map(|token| base64::engine::general_purpose::STANDARD.decode(token).ok())
        .any(|pair| {
            pair.splitn(2, |b| *b == b':')
                .nth(1)
                .is_some_and(|given| link_secretive::equal(given, secret.as_bytes()))
        })
}

/// A plain request's head as it goes to `host`:`port` in origin-form
/// (`path`), with `Host` set to that host, `Connection: close`, and no
/// header that would keep the connection for another request, upgrade it
/// or frame its body otherwise; and its body's length. A body sent in
/// chunks, or two lengths that differ, is refused.
pub fn one_request(
    request: &RequestHead,
    path: &str,
    host: &str,
    port: u16,
) -> anyhow::Result<(String, u64)> {
    let mut head = format!("{} {path} {}\r\n", request.method, request.version);
    let mut length: Option<u64> = None;
    for header in &request.headers {
        match header.name.as_str() {
            "host" | "connection" | "keep-alive" | "upgrade" | "te" | "trailer" => continue,
            name if name.starts_with("proxy-") => continue,
            "transfer-encoding" => bail!("refused: a plain request's body in chunks"),
            "content-length" => {
                let value: u64 = header
                    .value
                    .parse()
                    .context("refused: a Content-Length that isn't a number")?;
                ensure!(
                    length.is_none_or(|l| l == value),
                    "refused: two Content-Lengths"
                );
                length = Some(value);
            }
            _ => {}
        }
        head.push_str(header.line);
        head.push_str("\r\n");
    }
    let host = match host.contains(':') {
        true => format!("[{host}]"),
        false => host.to_string(),
    };
    match port {
        80 => head.push_str(&format!("Host: {host}\r\n")),
        port => head.push_str(&format!("Host: {host}:{port}\r\n")),
    }
    head.push_str("Connection: close\r\n\r\n");
    Ok((head, length.unwrap_or(0)))
}

//! Where a call may go: the URLs a setting names ([`HttpsUrl`],
//! [`SendUrl`]), this machine and public addresses, and how a host is found
//! ([`Loopback`]).

use super::*;

/// A cloud's metadata server: its name, found at its address alone
/// ([`Loopback`]), and that address, on the machine's own link.
const METADATA_NAME: &str = "metadata.google.internal";
const METADATA_ADDRESS: Ipv4Addr = Ipv4Addr::new(169, 254, 169, 254);

/// An address Link's calls may go to, typed: `https://` (and in a debug
/// build alone, a test's `http://` to this machine, [`may_send_to`]: the
/// code isn't in a release), with a host, and no login
/// (`user:password@`), which would show in a process list and in logs;
/// nothing in it a shell or a URL reads apart (spaces, controls, `\\`,
/// which parsers read differently), and no query or fragment, so a path
/// can be added to it. Read once where it comes in, carried as it was
/// read (normalized) after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsUrl(String);

impl HttpsUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HttpsUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for HttpsUrl {
    type Err = String;

    fn from_str(text: &str) -> Result<HttpsUrl, String> {
        read_address(text, cfg!(debug_assertions)).map(HttpsUrl)
    }
}

/// An address a call to an endpoint of the user's own goes to, typed:
/// [`HttpsUrl`]'s rules, but `http://` to this machine in every build
/// ([`may_send_to`]), such as an OpenTelemetry collector here
/// (`http://localhost:4318`). Plain `http://` anywhere else is refused:
/// what the call carries would cross the network in the clear.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendUrl(String);

impl SendUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Its host and port, all a log says of it.
    pub fn host(&self) -> &str {
        let rest = self.0.split_once("://").map_or(&*self.0, |(_, rest)| rest);
        rest.split('/').next().unwrap_or(rest)
    }
}

impl fmt::Display for SendUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for SendUrl {
    type Err = String;

    fn from_str(text: &str) -> Result<SendUrl, String> {
        read_address(text, true).map(SendUrl)
    }
}

impl serde::Serialize for SendUrl {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for SendUrl {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// The one reading of an address [`HttpsUrl`] and [`SendUrl`] share:
/// normalized, or why not. `http://` to this machine is taken only when
/// `here`.
fn read_address(text: &str, here: bool) -> Result<String, String> {
    // Never what a login or a query holds, either of which may be a key.
    let shown = || {
        let after = text.split('@').next_back().unwrap_or_default();
        printable(after.split(['?', '#']).next().unwrap_or_default())
    };
    if !text.bytes().all(|b| b.is_ascii_graphic() && b != b'\\') {
        return Err(format!(
            "{} isn't an address: it holds spaces, controls or a \\",
            shown()
        ));
    }
    let parsed = reqwest::Url::parse(text).map_err(|_| format!("{} isn't an address", shown()))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(format!(
            "{} holds a login (user@), which would show in a process list: give an address \
             anyone may read",
            shown()
        ));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(format!(
            "{} isn't an address to add paths to: it has a ? or a #",
            shown()
        ));
    }
    let https = parsed.scheme() == "https" && parsed.host_str().is_some_and(|h| !h.is_empty());
    if !(https || here && may_send_to(text)) {
        return Err(match here {
            true => format!("{} isn't https://, or http:// to this machine", shown()),
            false => format!("{} isn't https://", shown()),
        });
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

/// Whether a call may go to `url`: `https://`, or `http://` to this
/// machine ([`is_on_this_machine`]). Anywhere else, what it carries would
/// cross the network in the clear.
pub fn may_send_to(url: &str) -> bool {
    may_send(url, Plain::Here)
}

/// [`may_send_to`], or `http://` to the metadata server too, or anywhere,
/// as the client says ([`Builder::metadata_server`],
/// [`Builder::plain_http`]).
pub(super) fn may_send(url: &str, plain: Plain) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let has_host = parsed.host_str().is_some_and(|host| !host.is_empty());
    has_host
        && match parsed.scheme() {
            "https" => true,
            "http" => match plain {
                Plain::Said => true,
                Plain::Metadata => is_on_this_machine(url) || is_metadata_server(&parsed),
                Plain::Here => is_on_this_machine(url),
            },
            _ => false,
        }
}

/// Whether `url`'s host is this machine ([`is_this_machine`]), whatever
/// its scheme: not a user before an `@`, or a path that names one.
pub fn is_on_this_machine(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(is_this_machine))
        .unwrap_or(false)
}

/// Whether `host` is this machine: `localhost`, a name under it, or a
/// loopback address (`127.0.0.1`, `[::1]`), which never leave it. A
/// plain `http://` call finds the names at loopback alone ([`Loopback`]).
pub fn is_this_machine(host: &str) -> bool {
    is_localhost(host)
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Whether `url` is `https://` to a name, or to a public address: a
/// name's addresses are checked when it's found ([`PublicOnly`]).
pub(super) fn is_public_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let host = parsed.host_str().unwrap_or_default();
    let address = host.trim_start_matches('[').trim_end_matches(']');
    parsed.scheme() == "https"
        && !host.is_empty()
        && address.parse::<IpAddr>().map_or(true, is_public)
}

/// Finds names as usual, keeping only their public addresses, so a name
/// can't point a call at this machine or its network.
#[derive(Debug)]
pub(super) struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let found: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| is_public(a.ip()))
                .collect();
            if found.is_empty() {
                return Err(format!("{} isn't at a public address", printable(&host)).into());
            }
            Ok(Box::new(found.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Whether `ip` is on the public internet: not this machine, a private
/// or link-local network, or a range kept for other uses, including an
/// IPv4 address carried in IPv6.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || a == 0
                || a >= 240
                || (a == 100 && (64..128).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 88 && c == 99)
                || (a == 198 && (b == 18 || b == 19)))
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            if let Some(v4) = v6.to_ipv4() {
                // Mapped (::ffff:a.b.c.d) and compatible (::a.b.c.d).
                return is_public(IpAddr::V4(v4));
            }
            let embedded = |hi: u16, lo: u16| {
                IpAddr::V4(Ipv4Addr::new(
                    (hi >> 8) as u8,
                    hi as u8,
                    (lo >> 8) as u8,
                    lo as u8,
                ))
            };
            if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
                return is_public(embedded(s[6], s[7]));
            }
            if s[0] == 0x2002 {
                return is_public(embedded(s[1], s[2]));
            }
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] & 0xffc0) == 0xfec0
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                // Teredo, and the local NAT64 prefix (64:ff9b:1::/48),
                // which carry a v4 address elsewhere.
                || (s[0] == 0x2001 && s[1] == 0)
                || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1))
        }
    }
}

/// How a plain `http://` call finds its host: `localhost`, and every name
/// under it, is this machine's loopback, `127.0.0.1` and `::1`, and DNS
/// is never asked for one, so a network's can't answer `dev.localhost`
/// with an address elsewhere and have a credential sent there in the
/// clear. No other name is found at all, but on a client for a cloud's
/// metadata server ([`Builder::metadata_server`]), whose name is its
/// address alone, and on one its user said sends plain `http://` anywhere
/// ([`Builder::plain_http`]), where DNS finds it. An address
/// (`127.0.0.1`) isn't looked up.
#[derive(Debug)]
pub struct Loopback {
    pub(super) beyond: Plain,
}

impl reqwest::dns::Resolve for Loopback {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let (here, beyond) = (is_localhost(name.as_str()), self.beyond);
        let name = name.as_str().to_string();
        Box::pin(async move {
            if here {
                let addrs = [
                    SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                    SocketAddr::from((Ipv6Addr::LOCALHOST, 0)),
                ];
                return Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs);
            }
            match beyond {
                Plain::Said => {
                    let found = tokio::net::lookup_host((name, 0)).await?;
                    Ok(Box::new(found.collect::<Vec<_>>().into_iter()) as reqwest::dns::Addrs)
                }
                Plain::Metadata if is_metadata_name(&name) => {
                    let addrs = [SocketAddr::from((METADATA_ADDRESS, 0))];
                    Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
                }
                Plain::Metadata => {
                    Err("plain http:// goes to this machine or its metadata server alone".into())
                }
                Plain::Here => Err("plain http:// goes to this machine alone".into()),
            }
        })
    }
}

/// The metadata server's name, with or without its final dot.
fn is_metadata_name(name: &str) -> bool {
    name.trim_end_matches('.')
        .eq_ignore_ascii_case(METADATA_NAME)
}

/// Whether `url`'s host is the metadata server, by its name or its
/// address.
fn is_metadata_server(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| {
        is_metadata_name(host) || host.parse::<Ipv4Addr>() == Ok(METADATA_ADDRESS)
    })
}

/// `localhost`, or a name under it, with or without its final dot.
fn is_localhost(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    name == "localhost" || name.ends_with(".localhost")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_https_url_is_https_or_here_with_no_login() {
        use super::HttpsUrl;
        for good in [
            "https://github.com/so-local/link-releases/releases",
            "https://builds.example/staging/0123/",
            "https://builds.example/a@b",
            #[cfg(debug_assertions)]
            "http://127.0.0.1:8080/releases",
            #[cfg(debug_assertions)]
            "http://localhost:9/x",
        ] {
            let url: HttpsUrl = good.parse().unwrap_or_else(|e| panic!("{good}: {e}"));
            assert_eq!(url.as_str(), good.trim_end_matches('/'));
        }
        for (bad, says) in [
            ("http://builds.example", "isn't https://"),
            #[cfg(not(debug_assertions))]
            ("http://127.0.0.1:8080/releases", "isn't https://"),
            #[cfg(not(debug_assertions))]
            ("http://localhost:9/x", "isn't https://"),
            ("ftp://builds.example", "isn't https://"),
            ("https://me:secret@builds.example/staging", "holds a login"),
            ("https://me@builds.example", "holds a login"),
            ("https://builds.example/a b", "spaces, controls"),
            ("https://builds.example/\nx", "spaces, controls"),
            ("http://127.0.0.1\\@evil.example/r", "spaces, controls"),
            ("https://builds.example/r?x=1", "a ? or a #"),
            ("https://builds.example/r?key=secret", "a ? or a #"),
            ("https://builds.example/r#x", "a ? or a #"),
            ("not a url", "spaces, controls"),
            ("builds.example", "isn't an address"),
        ] {
            let err = bad.parse::<HttpsUrl>().unwrap_err();
            assert!(err.contains(says), "{bad}: {err}");
            assert!(!err.contains("secret"), "{err}");
        }
    }

    /// An endpoint of the user's own: [`HttpsUrl`]'s rules, and plain
    /// `http://` to this machine in every build, never beyond it.
    #[test]
    fn a_send_url_is_https_or_http_here() {
        use super::SendUrl;
        for good in [
            "https://api.honeycomb.io",
            "https://otlp.example/v1/metrics/",
            "http://localhost:4318",
            "http://127.0.0.1:4318/v1/metrics",
            "http://[::1]:4318",
        ] {
            let url: SendUrl = good.parse().unwrap_or_else(|e| panic!("{good}: {e}"));
            assert_eq!(url.as_str(), good.trim_end_matches('/'));
        }
        let url: SendUrl = "http://localhost:4318/v1/metrics".parse().unwrap();
        assert_eq!(url.host(), "localhost:4318");
        for (bad, says) in [
            ("http://otel-collector:4318", "or http:// to this machine"),
            ("http://localhost@example.com/", "holds a login"),
            ("https://me:secret@otlp.example", "holds a login"),
            ("https://otlp.example/?api_key=secret", "a ? or a #"),
            ("ftp://localhost", "or http:// to this machine"),
        ] {
            let err = bad.parse::<SendUrl>().unwrap_err();
            assert!(err.contains(says), "{bad}: {err}");
            assert!(!err.contains("secret"), "{err}");
        }
        // Kept as text, read back as it was, and refused when it isn't one.
        let quoted = |url: &str| format!("\"{url}\"");
        let read: SendUrl = serde_json::from_str(&quoted("http://localhost:4318/")).unwrap();
        assert_eq!(
            serde_json::to_string(&read).unwrap(),
            quoted("http://localhost:4318")
        );
        assert!(serde_json::from_str::<SendUrl>(&quoted("http://example.com")).is_err());
    }

    #[test]
    fn only_https_or_this_machine() {
        assert!(may_send_to("https://accounts.example.com/api/v1/device"));
        assert!(may_send_to("http://localhost:3000/api/v1/device"));
        assert!(may_send_to("http://dev.localhost:7080/_link/shares"));
        assert!(may_send_to("http://127.0.0.1:7080"));
        assert!(may_send_to("http://[::1]:7080"));
        assert!(!may_send_to("http://accounts.example.com/api/v1/device"));
        assert!(!may_send_to("http://localhost@example.com/"));
        assert!(!may_send_to("http://example.com/localhost"));
        assert!(!may_send_to("ftp://localhost/"));
        assert!(!may_send_to("accounts.example.com"));
        assert!(!may_send_to("https://"));
    }

    /// `localhost` and the names under it are loopback, without DNS; no
    /// other name is found, whatever DNS would answer.
    #[tokio::test]
    async fn plain_http_finds_loopback_alone() {
        use reqwest::dns::Resolve;
        for name in [
            "localhost",
            "localhost.",
            "dev.localhost",
            "A.Dev.LOCALHOST.",
        ] {
            let addrs: Vec<_> = Loopback {
                beyond: Plain::Here,
            }
            .resolve(name.parse().unwrap())
            .await
            .unwrap()
            .collect();
            assert!(!addrs.is_empty(), "{name}");
            assert!(
                addrs.iter().all(|a| a.ip().is_loopback()),
                "{name}: {addrs:?}"
            );
        }
        for name in [
            "example.com",
            "localhost.example.com",
            "notlocalhost",
            "one.one.one.one",
        ] {
            assert!(
                Loopback {
                    beyond: Plain::Here
                }
                .resolve(name.parse().unwrap())
                .await
                .is_err(),
                "{name}"
            );
        }
    }

    /// A metadata server's client finds its name at its address alone,
    /// and no other but this machine's.
    #[tokio::test]
    async fn the_metadata_server_is_found_at_its_address() {
        use reqwest::dns::Resolve;
        let metadata = Loopback {
            beyond: Plain::Metadata,
        };
        for name in ["metadata.google.internal", "Metadata.Google.Internal."] {
            let addrs: Vec<_> = metadata
                .resolve(name.parse().unwrap())
                .await
                .unwrap()
                .collect();
            assert_eq!(addrs, [SocketAddr::from((METADATA_ADDRESS, 0))], "{name}");
        }
        for name in ["example.com", "metadata.google.internal.example"] {
            assert!(
                metadata.resolve(name.parse().unwrap()).await.is_err(),
                "{name}"
            );
        }
        assert!(may_send("http://169.254.169.254/x", Plain::Metadata));
        assert!(!may_send("http://169.254.169.254/x", Plain::Here));
    }
}

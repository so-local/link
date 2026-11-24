//! Addresses and names on the network, as every way out of a machine's
//! programs checks them: what no proxy, exit or fetch ever reaches
//! ([`forbidden`], [`this_machine`]), the shapes a host, an endpoint and a
//! machine of a network are written in, and where a proxy's connections
//! leave from ([`Upstream`]).

use link_format::WithinExt;
use std::{
    net::{IpAddr, SocketAddr},
    path::{Component, Path, PathBuf},
};

use anyhow::Context;

/// Addresses a sandbox never reaches through its proxy, by their range:
/// loopback, its networks, and a cloud's metadata service. (This machine's
/// other addresses are [`this_machine`].) Link's web tool, outside every
/// sandbox, never fetches them either.
pub fn forbidden(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || a == 0
                // Shared address space (carrier-grade NAT, Tailscale).
                || (a == 100 && (64..128).contains(&b))
                // IETF protocol assignments, benchmarking, and reserved.
                || (a == 192 && b == 0 && ip.octets()[2] == 0)
                || (a == 198 && (18..20).contains(&b))
                // Documentation (TEST-NET-1, -2 and -3), never a real host.
                || (a == 192 && b == 0 && ip.octets()[2] == 2)
                || (a == 198 && b == 51 && ip.octets()[2] == 100)
                || (a == 203 && b == 0 && ip.octets()[2] == 113)
                || a >= 240
                // Azure's host for its machines (their metadata and
                // agent), at a public address.
                || ip.octets() == [168, 63, 129, 16]
        }
        IpAddr::V6(ip) => {
            // IPv4 inside IPv6 reaches that IPv4 address: mapped
            // (`::ffff:a.b.c.d`), compatible (`::a.b.c.d`), NAT64
            // (`64:ff9b::a.b.c.d`, and the local `64:ff9b:1::/48`) and 6to4
            // (`2002:<v4>::`).
            if let Some(v4) = inner_v4(ip) {
                return forbidden(IpAddr::V4(v4));
            }
            let s = ip.segments();
            if s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1 {
                return true;
            }
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                // Unique local (fc00::/7), link-local (fe80::/10) and the
                // retired site-local (fec0::/10), which some networks route.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || (first & 0xffc0) == 0xfec0
                // Teredo (2001::/32), IPv6 tunnelled over IPv4 to an IPv4
                // address written inside it, inverted.
                || (first == 0x2001 && s[1] == 0)
                // Documentation (2001:db8::/32 and 3fff::/20).
                || (first == 0x2001 && s[1] == 0x0db8)
                || (first & 0xfff0) == 0x3ff0
        }
    }
}

/// The IPv4 address an IPv6 one reaches, written inside it: mapped
/// (`::ffff:a.b.c.d`), compatible (`::a.b.c.d`), NAT64
/// (`64:ff9b::a.b.c.d`) or 6to4 (`2002:<v4>::`).
fn inner_v4(ip: std::net::Ipv6Addr) -> Option<std::net::Ipv4Addr> {
    let s = ip.segments();
    let v4 = |hi: u16, lo: u16| {
        std::net::Ipv4Addr::from([(hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8])
    };
    if let Some(v4) = ip.to_ipv4_mapped() {
        return Some(v4);
    }
    if s[..6] == [0; 6] && !ip.is_loopback() && !ip.is_unspecified() {
        return Some(v4(s[6], s[7]));
    }
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return Some(v4(s[6], s[7]));
    }
    (s[0] == 0x2002).then(|| v4(s[1], s[2]))
}

/// Whether `ip` is one of this machine's own addresses, whatever its
/// range: a server's public address on its network interface reaches its
/// own servers, written plainly or inside IPv6. Binding to an address
/// works only where it's this machine's; nothing is sent.
pub fn this_machine(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(v6) => inner_v4(v6).map(IpAddr::V4).unwrap_or(ip),
        v4 => v4,
    };
    std::net::UdpSocket::bind(SocketAddr::new(ip, 0)).is_ok()
}

/// An error unless `pattern` names hosts: a name (`api.anthropic.com`),
/// `*.` and a name for every name under it, or an IP address.
pub fn check_host(pattern: &str) -> anyhow::Result<()> {
    if pattern.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    let name = pattern.strip_prefix("*.").unwrap_or(pattern);
    let plain = name.contains('.')
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    anyhow::ensure!(
        plain,
        "{pattern:?} isn't a host: a name (example.com), *.example.com, or an IP address"
    );
    Ok(())
}

/// An error unless `entry` names machines of a network: `*`, an address,
/// a range (`192.168.1.0/24`, `fd00::/8`) or a name (`nas.local`).
pub fn check_lan(entry: &str) -> anyhow::Result<()> {
    if entry == "*" || entry.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    if let Some((ip, bits)) = entry.split_once('/') {
        let max = match ip.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(_)) => 32,
            Ok(std::net::IpAddr::V6(_)) => 128,
            Err(_) => anyhow::bail!("{entry:?} isn't a range: an address, /, and its bits"),
        };
        let bits: u8 = bits
            .parse()
            .map_err(|_| anyhow::anyhow!("{entry:?} isn't a range: an address, /, and its bits"))?;
        anyhow::ensure!(bits <= max, "{entry:?} has more bits than its address");
        return Ok(());
    }
    let plain = entry.len() <= 253
        && entry.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    anyhow::ensure!(
        plain,
        "{entry:?} isn't a machine of a network: a name (nas.local), an address, a range \
         (192.168.1.0/24), or *"
    );
    Ok(())
}

/// An endpoint a proxy reaches raw: `host:port`, the host a name, the
/// names under one (`*.abc.mongodb.net`) or an IPv4 address, the port not
/// 0. Its host (lowercase, without a final dot) and port.
pub fn check_endpoint(text: &str) -> anyhow::Result<(String, u16)> {
    let parsed = text.rsplit_once(':').and_then(|(host, port)| {
        let port: u16 = port.parse().ok().filter(|p| *p != 0)?;
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        let ok = !host.contains(':') && check_host(&host).is_ok();
        ok.then_some((host, port))
    });
    parsed.ok_or_else(|| {
        anyhow::anyhow!(
            "{text:?} isn't an endpoint: host:port, such as github.com:22, db.example.com:5432 \
             or *.abc.mongodb.net:27017"
        )
    })
}

/// Whether `host:port` is one of `endpoints`, a `*.` one holding the
/// names under it (names compared without case or a final dot).
pub fn endpoint_named(host: &str, port: u16, endpoints: &[String]) -> bool {
    endpoints.iter().any(|e| {
        e.rsplit_once(':').is_some_and(|(h, p)| {
            p.parse() == Ok(port) && host_allowed(host, std::slice::from_ref(&h.to_string()))
        })
    })
}

/// Whether `host` is one `patterns` allow (names compared without case or
/// a final dot).
pub fn host_allowed(host: &str, patterns: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
        match pattern.strip_prefix("*.") {
            Some(parent) => host
                .strip_suffix(parent)
                .is_some_and(|sub| sub.len() > 1 && sub.ends_with('.')),
            None => host == pattern,
        }
    })
}

/// Where a proxy's connections leave from.
#[derive(Clone, Debug, PartialEq)]
pub enum Upstream {
    /// This machine's own network.
    Direct,
    /// A SOCKS5 proxy on this machine's loopback.
    Socks5(SocketAddr),
    /// A SOCKS5 proxy on a Unix socket of this machine, one only its user
    /// reaches.
    Socks5Unix(PathBuf),
    /// An HTTP proxy on this machine's loopback, asked `CONNECT
    /// <name>:<port>`: it resolves each name itself, and refuses what this
    /// proxy would (private, loopback and metadata addresses), so no name
    /// a sandbox asks for is looked up on this machine. A machine whose
    /// traffic leaves only through a filtering proxy of its network.
    Http(SocketAddr),
}

impl Upstream {
    /// It as [`Upstream::parse`] reads it.
    pub fn text(&self) -> String {
        match self {
            Self::Direct => "direct".into(),
            Self::Socks5(addr) => format!("socks5://{addr}"),
            Self::Socks5Unix(path) => format!("socks5://{}", path.display()),
            Self::Http(addr) => format!("http://{addr}"),
        }
    }

    /// `direct`, `socks5://127.0.0.1:<port>`, `socks5://<socket>` with
    /// the socket's absolute path (`socks5:///home/me/...`), or
    /// `http://127.0.0.1:<port>`.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        if text == "direct" {
            return Ok(Self::Direct);
        }
        if let Some(rest) = text.strip_prefix("http://") {
            let addr: SocketAddr = rest
                .trim_end_matches('/')
                .parse()
                .within(format!("{text:?} isn't an address and port"))?;
            anyhow::ensure!(
                addr.ip().is_loopback(),
                "the HTTP upstream must be on this machine's loopback, not {addr}"
            );
            return Ok(Self::Http(addr));
        }
        let rest = text.strip_prefix("socks5://").with_context(|| {
            format!(
                "{text:?}: an upstream is direct, socks5://127.0.0.1:<port>, \
                 socks5://<a socket's absolute path> or http://127.0.0.1:<port>"
            )
        })?;
        if rest.starts_with('/') {
            let path = Path::new(rest);
            anyhow::ensure!(
                !rest.contains('\0')
                    && path
                        .components()
                        .all(|c| matches!(c, Component::RootDir | Component::Normal(_))),
                "{text:?} isn't a socket's absolute path"
            );
            return Ok(Self::Socks5Unix(path.to_path_buf()));
        }
        let addr: SocketAddr = rest
            .parse()
            .within(format!("{text:?} isn't an address and port"))?;
        anyhow::ensure!(
            addr.ip().is_loopback(),
            "the SOCKS upstream must be on this machine's loopback, not {addr}"
        );
        Ok(Self::Socks5(addr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This machine's address written inside IPv6 (mapped, compatible,
    /// NAT64, 6to4) is this machine's as written plainly.
    /// Retired, tunnelled and documentation ranges are never reached;
    /// public addresses beside them are.
    #[test]
    fn retired_tunnelled_and_documentation_ranges_are_forbidden() {
        for refused in [
            "fec0::1",
            "feff::1",
            "2001::1",
            "2001:0:4136:e378:8000:63bf:3fff:fdd2",
            "2001:db8::1",
            "3fff::1",
            "3fff:fff::1",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.1",
        ] {
            assert!(forbidden(refused.parse().unwrap()), "{refused}");
        }
        for allowed in [
            "2001:4860::8888",
            "2001:1::1",
            "4000::1",
            "203.0.114.1",
            "198.51.101.1",
        ] {
            assert!(!forbidden(allowed.parse().unwrap()), "{allowed}");
        }
    }

    #[test]
    fn this_machine_is_judged_through_ipv6s_forms() {
        for v6 in ["::ffff:127.0.0.1", "64:ff9b::7f00:1", "2002:7f00:1::"] {
            let ip: IpAddr = v6.parse().unwrap();
            assert!(this_machine(ip), "{v6}");
        }
    }

    #[test]
    fn hosts_match_exactly_or_below_a_wildcard() {
        let list = ["api.anthropic.com".to_string(), "*.example.org".into()];
        assert!(host_allowed("api.anthropic.com", &list));
        assert!(host_allowed("API.Anthropic.com.", &list));
        assert!(host_allowed("api.example.org", &list));
        assert!(host_allowed("a.b.example.org", &list));
        assert!(!host_allowed("example.org", &list));
        assert!(!host_allowed("evilexample.org", &list));
        assert!(!host_allowed("anthropic.com", &list));
        assert!(!host_allowed("api.anthropic.com.evil.net", &list));
        for good in ["example.com", "*.example.com", "1.1.1.1", "2606:4700::1111"] {
            check_host(good).unwrap();
        }
        for bad in [
            "",
            "*",
            "*.com.",
            "localhost",
            "a..b",
            "-a.com",
            "a b.com",
            "*x.com",
            "a/b.com",
        ] {
            assert!(check_host(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn endpoints_are_a_host_and_a_port() {
        assert_eq!(
            check_endpoint("GitHub.com.:22").unwrap(),
            ("github.com".into(), 22)
        );
        assert_eq!(
            check_endpoint("10.0.0.5:5432").unwrap(),
            ("10.0.0.5".into(), 5432)
        );
        for bad in [
            "github.com",
            "github.com:0",
            "github.com:99999",
            "*.com:22",
            "*:22",
            ":22",
            "a b.com:22",
            "localhost:22",
            "2606:4700::1111:22",
        ] {
            assert!(check_endpoint(bad).is_err(), "{bad:?}");
        }
        let named = ["github.com:22".to_string(), "db.example.com:5432".into()];
        assert!(endpoint_named("GITHUB.com.", 22, &named));
        assert!(!endpoint_named("github.com", 443, &named));
        assert!(!endpoint_named("gitlab.com", 22, &named));
        // The names under one, never the one itself or another port.
        assert_eq!(
            check_endpoint("*.ABC.mongodb.net:27017").unwrap(),
            ("*.abc.mongodb.net".into(), 27017)
        );
        let wild = ["*.abc.mongodb.net:27017".to_string()];
        assert!(endpoint_named("shard-00.abc.mongodb.net", 27017, &wild));
        assert!(!endpoint_named("abc.mongodb.net", 27017, &wild));
        assert!(!endpoint_named("x.other.mongodb.net", 27017, &wild));
        assert!(!endpoint_named("shard-00.abc.mongodb.net", 27018, &wild));
    }

    #[test]
    fn upstreams_are_direct_or_on_loopback() {
        assert_eq!(Upstream::parse("direct").unwrap(), Upstream::Direct);
        assert_eq!(
            Upstream::parse("socks5://127.0.0.1:1080").unwrap(),
            Upstream::Socks5("127.0.0.1:1080".parse().unwrap())
        );
        assert_eq!(
            Upstream::parse("socks5:///run/user/1000/lnk/upstream.sock").unwrap(),
            Upstream::Socks5Unix("/run/user/1000/lnk/upstream.sock".into())
        );
        assert!(Upstream::parse("socks5://10.0.0.1:1080").is_err());
        assert!(Upstream::parse("socks5:///home/me/../other/upstream.sock").is_err());
        assert!(Upstream::parse("socks5://upstream.sock").is_err());
        assert!(Upstream::parse("http://x").is_err());
        assert_eq!(
            Upstream::parse("http://127.0.0.1:3128").unwrap(),
            Upstream::Http("127.0.0.1:3128".parse().unwrap())
        );
        assert!(Upstream::parse("http://10.0.0.1:3128").is_err());
        assert!(Upstream::parse("https://127.0.0.1:3128").is_err());
    }
}

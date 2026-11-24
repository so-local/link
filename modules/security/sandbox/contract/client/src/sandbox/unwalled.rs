//! A program with no sandbox, its proxy beside it: the wall gone, the
//! way out kept.
//!
//! An agent with no sandbox (`sandbox = false`) keeps its proxy, so it
//! keeps what doesn't depend on the wall: its tools, its memory, its
//! scheduler, its model's key and its sealed secrets added outside, the
//! log, `lnk sandbox stop` and its counted usage. The proxy listens on
//! this machine's loopback, its routes behind its URL secret, as a Mac's
//! does; but nothing holds the program to it: it's advice the program may
//! ignore, not a wall.

use serde::{Deserialize, Serialize};

use super::confinement::{ProxyAt, opened, proxy};
use super::spec::{NetworkMode, Os, Spec};
use super::{Proxy, check_name};
use link_secretive::Duplicate;

/// A program run as it is, with no wall, beside its proxy.
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unwalled {
    /// Its name ([`super::Confinement::name`]), naming its wire and the
    /// files its proxy reads its secrets from.
    pub name: String,
    /// Its proxy, on this machine's loopback at its `listen` port (none,
    /// a free one); its `dir`, `serve` and `hold` aren't read: there's no
    /// sandbox's network to cross.
    #[duplicate]
    pub proxy: Proxy,
    /// The program is pointed at its proxy (`HTTPS_PROXY`), which filters
    /// its network as its spec says. No: its spec's network is the
    /// machine's as it is (`[network] mode = "host"`), and its proxy
    /// serves only Link's ways out (its tools, its model, its secrets).
    pub pointed: bool,
}

impl Unwalled {
    /// What `spec` makes with no wall, named `name`, its proxy run as `at`
    /// says: its network sections as its proxy's rules, but for
    /// `[network] mode = "host"`, which points it at no proxy; its files
    /// and programs aren't read, as nothing confines them. An upstream
    /// exit remains advice too: only a sandbox can keep a program on the
    /// route it was given.
    pub fn of(spec: &Spec, name: &str, at: ProxyAt) -> anyhow::Result<Unwalled> {
        check_name(name)?;
        let spec = spec.on(Os::here())?;
        let host = spec.network.as_ref().and_then(|n| n.mode) == Some(NetworkMode::Host);
        let mut opened = opened(&spec, host)?;
        if host {
            opened.internet = None;
            opened.tcp.clear();
            opened.lan.clear();
        }
        Ok(Unwalled {
            name: name.to_string(),
            pointed: !host,
            proxy: proxy(opened, at)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(text: &str, upstream: &str) -> anyhow::Result<Unwalled> {
        let spec: Spec = link_spec::parse_spec(&format!("schema = 1\n{text}"))?;
        let at = ProxyAt {
            upstream: upstream.into(),
            listen: Some(40999),
            ..Default::default()
        };
        Unwalled::of(&spec, "main-link", at)
    }

    #[test]
    fn its_proxy_filters_as_its_spec_says_and_it_is_pointed_there() {
        let unwalled = of(
            "[internet]\nhosts = [\"api.example.com\"]\n[tools]\nnames = [\"gh\"]\n",
            "direct",
        )
        .unwrap();
        assert!(unwalled.pointed && unwalled.proxy.pages().is_none());
        assert_eq!(unwalled.proxy.hosts, ["api.example.com"]);
        assert_eq!(tools(&unwalled.proxy), ["gh"]);
        assert_eq!(unwalled.proxy.listen, Some(40999));
        assert!(!unwalled.proxy.no_internet);
        // No [internet]: the advice is none, its routes still served.
        let closed = of("[tools]\nnames = [\"gh\"]\n", "direct").unwrap();
        assert!(closed.pointed && closed.proxy.no_internet);
        assert_eq!(tools(&closed.proxy), ["gh"]);
    }

    /// The tools `proxy`'s tools route may call.
    fn tools(proxy: &crate::sandbox::Proxy) -> Vec<String> {
        let route = proxy.route("mcp").unwrap();
        crate::route::given::<crate::sandbox::routes::Tools>(&route.given)
            .unwrap()
            .tools
    }

    #[test]
    fn the_network_as_it_is_points_it_at_no_proxy() {
        let host = of(
            "[network]\nmode = \"host\"\n[internet]\nhosts = [\"*\"]\n[browser]\nhosts = [\"*\"]\n",
            "direct",
        )
        .unwrap();
        assert!(!host.pointed && host.proxy.pages().is_some());
        assert!(host.proxy.no_internet && host.proxy.lan.is_empty());
        let open: crate::sandbox::routes::Open =
            crate::route::given(&host.proxy.route("open").unwrap().given).unwrap();
        assert_eq!(open.hosts, ["*"]);
    }

    #[test]
    fn an_exit_is_advisory_with_no_wall() {
        let unwalled = of("[internet]\nhosts = [\"*\"]\n", "socks5://127.0.0.1:1080").unwrap();
        assert!(unwalled.pointed);
        assert_eq!(unwalled.proxy.upstream, "socks5://127.0.0.1:1080");
    }
}

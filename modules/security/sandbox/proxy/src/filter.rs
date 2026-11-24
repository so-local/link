//! The proxy's rules: where it dials from, which hosts and addresses it
//! reaches, and its routes.

use std::net::IpAddr;

use anyhow::ensure;
use link_sandbox_contract::net::{Upstream, forbidden, this_machine};

use crate::routes::{NO_ROUTES, Routes};

/// The proxy's rules: where to dial, and which addresses are allowed.
#[derive(Clone, Copy)]
pub struct Filter {
    /// Where its internet connections leave from; it lives as long as the
    /// proxy, which is the process.
    pub upstream: &'static Upstream,
    pub allowed: fn(IpAddr) -> bool,
    /// The only hosts it may reach, by name, before any is resolved;
    /// None: any.
    pub hosts: Option<&'static [String]>,
    /// Endpoints reached beyond HTTPS's and HTTP's ports (`host:port`);
    /// `hosts`, if any, still applies.
    pub tcp: &'static [String],
    /// Its wire: where each call is logged, and whether every wire is
    /// stopped ([`crate::wire`]). None: neither (tests).
    pub wire: Option<&'static crate::wire::Wire>,
    /// Its routes ([`crate::routes`]): each way out but the network.
    pub routes: &'static Routes,
    /// Who its route calls are from ([`crate::bridged`]); None: it calls
    /// none.
    pub calling: Option<&'static crate::bridged::Calling>,
    /// The secret the URLs of routes that need it carry
    /// (`/_link/url/<secret>/...`); None: those are off.
    pub url_secret: Option<&'static link_secretive::Secret>,
    /// The run's secret a `CONNECT` or a plain `http://` request carries
    /// ([`crate::http::proxy_authorized`]); None: none needed.
    pub proxy_secret: Option<&'static link_secretive::Secret>,
    /// Machines of your network it may reach ([`crate::lan`]), on any
    /// port, dialed from here.
    pub lan: &'static [String],
    /// Whether it reaches the internet's hosts; false: only `lan`.
    pub internet: bool,
    /// This call reaches a port only `lan` allows: not the internet.
    pub lan_only: bool,
}

impl Filter {
    /// Rules leaving from `upstream`: a SOCKS proxy's lives as long as
    /// the proxy, which is the process; the direct one is a constant, so
    /// rules made again and again (tests, the fuzzer) keep nothing.
    pub fn new(upstream: Upstream) -> Self {
        Self {
            upstream: match upstream {
                Upstream::Direct => &Upstream::Direct,
                upstream => Box::leak(Box::new(upstream)),
            },
            allowed: |ip| !forbidden(ip) && !this_machine(ip),
            hosts: None,
            tcp: &[],
            wire: None,
            routes: &NO_ROUTES,
            calling: None,
            url_secret: None,
            proxy_secret: None,
            lan: &[],
            internet: true,
            lan_only: false,
        }
    }

    /// Whether it reaches every public host: the internet, with no list
    /// of hosts. A sandbox with no internet (only `[lan]`, or nothing but
    /// its routes) reaches none, though it has no list either.
    pub fn every_host(&self) -> bool {
        self.internet && self.hosts.is_none()
    }

    /// The same, reaching `lan`'s machines of your network, and the
    /// internet only when `internet`. The list lives as long as the proxy,
    /// which is the process.
    pub fn with_lan(self, lan: Vec<String>, internet: bool) -> Self {
        Self {
            lan: Vec::leak(lan),
            internet,
            ..self
        }
    }

    /// The same, serving `routes`, those that need it behind `secret`
    /// (None: those are off). They live as long as the proxy, which is
    /// the process.
    pub fn with_routes(self, routes: Routes, secret: Option<link_secretive::Secret>) -> Self {
        Self {
            routes: Box::leak(Box::new(routes)),
            url_secret: secret.map(|s| &*Box::leak(Box::new(s))),
            ..self
        }
    }

    /// The same, taking a `CONNECT` or a plain `http://` request only
    /// with `secret` (None: any). It lives as long as the proxy, which is
    /// the process.
    pub fn with_proxy_secret(self, secret: Option<link_secretive::Secret>) -> Self {
        Self {
            proxy_secret: secret.map(|s| &*Box::leak(Box::new(s))),
            ..self
        }
    }

    /// The same, its route calls from `calling`, which lives as long as
    /// the proxy, which is the process.
    pub fn calling(self, calling: crate::bridged::Calling) -> Self {
        Self {
            calling: Some(&*Box::leak(Box::new(calling))),
            ..self
        }
    }

    /// The same, reaching `endpoints` (`host:port`) too. The list lives as
    /// long as the proxy, which is the process.
    pub fn with_tcp(self, endpoints: Vec<String>) -> Self {
        Self {
            tcp: Vec::leak(endpoints),
            ..self
        }
    }

    /// Refused unless `port` is `standard` (HTTPS's for `CONNECT`, HTTP's
    /// for a plain request) or `host:port` is an endpoint named.
    pub fn at_port(self, host: &str, port: u16, standard: u16) -> anyhow::Result<Self> {
        if port == standard || link_sandbox_contract::net::endpoint_named(host, port, self.tcp) {
            return Ok(self);
        }
        // A machine of your network, on any port: but only as that.
        ensure!(
            crate::lan::may(host, self.lan),
            "refused: {host}:{port} isn't port {standard}, nor an endpoint this sandbox names"
        );
        Ok(Self {
            lan_only: true,
            ..self
        })
    }

    /// The same, logging each call on `wire` and refusing all once every
    /// wire is stopped. The wire lives as long as the proxy, which is the
    /// process.
    pub fn on(self, wire: crate::wire::Wire) -> Self {
        Self {
            wire: Some(&*Box::leak(Box::new(wire))),
            ..self
        }
    }

    /// The same, reaching only `hosts` (none: any). The list lives as long
    /// as the proxy, which is the process.
    pub fn only(self, hosts: Vec<String>) -> Self {
        Self {
            hosts: (!hosts.is_empty()).then(|| &*Vec::leak(hosts)),
            ..self
        }
    }
}

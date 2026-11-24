//! The HTTP clients it keeps across calls, each fetch's pinned to the
//! addresses it checked.

use super::*;

/// The clients a session's fetches and searches share, each made at its
/// first use: one for pages, which follows no redirect itself and
/// connects to the addresses checked only ([`Pins`]), one for the search
/// API. One client keeps its connections, its TLS roots and its thread
/// across calls, where a client a fetch would parse the roots again.
#[derive(Clone, Debug, Default)]
pub struct Clients(std::sync::Arc<std::sync::Mutex<ClientsInner>>);

#[derive(Debug, Default)]
struct ClientsInner {
    pages: Option<(reqwest::blocking::Client, Pins)>,
    search: Option<reqwest::blocking::Client>,
}

/// The addresses each host a pages client fetches was checked at, at
/// most `limits.toml`'s `pinned-max` hosts, past which the next fetch
/// makes a new client: the
/// client's resolver answers a host with them, never resolving it again,
/// so a name can't point elsewhere between its check and the fetch. A
/// host is pinned before each request to it, and stays while its client
/// lives, so a fetch in flight never finds its host gone. A name never
/// pinned, as a proxy's from the tool's environment, resolves as the
/// system does.
#[derive(Clone, Debug, Default)]
struct Pins(std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<SocketAddr>>>>);

impl reqwest::dns::Resolve for Pins {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_ascii_lowercase();
        let pinned = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&host)
            .cloned();
        Box::pin(async move {
            let addresses: Vec<SocketAddr> = match pinned {
                Some(addresses) => addresses,
                None => (host.as_str(), 0).to_socket_addrs()?.collect(),
            };
            let addresses: reqwest::dns::Addrs = Box::new(addresses.into_iter());
            Ok(addresses)
        })
    }
}

impl Clients {
    /// The pages client, `host` pinned to `addresses` in it first (a
    /// name; an address is connected to as it is).
    pub(super) fn pages(
        &self,
        host: &str,
        addresses: &[SocketAddr],
    ) -> anyhow::Result<reqwest::blocking::Client> {
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let full = inner.pages.as_ref().is_some_and(|(_, pins)| {
            pins.0
                .lock()
                .is_ok_and(|p| p.len() >= crate::limits().pinned_max)
        });
        if inner.pages.is_none() || full {
            let pins = Pins::default();
            let client = reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(USER_AGENT)
                .timeout(crate::limits().timeout())
                .connect_timeout(crate::limits().connect())
                .dns_resolver(std::sync::Arc::new(pins.clone()))
                .build()?;
            inner.pages = Some((client, pins));
        }
        let (client, pins) = inner.pages.as_ref().expect("made above");
        if host.parse::<IpAddr>().is_err() && !host.starts_with('[') {
            pins.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(host.to_ascii_lowercase(), addresses.to_vec());
        }
        Ok(client.clone())
    }

    /// The search API's client, which follows no redirect: the key rides
    /// in each request's headers, and a redirect would carry it to
    /// another host, or to plain http.
    pub(super) fn search(&self) -> anyhow::Result<reqwest::blocking::Client> {
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(client) = &inner.search {
            return Ok(client.clone());
        }
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(USER_AGENT)
            .timeout(crate::limits().timeout())
            .connect_timeout(crate::limits().connect())
            .build()?;
        inner.search = Some(client.clone());
        Ok(client)
    }
}

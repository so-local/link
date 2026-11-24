//! Dialing a host the proxy's rules allow: straight, or through its
//! upstream, on [`link_sandbox_route::dial`].

use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, bail};
use link_sandbox_contract::net::Upstream;

use link_sandbox_route::dial::CONNECT_TIMEOUT;
pub use link_sandbox_route::dial::Dialed;
use link_sandbox_route::dial::UPSTREAM_GONE;
use link_sandbox_route::dial::connect_any;
use link_sandbox_route::dial::http_connect;
use link_sandbox_route::io::connect_tcp;
use link_sandbox_route::resolve::forget;
use link_sandbox_route::resolve::prefer;
use link_sandbox_route::resolve::resolve;

use crate::filter::Filter;

/// Resolves `host` here, refuses it if every address is forbidden, and
/// connects to the allowed ones in turn, each by its [`route`], until one
/// answers within [`CONNECT_TIMEOUT`]: a host whose IPv6 goes nowhere is
/// reached on IPv4. Resolving here (never at a SOCKS upstream) keeps a
/// name from pointing the exit at its own machine. With an HTTP upstream,
/// which resolves and refuses as this would, this machine's resolver is
/// asked no name a sandbox chose but one `[lan]` lists by itself or one of
/// no internet's: any other is handed upstream unresolved if the internet
/// allows it, and refused unasked if not (`[lan] *` alone never reaches
/// an internet name there). An
/// address, however a URL writes it (`2130706433`, `127.1`), is no name:
/// it's read and checked here, as any address is.
pub async fn dial(host: &str, port: u16, rules: Filter) -> anyhow::Result<Dialed> {
    let listed = rules
        .hosts
        .is_none_or(|hosts| link_sandbox_contract::net::host_allowed(host, hosts));
    let internet = rules.internet && !rules.lan_only && listed;
    let lan = crate::lan::may(host, rules.lan);
    if !internet && !lan {
        bail!("refused: {host} isn't one of the hosts this sandbox may reach");
    }
    // A name `[lan]` lets through by its own name, or a name of no
    // internet's (`printer.local`, `nas`), is your network's: dialed from
    // here. Any other goes upstream unresolved, `[lan] *` or not, or,
    // with no internet for it, nowhere: the one lookup here is below.
    let lans = lan
        && (!crate::dns::public_name(host)
            || rules
                .lan
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case(host.trim_end_matches('.'))));
    if let Upstream::Http(proxy) = rules.upstream
        && !lans
        && host.parse::<IpAddr>().is_err()
        && !crate::http::ends_in_number(host)
    {
        if !internet {
            bail!(
                "refused: {host} isn't one of the hosts this sandbox may reach \
                 (with an HTTP upstream, [lan] reaches a name only by its own)"
            );
        }
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
        .filter_map(|a| Some((*a, route(&rules, host, internet, lan, a.ip())?)))
        .collect();
    if allowed.is_empty() {
        // Whatever it was written as (`2852039166`, a name pointing
        // there): the pause for a metadata service goes by the address.
        if addrs.iter().any(|a| crate::wire::is_metadata(a.ip())) {
            return Err(crate::wire::Metadata(format!("{host}:{port}")).into());
        }
        bail!("refused: {host} is this machine, its network, or nothing reachable");
    }
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

/// Where `ip`, one of `host`'s addresses, is dialed from, if it may be at
/// all: a machine of your network (with `lan`, as [`crate::lan`] allows)
/// from here, whatever the upstream, never through it; an internet
/// address (with `internet`) through the upstream. A name with both
/// kinds of addresses gets each its own way.
pub(crate) fn route(
    rules: &Filter,
    host: &str,
    internet: bool,
    lan: bool,
    ip: IpAddr,
) -> Option<&'static Upstream> {
    if lan && crate::lan::allowed(ip, host, rules.lan) {
        return Some(&Upstream::Direct);
    }
    (internet && (rules.allowed)(ip)).then_some(rules.upstream)
}

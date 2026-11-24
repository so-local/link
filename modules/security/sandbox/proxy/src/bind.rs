//! A proxy as a [`ProxyBind`] says: each of its values checked as its
//! command line checks them, its routes mounted as they are, and who
//! calls them, whether it's started with them or bound blank
//! ([`crate::blank`]).

use anyhow::{Context, ensure};
use link_sandbox_client::ProxyBind;
use link_sandbox_contract::net::Upstream;

use crate::bridged::Calling;
use crate::filter::Filter;
use crate::listen::Folder;
use crate::outside::Bound;
use crate::routes::Routes;

/// What the proxy `bind` says serves, its sockets in `folder` (none:
/// `--listen`, macOS).
pub fn bound(bind: ProxyBind, folder: Option<Folder>) -> anyhow::Result<Bound> {
    for host in &bind.host {
        link_sandbox_contract::net::check_host(host)?;
    }
    let tcp = bind
        .tcp
        .iter()
        .map(|e| {
            link_sandbox_contract::net::check_endpoint(e)
                .map(|(host, port)| format!("{host}:{port}"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    for entry in &bind.lan {
        link_sandbox_contract::net::check_lan(entry)?;
    }
    // However it's started (a blank's bind, `lnk sandbox proxy` run
    // directly): a cloud's private ranges are its network.
    crate::lan::lan_here(&bind.lan, crate::lan::on_cloud())?;
    link_sandbox_route::check_routes(&bind.routes)?;
    for route in &bind.routes {
        ensure!(
            bind.serve.is_disjoint(&route.ports),
            "route {} serves a port the sandbox serves itself",
            route.name
        );
    }
    ensure!(
        bind.url_secret
            .as_ref()
            .is_none_or(|secret| link_sandbox_route::is_url_secret(secret.expose())),
        "the proxy's URL secret isn't one (16 to 64 letters and digits)"
    );
    ensure!(
        bind.proxy_secret
            .as_ref()
            .is_none_or(|secret| link_sandbox_route::is_url_secret(secret.expose())),
        "the proxy's secret isn't one (16 to 64 letters and digits)"
    );
    ensure!(bind.per_minute != Some(0), "a limit of 0 calls a minute");
    let upstream = Upstream::parse(&bind.upstream)?;
    let calling = match (bind.routes.is_empty(), bind.caller) {
        (true, _) => None,
        (false, None) => anyhow::bail!("a proxy with routes needs who calls them (its caller)"),
        (false, Some(caller)) => Some(Calling {
            caller,
            sandbox: bind.name.clone(),
            upstream: upstream.text(),
            inbound: folder.map(|f| f.inbound()),
        }),
    };
    link_sandbox_route::desktop::set_desktop(
        bind.desktop
            .iter()
            .filter_map(|pair| pair.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    );
    let wire = match bind.wire {
        Some(dir) => crate::wire::Wire::new(&bind.name, dir),
        None => crate::wire::Wire::here(&bind.name).context("finding the wire")?,
    }
    .limited(bind.per_minute);
    // What it carries, for `lnk sandbox show` and `lnk measure`.
    crate::wire::keep_traffic(&wire);
    let routes = Routes {
        routes: bind.routes,
    };
    let mut filter = Filter::new(upstream)
        .only(bind.host)
        .with_tcp(tcp)
        .with_routes(routes, bind.url_secret)
        .with_proxy_secret(bind.proxy_secret)
        .with_lan(bind.lan, !bind.no_internet)
        .on(wire);
    if let Some(calling) = calling {
        filter = filter.calling(calling);
    }
    Ok(Bound {
        filter,
        serve: bind.serve,
        reach: bind.reach,
    })
}

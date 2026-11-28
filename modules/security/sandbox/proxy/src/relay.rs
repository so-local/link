//! The small relays across a sandbox's network (Linux): out to the
//! proxy, to this machine's ports, and in to the program's.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};

use crate::filter::Filter;
use crate::http::{AnswerHead, read_head};
use crate::listen::{each, listen_inside, listen_unix};
use crate::routes::Client;
use crate::run::{raise_open_files, run};
use crate::serve::watched;
use link_sandbox_route::Inbound;
use link_sandbox_route::io::connect_tcp;
use link_sandbox_route::io::pipe;

/// Of `endpoints` (a policy's `tcp`), those a program reaches by name
/// in the sandbox's own network (Linux): a named host, a port from 1024
/// (a sandbox can't listen below it), each on an address of its own,
/// `127.0.1.1` on, which the sandbox's `/etc/hosts` gives its name. The
/// rest are reached through `sandbox connect` only.
pub fn named_endpoints(endpoints: &[String]) -> Vec<(String, u16, std::net::Ipv4Addr)> {
    endpoints
        .iter()
        .filter_map(|e| link_sandbox_contract::net::check_endpoint(e).ok())
        .filter(|(host, port)| {
            *port >= 1024 && host.parse::<IpAddr>().is_err() && !host.starts_with("*.")
        })
        .take(254)
        .enumerate()
        .map(|(i, (host, port))| (host, port, std::net::Ipv4Addr::new(127, 0, 1, i as u8 + 1)))
        .collect()
}

/// The sandbox's `/etc/hosts`: this machine's, and each of `named` its
/// `hosts` allow (empty: any) at its own address, where it reaches the
/// internet. A name they refuse gets none: it would reach nothing.
pub fn etc_hosts(
    system: &str,
    named: &[(String, u16, std::net::Ipv4Addr)],
    hosts: &[String],
    internet: bool,
) -> String {
    let mut text = system.trim_end().to_string();
    text.push_str("\n# Link: endpoints its proxy reaches (tcp)\n");
    let mut seen = BTreeSet::new();
    let allowed = |host: &str| {
        internet && (hosts.is_empty() || link_sandbox_contract::net::host_allowed(host, hosts))
    };
    for (host, _, addr) in named {
        if allowed(host) && seen.insert(host) {
            text.push_str(&format!("{addr} {host}\n"));
        }
    }
    text
}

/// `sandbox proxy-inside`: in the sandbox. Serves `127.0.0.1:port` (the
/// program's proxy), this machine's `reach` ports (through
/// `dir/local.sock`), each of `tcp`'s [`named_endpoints`] at its own
/// address (through the proxy), and `dir/in.sock` (Link reaching the
/// program's ports); runs `argv` (the program), and returns its exit
/// code. Each of `also` is another proxy's folder and the port inside
/// it's reached at, served the same way: one sandbox for several
/// programs' proxies. With `shared`, the folders are shared with their
/// proxies by a group ([`crate::listen::Sharing::Group`]), and each
/// `in.sock` is the folder's group's too (`0660`); else this user's
/// alone.
pub async fn inside(
    dir: &Path,
    shared: bool,
    port: u16,
    also: &[(PathBuf, u16)],
    reach: &BTreeSet<u16>,
    tcp: &[String],
    argv: &[String],
) -> anyhow::Result<i32> {
    raise_open_files();
    let proxy = TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("listening on 127.0.0.1:{port} in the sandbox"))?;
    tokio::spawn(relay_out(proxy, dir.join("out.sock")));
    // A `*.` endpoint's port, on every address of its loopback: its DNS
    // gives each name under it one of its own (`127.1.0.1` on), and the
    // proxy takes the address for the name. A name listed outright on
    // that port too is reached the same way.
    let wild: BTreeSet<u16> = tcp
        .iter()
        .filter_map(|e| link_sandbox_contract::net::check_endpoint(e).ok())
        .filter(|(host, port)| host.starts_with("*.") && *port >= 1024)
        .map(|(_, port)| port)
        .collect();
    for &port in &wild {
        let listener = TcpListener::bind(("0.0.0.0", port))
            .await
            .with_context(|| format!("listening on port {port} in the sandbox"))?;
        let out = dir.join("out.sock");
        tokio::spawn(each(listener, move |client: TcpStream| {
            let out = out.clone();
            async move {
                let Ok(at) = client.local_addr() else {
                    return;
                };
                connect_through(client, &out, &format!("{}:{port}", at.ip())).await;
            }
        }));
    }
    // A host's address is the same for all its ports.
    let mut addresses = BTreeMap::new();
    for (host, port, addr) in named_endpoints(tcp) {
        if wild.contains(&port) {
            continue;
        }
        let addr = *addresses.entry(host.clone()).or_insert(addr);
        let listener = TcpListener::bind((addr, port))
            .await
            .with_context(|| format!("listening on {addr}:{port} in the sandbox"))?;
        tokio::spawn(relay_endpoint(listener, dir.join("out.sock"), host, port));
    }
    for &port in reach {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .with_context(|| format!("listening on 127.0.0.1:{port} in the sandbox"))?;
        tokio::spawn(relay_local(listener, dir.join("local.sock"), port));
    }
    // Inside, where the group can't be named: the folder's, which it
    // takes as it's made there.
    let mode = if shared { 0o660 } else { 0o600 };
    let inbound = listen_inside(&dir.join("in.sock"), mode)?;
    tokio::spawn(relay_in(inbound));
    for (dir, port) in also {
        let proxy = TcpListener::bind(("127.0.0.1", *port))
            .await
            .with_context(|| format!("listening on 127.0.0.1:{port} in the sandbox"))?;
        tokio::spawn(relay_out(proxy, dir.join("out.sock")));
        let inbound = listen_inside(&dir.join("in.sock"), mode)?;
        tokio::spawn(relay_in(inbound));
    }
    run(argv).await
}

/// `sandbox loopback`: outside a sandbox whose only network is this
/// machine's `reach` ports. Serves them on `dir/local.sock`, and nothing
/// else: no proxy, no wire, no keys. Runs `argv` (the sandboxed program)
/// and returns its exit code.
pub async fn loopback_outside(
    dir: &Path,
    reach: &BTreeSet<u16>,
    argv: &[String],
) -> anyhow::Result<i32> {
    let local = listen_unix(&dir.join("local.sock"), Default::default())?;
    tokio::spawn(serve_local(
        local,
        Arc::new(reach.clone()),
        Filter::new(link_sandbox_contract::net::Upstream::Direct),
    ));
    run(argv).await
}

/// `sandbox loopback --inside`: in that sandbox. Serves this machine's
/// `reach` ports on its own loopback, through `dir/local.sock`; runs
/// `argv` (the program) and returns its exit code.
pub async fn loopback_inside(
    dir: &Path,
    reach: &BTreeSet<u16>,
    argv: &[String],
) -> anyhow::Result<i32> {
    for &port in reach {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .with_context(|| format!("listening on 127.0.0.1:{port} in the sandbox"))?;
        tokio::spawn(relay_local(listener, dir.join("local.sock"), port));
    }
    run(argv).await
}

/// Inside: each connection to the proxy port, to `out.sock`.
async fn relay_out(listener: TcpListener, out: PathBuf) {
    each(listener, move |mut client| {
        let out = out.clone();
        async move {
            if let Ok(mut upstream) = UnixStream::connect(&out).await {
                let _ = pipe(&mut client, &mut upstream).await;
            }
        }
    })
    .await
}

/// Inside: each connection to an endpoint's address, through the proxy
/// on `out.sock` with `CONNECT host:port`, which the proxy checks and
/// logs as any other.
async fn relay_endpoint(listener: TcpListener, out: PathBuf, host: String, port: u16) {
    each(listener, move |client| {
        let (out, host) = (out.clone(), host.clone());
        async move { connect_through(client, &out, &format!("{host}:{port}")).await }
    })
    .await
}

/// Inside: `client` joined to `target` (`host:port`) through the proxy
/// on `out`, by `CONNECT`, which the proxy checks and logs as any other.
async fn connect_through(mut client: TcpStream, out: &Path, target: &str) {
    let Ok(mut upstream) = UnixStream::connect(out).await else {
        return;
    };
    let request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n");
    if upstream.write_all(request.as_bytes()).await.is_err() {
        return;
    }
    let Ok((head, rest)) = read_head(&mut upstream).await else {
        return;
    };
    let established = AnswerHead::parse(&head).is_ok_and(|answer| answer.code == 200);
    if !established || client.write_all(&rest).await.is_err() {
        return;
    }
    let _ = pipe(&mut client, &mut upstream).await;
}

/// Inside: each connection to one of this machine's ports, to
/// `local.sock`, naming the port (two bytes).
async fn relay_local(listener: TcpListener, local: PathBuf, port: u16) {
    each(listener, move |mut client| {
        let local = local.clone();
        async move {
            if let Ok(mut link) = UnixStream::connect(&local).await
                && link.write_u16(port).await.is_ok()
            {
                let _ = pipe(&mut client, &mut link).await;
            }
        }
    })
    .await
}

/// Outside: each connection on `local.sock` names a port, and is joined to
/// it on this machine's loopback if it's one of `reach`, through its route
/// if `routes` has one for it. The port comes from inside the sandbox, so
/// it's checked here.
pub async fn serve_local(listener: UnixListener, reach: Arc<BTreeSet<u16>>, filter: Filter) {
    let (routes, wire) = (filter.routes, filter.wire);
    each(listener, move |mut link| {
        let reach = reach.clone();
        async move {
            let Some(port) = port_named(&mut link, PORT_WAIT).await else {
                return;
            };
            let mut call = crate::wire::Logged(crate::wire::Call::on(wire));
            call.verb = Some(
                routes
                    .port(port)
                    .map_or("port", |route| route.name.as_str()),
            );
            call.to = format!("127.0.0.1:{port}");
            let link = crate::wire::Counted::new(link, &call);
            let serving = local_port(link, port, &reach, filter, &mut call);
            let result = watched(wire, serving).await;
            call.end(&result);
        }
    })
    .await
}

/// How long a connection on `local.sock` or `in.sock` has to name its
/// port: one that names none ends, so a sandbox can't hold every place
/// on its sockets by saying nothing.
const PORT_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The port `link`'s first two bytes name, within `wait`.
pub(crate) async fn port_named(link: &mut UnixStream, wait: std::time::Duration) -> Option<u16> {
    tokio::time::timeout(wait, link.read_u16()).await.ok()?.ok()
}

/// One connection to this machine's `port`, if it's one of `reach`,
/// through its route if `routes` has one for it.
async fn local_port<S>(
    mut link: S,
    port: u16,
    reach: &BTreeSet<u16>,
    filter: Filter,
    call: &mut crate::wire::Call,
) -> anyhow::Result<()>
where
    S: Client + 'static,
{
    let wire = filter.wire;
    if !reach.contains(&port) {
        bail!("refused: port {port} isn't one this sandbox may reach");
    }
    if let Some(why) = wire.and_then(|w| w.why_stopped()) {
        bail!("refused: {why}");
    }
    if let Some((_, why)) = wire.and_then(|w| w.over_limit()) {
        call.limited = true;
        bail!("refused: {why}");
    }
    // A port a route serves (a model runtime's, for inference only),
    // which refuses what it doesn't serve itself.
    if let Some(route) = filter.routes.port(port) {
        let to = link_sandbox_route::To::Port { port };
        return crate::bridged::serve(link, route, to, Vec::new(), filter, call).await;
    }
    let mut local = connect_tcp(("127.0.0.1", port)).await?;
    call.allowed();
    pipe(&mut link, &mut local).await?;
    Ok(())
}

/// Inside: each connection on `in.sock` names a port (two bytes), and is
/// joined to that port in the sandbox.
async fn relay_in(listener: UnixListener) {
    each(listener, |mut link| async move {
        let Some(port) = port_named(&mut link, PORT_WAIT).await else {
            return;
        };
        if let Ok(mut harness) = connect_tcp(("127.0.0.1", port)).await {
            let _ = pipe(&mut link, &mut harness).await;
        }
    })
    .await
}

/// How long a connection to a served port may send nothing before it's
/// dropped: held open with nothing said, many would fill the sandbox's.
const HEAD_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// Whether `client` says something within `wait`: its first bytes,
/// read only to look (`peek`), so all it sends reaches the sandbox.
async fn speaks(client: &TcpStream, wait: std::time::Duration) -> bool {
    let mut first = [0u8; 1];
    matches!(
        tokio::time::timeout(wait, client.peek(&mut first)).await,
        Ok(Ok(n)) if n > 0
    )
}

/// Outside: each connection to one of the harness's ports, into the
/// sandbox through `in.sock`, once it says something within
/// [`HEAD_WAIT`]: one that says nothing holds nothing of the sandbox's.
pub async fn serve_inbound(listener: TcpListener, inbound: Inbound, port: u16) {
    each(listener, move |mut client| {
        let inbound = inbound.clone();
        async move {
            if !speaks(&client, HEAD_WAIT).await {
                return;
            }
            if let Ok(mut link) = inbound.connect().await
                && link.write_u16(port).await.is_ok()
            {
                let _ = pipe(&mut client, &mut link).await;
            }
        }
    })
    .await
}

#[cfg(test)]
mod head_tests {
    /// A served port holds a connection only once it says something in
    /// time: silent, it's dropped; speaking, all it said is still there.
    #[tokio::test]
    async fn a_served_port_holds_a_connection_only_until_its_head_is_due() {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = listener.local_addr().unwrap();
        let _silent = tokio::net::TcpStream::connect(at).await.unwrap();
        let (client, _) = listener.accept().await.unwrap();
        let wait = std::time::Duration::from_millis(50);
        assert!(!super::speaks(&client, wait).await, "said nothing");
        let mut speaking = tokio::net::TcpStream::connect(at).await.unwrap();
        speaking.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
        let (client, _) = listener.accept().await.unwrap();
        assert!(super::speaks(&client, std::time::Duration::from_secs(5)).await);
        let mut first = [0u8; 3];
        assert_eq!(client.peek(&mut first).await.unwrap(), 3);
        assert_eq!(&first, b"GET");
    }
}

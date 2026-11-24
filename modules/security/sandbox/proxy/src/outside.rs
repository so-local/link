//! The proxy outside its sandbox: started as the sandbox's parent with
//! what it was bound with, held, or blank until it's bound.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, ensure};
use link_sandbox_client::{ProxyBind, ProxyBound};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UnixListener, UnixStream};

use crate::filter::Filter;
use crate::http::{HEAD_TIMEOUT, read_head};
use crate::listen::{Folder, each, listen_unix};
use crate::relay::{named_endpoints, serve_inbound, serve_local};
use crate::run::{raise_open_files, run};
use crate::serve::proxy;

/// What a proxy serves, made from what it was given: its rules, the
/// program's ports it serves on this machine's loopback, and this
/// machine's ports it reaches.
pub struct Bound {
    pub filter: Filter,
    pub serve: BTreeSet<u16>,
    pub reach: BTreeSet<u16>,
}

/// `sandbox proxy`: outside the sandbox. Serves the proxy on
/// `dir/out.sock`, this machine's `reach` ports on `dir/local.sock`, and
/// the program's `serve` ports on this machine's loopback; runs `argv`
/// (the sandboxed program), and returns its exit code. With `dns`, the
/// place of bubblewrap's command in `argv`, it answers the sandbox's
/// names too ([`crate::dns`], Linux).
pub async fn outside(
    folder: Folder<'_>,
    bound: Bound,
    dns: Option<usize>,
    argv: &[String],
) -> anyhow::Result<i32> {
    raise_open_files();
    folder.sharing.check(folder.dir)?;
    let filter = bound.filter;
    let out = listen_unix(&folder.dir.join("out.sock"), folder.sharing.given())?;
    tokio::spawn(serve_proxy(out, filter));
    beside(folder, bound).await?;
    // Held alone: its sandbox is another's, started apart.
    if argv.is_empty() {
        return std::future::pending().await;
    }
    match dns {
        #[cfg(target_os = "linux")]
        Some(bwrap) => crate::run::run_with_dns(argv, bwrap, filter).await,
        _ => run(argv).await,
    }
}

/// What a proxy serves beside `dir/out.sock`: this machine's `reach`
/// ports on `dir/local.sock`, the program's `serve` ports on this
/// machine's loopback, and the names of its endpoints.
async fn beside(folder: Folder<'_>, bound: Bound) -> anyhow::Result<()> {
    let Bound {
        filter,
        serve,
        reach,
    } = bound;
    let mut served = Vec::new();
    for port in serve {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .with_context(|| format!("listening on 127.0.0.1:{port} for the sandbox"))?;
        served.push((port, listener));
    }
    let local = listen_unix(&folder.dir.join("local.sock"), folder.sharing.given())?;
    // A connection to an address the sandbox's DNS or `/etc/hosts` gave
    // one of its endpoints' names asks for that name.
    if let Ok(mut names) = crate::dns::NAMES.lock() {
        *names = Some(crate::dns::Names::with(&named_endpoints(filter.tcp)));
    }
    tokio::spawn(serve_local(local, Arc::new(reach), filter));
    for (port, listener) in served {
        tokio::spawn(serve_inbound(listener, folder.inbound(), port));
    }
    Ok(())
}

/// `sandbox proxy --blank` (Linux): a proxy holding nothing of anyone's
/// until it's bound to one agent. Serves `dir/out.sock`, answering every
/// call 503, and `control`, a socket only this user may use (0600), which
/// takes one [`ProxyBind`] line and answers one line ([`ProxyBound`]):
/// from then on it serves as [`outside`] does, started with it. Bound
/// once: another bind is refused, and the binding ends with the process.
/// Runs `argv` (none: held alone) and returns its exit code.
pub async fn blank(
    folder: Folder<'_>,
    control: &Path,
    binder: Option<u32>,
    argv: &[String],
) -> anyhow::Result<i32> {
    raise_open_files();
    // Set once, when it's bound; it lives as long as the proxy, which is
    // the process.
    let rules: &'static std::sync::OnceLock<Filter> = Box::leak(Box::default());
    let (dir, sharing) = (folder.dir, folder.sharing);
    sharing.check(dir)?;
    let out = listen_unix(&dir.join("out.sock"), sharing.given())?;
    tokio::spawn(each(out, move |mut client: UnixStream| async move {
        match rules.get() {
            Some(filter) => {
                let _ = proxy(client, *filter).await;
            }
            None => not_bound(&mut client).await,
        }
    }));
    let control = link_sandbox_bridge::socket::listen_async(
        control,
        link_sandbox_bridge::socket::Given::default(),
    )?;
    let dir = dir.to_path_buf();
    let binding = Arc::new(tokio::sync::Mutex::new(()));
    tokio::spawn(each(control, move |mut client: UnixStream| {
        let (dir, binding) = (dir.clone(), binding.clone());
        async move {
            let bound = match bind_line(&mut client, binder).await {
                Ok(line) => {
                    let _one_at_a_time = binding.lock().await;
                    let folder = Folder { dir: &dir, sharing };
                    bind_blank(line, folder, rules).await
                }
                Err(err) => Err(err),
            };
            let answer = ProxyBound {
                error: bound.err().map(|err| format!("{err:#}")),
            };
            let line = serde_json::to_string(&answer).unwrap_or_default() + "\n";
            let _ = client.write_all(line.as_bytes()).await;
        }
    }));
    if argv.is_empty() {
        return std::future::pending().await;
    }
    run(argv).await
}

/// The most a [`ProxyBind`] line may be.
const MAX_BIND: u64 = 1 << 20;

/// The [`ProxyBind`] on `client`'s one line, from this user, or its
/// `binder` (`--binder`), only. Anyone else's line is read and dropped
/// unread, so its sender is told why, never cut off as it writes.
async fn bind_line(client: &mut UnixStream, binder: Option<u32>) -> anyhow::Result<ProxyBind> {
    use tokio::io::AsyncBufReadExt;
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    let peer = client.peer_cred()?.uid();
    let mut line = String::new();
    let mut reader = tokio::io::BufReader::new((&mut *client).take(MAX_BIND));
    let read = tokio::time::timeout(HEAD_TIMEOUT, reader.read_line(&mut line)).await;
    ensure!(
        peer == me || Some(peer) == binder,
        "refused: only this proxy's user, or its binder, binds it"
    );
    read.context("no bind in time")??;
    serde_json::from_str(&line).context("reading the bind")
}

/// Binds the blank proxy of `folder` whose rules are set in `rules`, with
/// `line`, unless it's bound already.
async fn bind_blank(
    line: ProxyBind,
    folder: Folder<'_>,
    rules: &'static std::sync::OnceLock<Filter>,
) -> anyhow::Result<()> {
    ensure!(
        rules.get().is_none(),
        "this proxy is bound already: its binding ends with it"
    );
    let bound = crate::bind::bound(line, Some(folder))?;
    let filter = bound.filter;
    beside(folder, bound).await?;
    let _ = rules.set(filter);
    Ok(())
}

/// A blank proxy's answer to every call: 503, nobody's yet.
async fn not_bound(client: &mut UnixStream) {
    let _ = tokio::time::timeout(HEAD_TIMEOUT, read_head(client)).await;
    let why = "Link's sandbox: this proxy isn't bound to an agent yet\n";
    let _ = client
        .write_all(
            format!(
                "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{why}",
                why.len()
            )
            .as_bytes(),
        )
        .await;
}

/// `sandbox proxy --listen` (macOS): serves the proxy on
/// `127.0.0.1:port`, runs `argv` (the sandboxed program, which may connect
/// to that port and no other of the network's), and returns its exit code.
pub async fn listening(port: u16, filter: Filter, argv: &[String]) -> anyhow::Result<i32> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("listening on 127.0.0.1:{port} for the sandbox"))?;
    listening_on(listener, filter, argv, &Default::default()).await
}

/// A listener already owned by the proxy, held through preparation of
/// its program. It is never released between choosing and using a port.
pub async fn listening_on(
    listener: TcpListener,
    filter: Filter,
    argv: &[String],
    env: &std::collections::BTreeMap<String, String>,
) -> anyhow::Result<i32> {
    raise_open_files();
    tokio::spawn(each(listener, move |client| async move {
        let _ = proxy(client, filter).await;
    }));
    crate::run::run_at(argv, env).await
}

async fn serve_proxy(listener: UnixListener, filter: Filter) {
    each(listener, move |client| async move {
        let _ = proxy(client, filter).await;
    })
    .await
}

//! The hidden steps around a sandbox: `wrap`, its proxy, the one inside, and their helpers.

use std::path::PathBuf;

use anyhow::{bail, ensure};
use link_format::{Done, Out, Report, WithinExt};
use link_sandbox::cli::{self};
use link_sandbox::ports;
use link_sandbox_client::{ProxyBind, Wrap, WrapRequest};
use link_sandbox_proxy::wire;

use super::*;

pub(crate) fn wrap(out: &mut Out) -> Result<Done, Report> {
    let request: WrapRequest =
        serde_json::from_reader(std::io::stdin().lock()).within("reading the request on stdin")?;
    // Asked to hand, its proxy's routes are printed to the caller's pipe,
    // never written.
    let handing = match (request.sandbox, request.hand) {
        (Wrap::Unwalled(unwalled), hand) => link_sandbox::unwalled(&unwalled, &request.argv, hand)?,
        (sandbox, true) => link_sandbox::wrap_handed(&door(sandbox)?, &request.argv)?,
        (sandbox, false) => link_sandbox_client::Handing {
            wrapped: link_sandbox::wrap(&door(sandbox)?, &request.argv)?,
            ..Default::default()
        },
    };
    // A protocol on stdout: its caller reads this and nothing else.
    out.data(
        format!(
            "{}\n",
            serde_json::to_string(&handing).within("writing what wrap made")?
        )
        .as_bytes(),
    );
    Ok(Done::shown())
}

pub(crate) fn proxy(given: cli::SandboxProxyArgs) -> Result<Done, Report> {
    Ok(Done::passed(proxying(given)?))
}

/// The proxy, until its program ends: its exit status.
fn proxying(
    cli::SandboxProxyArgs {
        name,
        wire: wire_dir,
        dir,
        listen,
        launch,
        upstream,
        blank,
        control,
        binder,
        agent_user,
        group,
        host,
        tcp,
        serve,
        reach,
        desktop,
        lan,
        no_internet,
        per_minute,
        routes,
        routes_handed,
        url_secret,
        pointed_by,
        delete_routes,
        dns,
        hold,
        argv,
    }: cli::SandboxProxyArgs,
) -> anyhow::Result<i32> {
    // What a flag left out means.
    let name = name.unwrap_or_default();
    let serve = serve.unwrap_or_default();
    let reach = reach.unwrap_or_default();
    let tcp = endpoints(&tcp)?;
    // The listener its starter chose its port by, handed open: taken
    // before anything else runs, so nothing this starts holds it.
    let handed = link_sandbox_proxy::listen::handed(listen)?;
    // Held alone, or running its program: one or the other.
    ensure!(
        hold == argv.is_empty(),
        "lnk sandbox proxy runs a program after --, or none with --hold"
    );
    // Run as a user other than its agent's, sharing its folder by a group.
    let sharing = link_sandbox_proxy::listen::Sharing::of(agent_user, group)?;
    // Nobody's until it's bound, on its control socket.
    if blank {
        let (Some(dir), Some(control)) = (dir, control) else {
            bail!("--blank needs --dir and --control");
        };
        let runtime = runtime()?;
        let folder = link_sandbox_proxy::listen::Folder { dir: &dir, sharing };
        let code = runtime.block_on(link_sandbox_proxy::blank(folder, &control, binder, &argv))?;
        runtime.shutdown_timeout(std::time::Duration::from_secs(1));
        wire::end_traffic();
        return Ok(code);
    }
    // Read before anything starts: the pipe they came on is closed.
    let routed: link_sandbox::Routed = match (&routes, &routes_handed) {
        // Held zeroed, as handed routes are, and dropped once parsed.
        (Some(file), _) => {
            let text = std::fs::File::open(file)
                .and_then(|mut file| link_secretive::Secret::read_all(&mut file))
                .within(format!("reading {}", file.display()))?;
            let routed: link_sandbox::Routed =
                link_files::saved::parse_json(text.expose().as_bytes())
                    .within(format!("reading {}", file.display()))?;
            routed
                .names_only()
                .within(format!("reading {}", file.display()))?;
            routed
        }
        (None, Some(variable)) => {
            let handed = link_secretive::Secret::from_env(variable)
                .within(format!("no routes were handed as {variable}"))?;
            serde_json::from_str(handed.expose())
                .within(format!("reading the routes handed as {variable}"))?
        }
        (None, None) => Default::default(),
    };
    // A run's: held in memory from here, never at rest on the disk
    // while it runs, nor after it's stopped.
    if delete_routes && let Some(file) = &routes {
        let _ = std::fs::remove_file(file);
    }
    // Who calls its routes, bound for the run: unbound as it ends.
    let unbind = routed.unbind_from.clone().zip(
        routed
            .caller
            .as_ref()
            .map(link_secretive::Duplicate::duplicate),
    );
    let url_secret = url_secret
        .as_deref()
        .map(|file| -> anyhow::Result<String> {
            let secret =
                std::fs::read_to_string(file).within(format!("reading {}", file.display()))?;
            let secret = secret.trim().to_string();
            ensure!(
                link_sandbox_route::is_url_secret(&secret),
                "{} holds no proxy secret",
                file.display()
            );
            Ok(secret)
        })
        .transpose()?;
    // The run's secret, in the address that points its program here, and
    // nowhere else. Started without it, its program isn't pointed here
    // either: one nobody holds, so no way out is taken.
    let proxy_secret = pointed_by
        .as_deref()
        .map(|variable| -> anyhow::Result<link_secretive::Secret> {
            let pointed = std::env::var(variable)
                .ok()
                .and_then(|address| link_sandbox_proxy::client::secret_in(&address))
                .filter(|secret| link_sandbox_route::is_url_secret(secret));
            Ok(link_secretive::Secret::from(match pointed {
                Some(secret) => secret,
                None => link_sandbox_route::url_secret()?,
            }))
        })
        .transpose()?;
    // A terminal it was started from answers what its routes ask you.
    if let Some(dir) = &wire_dir {
        link_sandbox::settings::read_from(dir);
        link_sandbox::bridges::asks::answer_on_terminal(dir.clone(), name.clone());
    }
    let bind = ProxyBind {
        name,
        wire: wire_dir,
        upstream: upstream.within("--upstream")?,
        host,
        tcp,
        serve: ports(&serve)?,
        reach: ports(&reach)?,
        desktop,
        lan,
        no_internet,
        per_minute,
        routes: routed.routes,
        caller: routed.caller,
        url_secret: url_secret.map(link_secretive::Secret::from),
        proxy_secret,
    };
    let folder = dir
        .as_deref()
        .map(|dir| link_sandbox_proxy::listen::Folder { dir, sharing });
    let bound = link_sandbox_proxy::bound(bind, folder)?;
    let runtime = runtime()?;
    let code = match (folder, listen) {
        (Some(folder), _) => runtime.block_on(link_sandbox_proxy::outside(
            folder,
            bound,
            dns_at(dns, &argv),
            &argv,
        ))?,
        (None, Some(port)) => runtime.block_on(async {
            // Served as handed, never let go of and bound again; else
            // bound here, before its program is given the port.
            let listener = match handed {
                Some(listener) => {
                    listener.set_nonblocking(true)?;
                    tokio::net::TcpListener::from_std(listener)?
                }
                None => tokio::net::TcpListener::bind(("127.0.0.1", port))
                    .await
                    .within(format!("listening on 127.0.0.1:{port} for the sandbox"))?,
            };
            match &launch {
                Some(path) => {
                    let launch = link_sandbox::Launch::read(path)?;
                    if delete_routes {
                        let _ = std::fs::remove_file(path);
                    }
                    let port = listener.local_addr()?.port();
                    let prepared = launch.prepare(port, &argv)?;
                    link_sandbox_proxy::outside::listening_on(
                        listener,
                        bound.filter,
                        &prepared.argv,
                        &prepared.env,
                    )
                    .await
                }
                None => {
                    link_sandbox_proxy::outside::listening_on(
                        listener,
                        bound.filter,
                        &argv,
                        &Default::default(),
                    )
                    .await
                }
            }
        })?,
        (None, None) => bail!("--dir or --listen"),
    };
    // The program is over: dropping the proxy's calls still open
    // logs each as ended.
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    wire::end_traffic();
    if let Some((keys, caller)) = unbind {
        link_sandbox_binding::Keyring::at(&keys)
            .unbind_key(&caller, &link_secret_contract::keeper::RouteRevoker);
    }
    Ok(code)
}

/// `sandbox bridge`: the sandbox's own routes' bridge, until idle.
pub(crate) fn bridge(started: link_sandbox_route::bridge::Started) -> Result<Done, Report> {
    runtime()
        .within("starting a runtime")?
        .block_on(link_sandbox::bridges::serve(&started))?;
    Ok(Done::shown())
}

/// With `dns`, where bubblewrap's command is in `argv`, for the proxy to
/// answer its sandbox's names (Linux).
fn dns_at(dns: bool, argv: &[String]) -> Option<usize> {
    #[cfg(target_os = "linux")]
    return dns.then(|| link_sandbox_confine::linux::mounts::command_at(argv));
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (dns, argv);
        None
    }
}

pub(crate) fn lookup_command(
    cli::SandboxLookupArgs { service }: cli::SandboxLookupArgs,
) -> Result<Done, Report> {
    Ok(Done::passed(match lookup(&service)? {
        true => 0,
        false => 1,
    }))
}

pub(crate) fn proxy_inside(given: cli::SandboxProxyInsideArgs) -> Result<Done, Report> {
    Ok(Done::passed(inside(given)?))
}

fn inside(
    cli::SandboxProxyInsideArgs {
        dir,
        port,
        shared,
        also,
        reach,
        tcp,
        argv,
    }: cli::SandboxProxyInsideArgs,
) -> anyhow::Result<i32> {
    let tcp = endpoints(&tcp)?;
    let reach = ports(&reach.unwrap_or_default())?;
    let also = also
        .iter()
        .map(|a| {
            let (dir, port) = a.rsplit_once('=').within("--also is DIR=PORT")?;
            let dir = PathBuf::from(dir);
            ensure!(
                dir.is_absolute(),
                "--also {a}: {} isn't an absolute path",
                dir.display()
            );
            let port: u16 = port.parse().within(format!("--also {a}: no port"))?;
            Ok((dir, port))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let code = runtime()?.block_on(link_sandbox_proxy::inside(
        &dir, shared, port, &also, &reach, &tcp, &argv,
    ))?;
    Ok(code)
}

pub(crate) fn loopback(given: cli::SandboxLoopbackArgs) -> Result<Done, Report> {
    Ok(Done::passed(looping(given)?))
}

fn looping(
    cli::SandboxLoopbackArgs {
        dir,
        reach,
        inside,
        argv,
    }: cli::SandboxLoopbackArgs,
) -> anyhow::Result<i32> {
    let reach = ports(&reach.unwrap_or_default())?;
    let runtime = runtime()?;
    let code = match inside {
        true => runtime.block_on(link_sandbox_proxy::loopback_inside(&dir, &reach, &argv))?,
        false => runtime.block_on(link_sandbox_proxy::loopback_outside(&dir, &reach, &argv))?,
    };
    Ok(code)
}

pub(crate) fn connect(
    cli::SandboxConnectArgs { host, port }: cli::SandboxConnectArgs,
) -> Result<Done, Report> {
    runtime()
        .within("starting a runtime")?
        .block_on(link_sandbox_proxy::connect(&host, port))?;
    Ok(Done::shown())
}

/// Each endpoint as `host:port`, as the proxy matches it.
fn endpoints(tcp: &[String]) -> anyhow::Result<Vec<String>> {
    tcp.iter()
        .map(|endpoint| {
            link_sandbox_contract::net::check_endpoint(endpoint)
                .map(|(host, port)| format!("{host}:{port}"))
        })
        .collect()
}

/// A runtime for one of the sandbox's processes: a few worker threads,
/// never one per CPU. A sandboxed agent runs several of these (its proxy,
/// the one inside, each remote tool), and a big machine has many CPUs.
pub(crate) fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    let cpus = std::thread::available_parallelism().map_or(2, |n| n.get());
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(cpus.clamp(1, 4))
        .enable_all()
        .build()
}

/// `sandbox lookup`: whether the macOS service `name` can be reached from
/// here (its sandbox's rules allow looking it up).
#[cfg(target_os = "macos")]
pub(crate) fn lookup(name: &str) -> anyhow::Result<bool> {
    unsafe extern "C" {
        static bootstrap_port: u32;
        fn bootstrap_look_up(bp: u32, name: *const std::ffi::c_char, port: *mut u32) -> i32;
    }
    let name = std::ffi::CString::new(name)?;
    let mut port = 0u32;
    // SAFETY: a plain call into libSystem with a NUL-terminated name and a
    // place for the port; the process exits right after.
    Ok(unsafe { bootstrap_look_up(bootstrap_port, name.as_ptr(), &mut port) } == 0)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn lookup(_name: &str) -> anyhow::Result<bool> {
    bail!("services are looked up by name on macOS only")
}

/// `sandbox dns-socket`: the sockets of a sandbox's resolver, made in its
/// network and handed to its proxy.
#[cfg(target_os = "linux")]
pub(crate) fn dns_socket(pid: u32, fd: i32) -> Result<Done, Report> {
    link_sandbox_proxy::dns::listen_in(pid, fd)?;
    Ok(Done::shown())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn dns_socket(_pid: u32, _fd: i32) -> Result<Done, Report> {
    Err(anyhow::anyhow!("a sandbox's DNS runs on Linux only").into())
}

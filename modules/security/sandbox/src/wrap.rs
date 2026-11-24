//! `wrap`: a program wrapped to run inside its confinement on this OS,
//! by its network: the wall alone where it's closed or the host's; on
//! Linux, this machine's ports relayed in ([`looped`]); its proxy, in a
//! network of its own ([`namespaced`]) or on this machine's loopback
//! ([`listening`]); other sandboxes' proxies ([`reaching`]).

mod args;
mod launch;
mod listening;
mod looped;
mod namespaced;
mod reaching;
mod routed;

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::ensure;
use link_format::WithinExt;
use link_sandbox_client::{Confinement, Handed, Handing, Net, Proxy, Unwalled, Wrapped};
use link_sandbox_contract::net::Upstream;

use crate::{bridges, check_name, rules_file, spec};
use listening::on_loopback;
use looped::looped;
use namespaced::namespaced;
use reaching::reaching;

pub use launch::Launch;
pub use listening::seatbelted;
pub use namespaced::{etc_dir, etc_hosts_path, resolv_conf_path};
pub use routed::{ROUTES_HANDED, Routed, handed, routes_file};

/// `argv`, wrapped to run inside `confinement` on this OS, and what to
/// add to its environment; the rules it needs (macOS) written first,
/// where nothing sandboxed can edit them. A confinement opening Link's
/// settings, a folder inside them or a write of one holding them, is
/// refused.
pub fn wrap(confinement: &Confinement, argv: &[String]) -> anyhow::Result<Wrapped> {
    wrap_as(confinement, argv, false, Secrets::Files, Files::Resolve)
}

/// [`wrap`], for `lnk sandbox learn` on macOS: every program it runs is
/// reported in the system log, allowed ([`reports`]).
pub fn wrap_learning(
    confinement: &crate::spec::Resolved,
    argv: &[String],
) -> anyhow::Result<Wrapped> {
    wrap_as(confinement, argv, true, Secrets::Files, Files::Resolved)
}

/// [`wrap`], for a caller that starts what it returns at once, handing
/// its proxy its routes ([`handed`]) on a pipe: none is written.
pub fn wrap_handing(
    confinement: &crate::spec::Resolved,
    argv: &[String],
) -> anyhow::Result<Wrapped> {
    wrap_as(confinement, argv, false, Secrets::Handed, Files::Resolved)
}

/// [`wrap`], for a caller that starts what it returns at once (`lnk sandbox
/// wrap`, asked to hand): its proxy's routes come back with it, for the
/// caller to hand on a pipe ([`handed`]); none is written.
pub fn wrap_handed(confinement: &Confinement, argv: &[String]) -> anyhow::Result<Handing> {
    let wrapped = wrap_as(confinement, argv, false, Secrets::Handed, Files::Resolve)?;
    Ok(Handing {
        wrapped,
        handed: Handed(handed(confinement)?),
    })
}

pub(crate) fn wrap_resolved(confinement: &Confinement, argv: &[String]) -> anyhow::Result<Wrapped> {
    wrap_as(confinement, argv, false, Secrets::Files, Files::Resolved)
}

enum Files {
    Resolve,
    Resolved,
}

/// How a proxy is given its routes, which may hold credentials.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Secrets {
    /// In a file of Link's settings (600), read as it starts: for a proxy
    /// something else starts later (an agent's service).
    Files,
    /// Handed on a pipe by whoever starts it ([`handed`]), at once (`lnk
    /// sandbox run`): never on the disk.
    Handed,
}

/// An error unless every path `confinement` names is plain
/// ([`link_sandbox_client::plain_path`]).
fn check_paths(confinement: &Confinement) -> anyhow::Result<()> {
    let dirs = match &confinement.net {
        Net::Loopback { dir, .. } => vec![dir],
        Net::Proxied { proxy, .. } => vec![&proxy.dir],
        Net::Reaching { reached, .. } => reached.iter().map(|r| &r.dir).collect(),
        Net::Closed | Net::Host { .. } => Vec::new(),
    };
    for path in confinement
        .write
        .iter()
        .chain(&confinement.read)
        .chain(&confinement.write_dot_files)
        .chain(&confinement.hide)
        .chain(dirs)
    {
        ensure!(
            link_sandbox_client::plain_path(path),
            "{:?} has a control character",
            path.display().to_string()
        );
    }
    Ok(())
}

fn wrap_as(
    confinement: &Confinement,
    argv: &[String],
    learning: bool,
    secrets: Secrets,
    files: Files,
) -> anyhow::Result<Wrapped> {
    check_name(&confinement.name)?;
    check_paths(confinement)?;
    // A build folder (a toolchain's, or a folder being installed into),
    // made before it's granted: each under a folder it writes, never
    // elsewhere.
    for build in &confinement.builds {
        ensure!(
            confinement
                .write
                .iter()
                .any(|write| build.starts_with(write)),
            "{} isn't under a folder it writes",
            build.display()
        );
        // A link there would grant where it leads.
        ensure!(
            !std::fs::symlink_metadata(build).is_ok_and(|m| m.file_type().is_symlink()),
            "{} is a link: a build folder is a folder",
            build.display()
        );
        std::fs::create_dir_all(build).within(format!("making {}", build.display()))?;
    }
    crate::requirements::check(
        "lnk sandbox wrap",
        confinement.commands.requirement_policy(),
    )?;

    protected_closed(
        confinement,
        &link_files::try_installation_dir()?,
        &link_files::try_link_dir()?,
    )?;
    ensure!(!argv.is_empty(), "nothing to run");
    for path in confinement.write.iter().chain(&confinement.read) {
        ensure!(
            path.is_absolute(),
            "{} isn't an absolute path",
            path.display()
        );
    }
    // Resolve the masks only after the protected roots exist. Preparing
    // them here also covers direct module callers that never went through lnk.
    link_settings::execution::prepare()?;

    let rules = match files {
        Files::Resolve => link_sandbox_confine::Rules::of(confinement)?,
        Files::Resolved => link_sandbox_confine::Rules::from_resolved(confinement),
    };
    if cfg!(target_os = "linux") {
        link_sandbox_confine::linux::check_commands(
            &confinement.commands,
            rules.linux.confine.is_some(),
        )?;
    }
    let profile = rules_file(&confinement.name)?;
    let profile = match cfg!(target_os = "macos") {
        true => profile.with_extension(format!("{}.sb", link_files::random_token()?)),
        false => profile,
    };
    let (proxied, ports) = match &confinement.net {
        // macOS: its rules alone reach its ports.
        Net::Loopback { dir, .. } if cfg!(target_os = "linux") => {
            let exe = std::env::current_exe()?.canonicalize()?;
            return looped(rules, dir, &profile, &exe, argv);
        }
        Net::Reaching { .. } => {
            ensure!(
                cfg!(target_os = "linux"),
                "a sandbox reaching other proxies runs on Linux only"
            );
            let exe = std::env::current_exe()?.canonicalize()?;
            return reaching(confinement, rules, &profile, &exe, argv);
        }
        Net::Closed | Net::Loopback { .. } | Net::Host { .. } => {
            let mut rules = rules;
            rules.macos.report_execs = learning;
            return Ok(Wrapped {
                argv: link_sandbox_confine::wrap(&rules, &profile, argv)?,
                env: Default::default(),
            });
        }
        Net::Proxied { proxy, ports } => (proxy, ports),
    };
    let models = route_ports(proxied);
    let reached: BTreeSet<u16> = ports.union(&models).copied().collect();
    check_proxy(proxied, &reached)?;
    let exe = std::env::current_exe()?.canonicalize()?;
    // This program runs inside a proxied sandbox, as Link's own: ssh's
    // ProxyCommand (`connect`) and a page's opener (`BROWSER`).
    let mut rules = rules;
    if let Some(programs) = &mut rules.common.commands {
        programs.push(link_sandbox_confine::resolve(&exe));
    }
    let way = Proxied {
        name: &confinement.name,
        proxy: proxied,
        ports,
    };
    if cfg!(target_os = "linux") {
        namespaced(&way, rules, &profile, &exe, argv, secrets)
    } else if cfg!(target_os = "macos") {
        let port = proxied.listen.unwrap_or_default();
        ensure!(
            !reached.contains(&port) && !proxied.serve.contains(&port),
            "port {port} is the proxy's own"
        );
        // This program, read only, but not its folder: `sandbox connect`
        // runs in the sandbox (ssh's ProxyCommand).
        let mut rules = seatbelted(rules, ports, &models, port);
        rules.common.read.push(link_sandbox_confine::resolve(&exe));
        // Certificate checks, end to end through a proxy that reaches the
        // internet.
        rules.macos.certificates = !proxied.no_internet;
        rules.macos.report_execs = learning;
        let wrapped = link_sandbox_confine::wrap(&rules, &profile, argv)?;
        on_loopback(&way, port, &exe, wrapped, secrets, true)
    } else {
        anyhow::bail!("Link has no sandbox for this OS")
    }
}

/// `argv` with no wall ([`Unwalled`]), as it is, under its proxy on
/// `listen` of this machine's loopback (none: a free port), and what its
/// environment gets: the proxy's address where it's pointed there, and
/// Link's ways out on it. Nothing holds the program to it. Its proxy's
/// secrets in files of Link's settings, as [`wrap`]'s, or with `hand`,
/// handed back as [`wrap_handed`]'s.
pub fn unwalled(unwalled: &Unwalled, argv: &[String], hand: bool) -> anyhow::Result<Handing> {
    ensure!(
        link_sandbox_contract::box_workers::controller()?.is_none(),
        "ordinary-box work requires a sandbox and an unprivileged worker"
    );
    check_name(&unwalled.name)?;
    ensure!(!argv.is_empty(), "nothing to run");
    link_settings::execution::prepare()?;
    let proxy = &unwalled.proxy;
    let models = route_ports(proxy);
    check_proxy(proxy, &models)?;
    let port = proxy.listen.unwrap_or_default();
    ensure!(!models.contains(&port), "port {port} is the proxy's own");
    let exe = std::env::current_exe()?.canonicalize()?;
    let way = Proxied {
        name: &unwalled.name,
        proxy,
        ports: &BTreeSet::new(),
    };
    let secrets = match hand {
        true => Secrets::Handed,
        false => Secrets::Files,
    };
    let wrapped = on_loopback(&way, port, &exe, argv.to_vec(), secrets, unwalled.pointed)?;
    let handed = match hand {
        true => routed::handed_of(&unwalled.name, proxy)?,
        false => Vec::new(),
    };
    Ok(Handing {
        wrapped,
        handed: Handed(handed),
    })
}

/// This machine's ports `proxy`'s routes serve (its model runtimes').
fn route_ports(proxy: &Proxy) -> BTreeSet<u16> {
    proxy
        .routes
        .iter()
        .flat_map(|r| r.ports.iter().copied())
        .collect()
}

/// An error unless `proxy`'s rules are each one it serves: its endpoints
/// free of `reached` (this machine's ports its program reaches), its
/// routes ones it can mount, the tools they name defined here, its
/// upstream, hosts and machines of your network.
fn check_proxy(proxy: &Proxy, reached: &BTreeSet<u16>) -> anyhow::Result<()> {
    for endpoint in &proxy.tcp {
        link_sandbox_contract::net::check_endpoint(endpoint)?;
    }
    link_sandbox_client::confinement::wildcard_ports_free(&proxy.tcp, reached)?;
    link_sandbox_route::check_routes(&proxy.routes)?;
    // Each tool named is one defined here: its rules, outside.
    if let Some(route) = proxy.route("mcp") {
        let tools: link_sandbox_client::routes::Tools = link_sandbox_route::given(&route.given)?;
        for tool in &tools.tools {
            bridges::tools::load(&link_sandbox_proxy::wire::dir()?, tool)?;
        }
    }
    Upstream::parse(&proxy.upstream)?;
    for host in &proxy.hosts {
        link_sandbox_contract::net::check_host(host)?;
    }
    for entry in &proxy.lan {
        link_sandbox_contract::net::check_lan(entry)?;
    }
    // Whoever made the confinement: a cloud's private ranges are its
    // network.
    spec::lan_here(&proxy.lan, spec::on_cloud())
}

/// A sandbox whose only way out is its proxy ([`Net::Proxied`]), or a
/// program with no wall beside its proxy ([`Unwalled`]), its parts at
/// hand.
struct Proxied<'a> {
    /// Its name, its wire's and its proxy's files'.
    name: &'a str,
    proxy: &'a Proxy,
    /// This machine's ports it reaches, but its models'.
    ports: &'a BTreeSet<u16>,
}

/// Safes stay closed by every path. Installed programs may be read but
/// never written, nor may their parents be written to replace either root.
/// Inside a memory service's folder only its writable runtime and its
/// read-only handoff are granted; its records and log stay closed
/// (`link_files::layout::in_memory_runtime`).
fn protected_closed(confinement: &Confinement, hidden: &Path, home: &Path) -> anyhow::Result<()> {
    let boundaries = &link_files::layout::LAYOUT.boundaries;
    let mut safes = vec![
        hidden.join(&boundaries.hidden_safe),
        home.join(&boundaries.home_safe),
    ];
    safes.extend(link_files::layout::system_safes());
    let programs = [
        hidden.join(&boundaries.exec),
        link_files::layout::SystemRoot::Exec.path(),
    ];
    let places = |path: &Path| [path.to_owned(), link_sandbox_confine::resolve(path)];
    let memory = link_files::layout::Hidden::SharedMemory.at(hidden);
    for path in confinement.write.iter().chain(&confinement.read) {
        let write = confinement.write.contains(path);
        for at in places(path) {
            ensure!(
                !at.starts_with(&memory)
                    || link_files::layout::in_memory_runtime(&at, hidden, write),
                "{} is a memory service's record, which no sandbox reaches",
                path.display()
            );
        }
    }
    for safe in &safes {
        for protected in places(safe) {
            for path in confinement.write.iter().chain(&confinement.read) {
                ensure!(
                    places(path).iter().all(|at| !at.starts_with(&protected)),
                    "{} is in Link's settings, which no sandbox reaches",
                    path.display()
                );
            }
        }
    }
    for root in safes.iter().chain(&programs) {
        for protected in places(root) {
            for path in &confinement.write {
                ensure!(
                    places(path)
                        .iter()
                        .all(|at| !at.starts_with(&protected) && !protected.starts_with(at)),
                    "{} reaches or holds Link's settings or installed programs ({}), which no sandbox writes",
                    path.display(),
                    root.display()
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

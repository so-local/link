//! A proxied sandbox in a network namespace of its own (Linux), or its
//! proxy held alone, and the sandbox's own `/etc` files.

use std::path::{Path, PathBuf};

use anyhow::ensure;
use link_format::WithinExt;
use link_sandbox_client::Wrapped;

use link_sandbox_contract::spec::SandboxProxyInsideArgs;

use super::args::{desktop, line, list, proxied_env, proxy_args};
use super::{Proxied, Secrets, route_ports};

/// Linux: the sandbox in a network namespace of its own, crossed by
/// `sandbox proxy-inside` (in it) and `sandbox proxy` (outside). `rules`,
/// its confinement's.
pub(super) fn namespaced(
    way: &Proxied,
    rules: link_sandbox_confine::Rules,
    profile: &Path,
    exe: &Path,
    argv: &[String],
    secrets: Secrets,
) -> anyhow::Result<Wrapped> {
    let proxied = way.proxy;
    let dir = &proxied.dir;
    ensure!(
        dir.is_absolute(),
        "{} isn't an absolute path",
        dir.display()
    );
    link_files::create_private_dir(dir).within(format!("creating {}", dir.display()))?;
    // Where its program reaches the proxy, in its network: the port it
    // was set up with, else the proxy's own.
    let port = proxied
        .listen
        .unwrap_or(link_sandbox_proxy::client::PROXY_PORT);
    // This machine's ports it may reach (its models' among them), but
    // for its own, which it serves.
    ensure!(
        !rules.linux.ports.contains(&port) && !proxied.serve.contains(&port),
        "port {port} is the proxy's own"
    );
    if proxied.hold {
        return held(way, exe, secrets);
    }
    let reach = list(&mut rules.linux.ports.difference(&proxied.serve));
    let inside = SandboxProxyInsideArgs {
        dir: dir.clone(),
        port,
        reach: Some(reach.clone()),
        tcp: proxied.tcp.clone(),
        argv: argv.to_vec(),
        also: Vec::new(),
        shared: false,
    };
    let inner = line(exe, inside.argv());
    // The endpoints it reaches by name: in its own /etc/hosts, in a
    // private folder beside the sockets' folder (which it can write)
    // rather than in it.
    let named = link_sandbox_proxy::relay::named_endpoints(&proxied.tcp);
    let etc_hosts = (!named.is_empty())
        .then(|| -> anyhow::Result<PathBuf> {
            let path = etc_hosts_path(dir);
            let system = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
            write_etc(
                &path,
                &link_sandbox_proxy::relay::etc_hosts(
                    &system,
                    &named,
                    &proxied.hosts,
                    !proxied.no_internet,
                ),
            )?;
            Ok(path.canonicalize()?)
        })
        .transpose()?;
    // Its resolver, where it names endpoints: its proxy, listening in its
    // network (`dns`), named in its own /etc/resolv.conf beside /etc/hosts.
    let dns = !proxied.tcp.is_empty();
    let resolv_conf = dns
        .then(|| -> anyhow::Result<PathBuf> {
            let path = resolv_conf_path(dir);
            write_etc(
                &path,
                "nameserver 127.0.0.1\noptions timeout:2 attempts:2\n",
            )?;
            Ok(path.canonicalize()?)
        })
        .transpose()?;
    // No network of its own ([`Net::Proxied`] has none): the sockets'
    // folder, and this program (not its folder), which relays inside.
    let mut rules = rules;
    rules.linux.etc_hosts = etc_hosts;
    rules.linux.resolv_conf = resolv_conf;
    rules.linux.own = vec![link_sandbox_confine::resolve(dir)];
    rules.common.read.push(link_sandbox_confine::resolve(exe));
    let wrapped = link_sandbox_confine::wrap(&rules, profile, &inner)?;
    let (mut outer, secret) = proxy_args(way, secrets)?;
    outer.dir = Some(dir.clone());
    outer.tcp = proxied.tcp.clone();
    outer.desktop = desktop();
    outer.serve = Some(list(&mut proxied.serve.iter()));
    outer.reach = Some(reach);
    outer.dns = dns;
    outer.argv = wrapped;
    Ok(Wrapped {
        argv: line(exe, outer.argv()),
        env: proxied_env(port, exe, way, &secret, None, true),
    })
}

/// Linux: a proxy alone ([`link_sandbox_client::Proxy::hold`]), for a
/// sandbox started apart that reaches it at its `listen` port: the
/// proxy's command, and what the program's environment gets as it
/// reaches it there.
fn held(way: &Proxied, exe: &Path, secrets: Secrets) -> anyhow::Result<Wrapped> {
    let proxied = way.proxy;
    // Its local models are reached on it, at their route, not at their
    // ports: none is relayed into a network that isn't the program's own.
    let models = route_ports(proxied);
    let reach = way
        .ports
        .iter()
        .filter(|p| !proxied.serve.contains(p) && !models.contains(p))
        .count();
    ensure!(
        proxied.tcp.is_empty() && reach == 0,
        "a proxy held alone reaches no endpoints or ports: its sandbox's network is another's"
    );
    let port = proxied
        .listen
        .within("a proxy held alone listens at a port its sandbox names")?;
    let (mut outer, secret) = proxy_args(way, secrets)?;
    outer.dir = Some(proxied.dir.clone());
    outer.desktop = desktop();
    outer.serve = Some(list(&mut proxied.serve.iter()));
    outer.hold = true;
    Ok(Wrapped {
        argv: line(exe, outer.argv()),
        env: proxied_env(port, exe, way, &secret, None, true),
    })
}

/// The folder of a sandbox's own `/etc` files, whose sockets are in
/// `dir`: beside it (the sandbox writes `dir`), and as private as it, so
/// no other user plants or rewrites what's in it.
pub fn etc_dir(dir: &Path) -> PathBuf {
    dir.with_extension("etc")
}

/// Where a sandbox whose sockets are in `dir` has its own `/etc/hosts`.
pub fn etc_hosts_path(dir: &Path) -> PathBuf {
    etc_dir(dir).join("hosts")
}

/// The sandbox's own `/etc/resolv.conf` (Linux), as [`etc_hosts_path`].
pub fn resolv_conf_path(dir: &Path) -> PathBuf {
    etc_dir(dir).join("resolv.conf")
}

/// Writes `text` to `path`, one of a sandbox's own `/etc` files, in its
/// private folder ([`etc_dir`]).
fn write_etc(path: &Path, text: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let (folder, name) = match (path.parent(), path.file_name()) {
        (Some(folder), Some(name)) => (folder, name),
        _ => anyhow::bail!("{} is not a file in a folder", path.display()),
    };
    link_files::create_private_dir(folder).within(format!("creating {}", folder.display()))?;
    // Write through the folder's handle, the name opened without following
    // a link: a link swapped in where the file goes is refused, never
    // followed to a path the sandbox chose. The folder is this user's alone
    // (`create_private_dir`), so its own real path is kept.
    let dir =
        link_files::nofollow::Dir::open(folder).within(format!("opening {}", folder.display()))?;
    let mut file = dir
        .open_file(name, libc::O_WRONLY | libc::O_CREAT, 0o600)
        .within(format!("writing {}", path.display()))?;
    file.set_len(0)?;
    file.write_all(text.as_bytes())
        .within(format!("writing {}", path.display()))
}

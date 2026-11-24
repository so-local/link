//! The proxy's command line beside its routes, and what a proxied
//! program's environment gets.

use std::path::{Path, PathBuf};

use anyhow::ensure;
use link_sandbox_contract::spec::SandboxProxyArgs;

use super::routed::routes_args;
use super::{Proxied, Secrets};
use crate::rules_file;

/// What a proxied program's environment gets: where it's `pointed` at
/// its proxy on `port`, the proxy and git over SSH through it (`exe`'s
/// `sandbox connect`); and what each of its routes names, behind the
/// proxy's URL `secret` where it needs it (a tool's address,
/// `LNK_TOOL_GH`; `exe` as its `BROWSER`).
pub(super) fn proxied_env(
    port: u16,
    exe: &Path,
    way: &Proxied,
    secret: &str,
    proxy_secret: Option<&str>,
    pointed: bool,
) -> std::collections::BTreeMap<String, String> {
    let mut env = match pointed {
        true => link_sandbox_proxy::client::proxy_env(port, proxy_secret),
        false => Default::default(),
    };
    // To its proxy, which resolves names (a test's).
    if let Some(names) = link_sandbox_route::resolve::test_resolve() {
        env.insert(link_sandbox_route::resolve::TEST_RESOLVE.into(), names);
    }
    if let Some(ca) = link_sandbox_route::tls::test_ca() {
        env.insert(link_sandbox_route::tls::TEST_CA.into(), ca);
    }
    if let Some(command) = link_sandbox_proxy::client::git_ssh_command(exe).filter(|_| pointed) {
        env.insert("GIT_SSH_COMMAND".into(), command);
    }
    // Each route's own: where it is on the proxy (`LNK_TOOL_GH`), and
    // this program as `BROWSER`, which asks the open route.
    let root = format!("http://127.0.0.1:{port}");
    let exe = browser(exe);
    for route in &way.proxy.routes {
        env.extend(route.env_for(&root, secret, exe.as_deref()));
    }
    env
}

/// `exe` running the command `args` wrote: a line of its own program.
pub(super) fn line(exe: &Path, args: Vec<String>) -> Vec<String> {
    std::iter::once(exe.display().to_string())
        .chain(args)
        .collect()
}

/// What every line of `way`'s proxy gives it: where it goes out and
/// to which hosts, its wire, what of your network it reaches, its
/// routes (`secrets` says how), and its URL secret's file, that secret
/// beside them. Its caller adds where it listens and what it runs.
pub(super) fn proxy_args(
    way: &Proxied,
    secrets: Secrets,
) -> anyhow::Result<(SandboxProxyArgs, String)> {
    let proxied = way.proxy;
    let (routes, routes_handed) = routes_args(way, secrets)?;
    let (secret, url_secret) = url_secret(way)?;
    let args = SandboxProxyArgs {
        upstream: Some(proxied.upstream.clone()),
        host: proxied.hosts.clone(),
        name: Some(way.name.to_string()),
        wire: Some(link_sandbox_proxy::wire::dir()?),
        per_minute: proxied.per_minute,
        lan: proxied.lan.clone(),
        no_internet: proxied.no_internet,
        routes,
        routes_handed,
        url_secret: Some(url_secret),
        ..Default::default()
    };
    Ok((args, secret))
}

/// The secret of the URLs of the proxy's routes that need one
/// (`Proxy.secret`, or a new one), and the file the proxy reads it from
/// (`--url-secret`): in Link's settings, which the sandbox never sees,
/// never on a command line, where other users of the machine read it.
fn url_secret(way: &Proxied) -> anyhow::Result<(String, PathBuf)> {
    let secret = match &way.proxy.secret {
        // Carried in the URLs it hands the program, by design.
        Some(secret) => secret.expose_owned(),
        None => link_sandbox_route::url_secret()?,
    };
    ensure!(
        link_sandbox_route::is_url_secret(&secret),
        "a proxy's secret is 16 to 64 letters and digits"
    );
    rules_file(way.name)?;
    let [_, file] = link_sandbox_client::secret_files(way.name);
    link_files::write_private(&file, secret.as_bytes())?;
    Ok((secret, file))
}

/// The run's proxy secret, new at each wrap, for a proxy on this
/// machine's loopback, which takes no `CONNECT` or plain `http://`
/// request without it, and the variable that points the program at the
/// proxy, which carries it (`--pointed-by`). The secret is in that
/// address and nowhere else: the proxy reads it there, so the command
/// runs again as it is, as a service, and two wraps of one sandbox never
/// take each other's.
pub(super) fn proxy_secret() -> anyhow::Result<(String, String)> {
    let secret = link_sandbox_route::url_secret()?;
    Ok((secret, crate::constants::POINTED_BY.into()))
}

/// `BROWSER` in a sandbox allowed to open pages: this program, which,
/// given only a URL, asks its proxy to open it
/// ([`bridges::open::request`]). One
/// path, no arguments, as every reader of `BROWSER` takes it; None for a
/// path a shell would read differently.
fn browser(exe: &Path) -> Option<String> {
    let exe = exe.to_str()?;
    exe.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/._-+".contains(&b))
        .then(|| exe.to_string())
}

/// Each of the desktop session's variables here, `NAME=value`
/// (Linux): where the proxy asks you, and tells you, on the screen.
pub(super) fn desktop() -> Vec<String> {
    link_sandbox_route::desktop::DESKTOP_VARS
        .iter()
        .filter_map(|k| Some(format!("{k}={}", std::env::var(k).ok()?)))
        .collect()
}

/// `ports`, comma-separated, for `sandbox proxy`.
pub(super) fn list(ports: &mut dyn Iterator<Item = &u16>) -> String {
    ports.map(u16::to_string).collect::<Vec<_>>().join(",")
}

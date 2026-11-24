//! The sandbox module: `lnk sandbox run`, `check` and `permission
//! list`, and `wrap` for the modules that confine a program.
//!
//! It owns the one sandbox Link runs things in: every harness (through
//! `lnk-agent`, which asks `lnk sandbox wrap` for each step it runs),
//! and any other program (`lnk sandbox run`). What each may touch is its
//! [`link_sandbox_client::Confinement`]; the wall that enforces it
//! (`link-sandbox-confine`), the filtering proxy some get instead of the network
//! (`link-sandbox-proxy`) and the probes that test them ([`check`]) are the same
//! for all. Its own routes behind the proxy (tools, pages to open, asking
//! you) are its bridge's ([`bridges`]), apart from the wall.

pub mod bridges;
pub mod check;
pub mod cli;
pub mod constants;
mod given;
pub mod opening;
pub mod settings;
pub mod spec;
mod wrap;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use link_format::WithinExt;
use link_sandbox_client::{Confinement, Net, Proxy};

pub use given::given;
pub(crate) use wrap::wrap_resolved;
pub use wrap::{
    Launch, ROUTES_HANDED, Routed, etc_dir, etc_hosts_path, handed, resolv_conf_path, routes_file,
    seatbelted, unwalled, wrap, wrap_handed, wrap_handing, wrap_learning,
};

/// Its proxy, when its network is one ([`Net::Proxied`]).
pub fn proxy_of(confinement: &Confinement) -> Option<&Proxy> {
    match &confinement.net {
        Net::Proxied { proxy, .. } => Some(proxy),
        _ => None,
    }
}

/// A folder for a run's proxy, in the system's temporary folder (its
/// sockets' paths stay short) under a name of its own that no other user
/// can guess and make first. Made when the run starts, only this user's.
pub fn run_proxy_dir() -> anyhow::Result<PathBuf> {
    Ok(std::env::temp_dir().join(format!("lnk-sandbox-{}", link_files::random_token()?)))
}

pub use link_sandbox_client::check_name;

/// Comma-separated ports, as `sandbox proxy` takes them.
pub fn ports(text: &str) -> anyhow::Result<std::collections::BTreeSet<u16>> {
    text.split(',')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().within("ports are numbers, comma-separated"))
        .collect()
}

/// [`link_sandbox_client::rules_file`], its folder created (only the
/// user's).
pub fn rules_file(name: &str) -> anyhow::Result<PathBuf> {
    link_files::try_installation_dir()?;
    let path = link_sandbox_client::rules_file(name);
    if let Some(dir) = path.parent() {
        link_files::create_private_dir(dir)?;
    }
    Ok(path)
}

/// The files `wrap` wrote for `confinement` holding what its proxy adds
/// (its routes, the credentials they add) or takes (the secret its URLs
/// carry), removed once it has ended: they're needed only while it runs.
pub fn forget_secrets(policy: &Confinement) {
    link_sandbox_client::forget_secrets(&policy.name);
}

pub mod requirements;

#[cfg(test)]
link_test_kit::program!(system);

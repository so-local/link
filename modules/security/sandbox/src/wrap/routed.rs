//! A proxy's routes and who calls them: handed on a pipe, or in a file
//! of Link's settings, never on a command line.

use std::path::PathBuf;

use link_sandbox_client::{Confinement, Proxy};

use super::{Proxied, Secrets};
use crate::{proxy_of, rules_file};

/// What a proxy is given beside its command line: its routes, who calls
/// them, and the keyring to unbind that caller from when it ends, when
/// `wrap` bound it for the run. In a file of Link's settings, or handed
/// on a pipe ([`ROUTES_HANDED`]), never on a command line.
use link_secretive::Duplicate;

#[derive(Duplicate, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Routed {
    #[serde(default)]
    #[duplicate]
    pub routes: Vec<link_sandbox_route::Route>,
    #[serde(default)]
    #[duplicate]
    pub caller: Option<link_sandbox_binding::Binding>,
    /// The keyring `wrap` bound `caller` in for the run, unbound when the
    /// proxy ends.
    #[serde(default)]
    pub unbind_from: Option<PathBuf>,
}

link_files::specification!(Routed, include_str!("routed.schema.toml"));

impl Routed {
    /// An error unless its routes hold no secret's value, only names
    /// ([`link_sandbox_route::Route::names_only`]): what a file of them
    /// holds, written or read.
    pub fn names_only(&self) -> anyhow::Result<()> {
        self.routes
            .iter()
            .try_for_each(link_sandbox_route::Route::names_only)
    }
}

/// The variable a proxy's routes are handed by.
pub const ROUTES_HANDED: &str = "LNK_SANDBOX_ROUTES";

/// What [`wrap_handing`]'s caller hands the program it starts
/// ([`link_secretive::process::hand_to`]): its proxy's routes and
/// their caller, by [`ROUTES_HANDED`].
pub fn handed(confinement: &Confinement) -> anyhow::Result<Vec<(String, link_secretive::Secret)>> {
    let Some(proxy) = proxy_of(confinement) else {
        return Ok(Vec::new());
    };
    handed_of(&confinement.name, proxy)
}

/// [`handed`], for sandbox `name`'s proxy `proxy`.
pub(super) fn handed_of(
    name: &str,
    proxy: &Proxy,
) -> anyhow::Result<Vec<(String, link_secretive::Secret)>> {
    let routed = routed(name, proxy)?;
    Ok(vec![(
        ROUTES_HANDED.to_string(),
        link_secretive::Secret::new(serde_json::to_string(&routed)?),
    )])
}

/// `proxy`'s routes and who calls them, for sandbox `name`'s proxy: its
/// own caller, or one bound for the run in this machine's keyring
/// (`run-<name>`, of this user), when it has routes and none.
fn routed(name: &str, proxy: &Proxy) -> anyhow::Result<Routed> {
    let mut routes = proxy
        .routes
        .iter()
        .map(link_secretive::Duplicate::duplicate)
        .collect::<Vec<_>>();
    if let Some(caller) = &proxy.caller {
        link_secret_contract::keeper::protect_routes(&mut routes, caller)?;
        return Ok(Routed {
            routes,
            caller: Some(caller.duplicate()),
            unbind_from: None,
        });
    }
    if proxy.routes.is_empty() {
        return Ok(Routed::default());
    }
    let identity = link_sandbox_binding::Identity {
        agent: format!("{}{name}", link_sandbox_client::grants::RUN_PREFIX)
            .chars()
            .take(64)
            .collect(),
        service: String::new(),
        owner: link_machine::owner::local_owner()?,
        teams: Vec::new(),
    };
    let keys = link_sandbox_binding::Keyring::here();
    let key = link_secretive::Secret::new(link_files::random_token()?);
    let caller = keys.bind(&identity, &key)?;
    link_secret_contract::keeper::protect_routes(&mut routes, &caller)?;
    Ok(Routed {
        routes,
        caller: Some(caller),
        unbind_from: Some(keys.dir().to_path_buf()),
    })
}

/// Its routes and who calls them for the proxy, never on a command line:
/// handed on a pipe (`--routes-handed`, [`ROUTES_HANDED`]), or in a file of
/// Link's settings, which the sandbox never sees (`--routes <file>`). A
/// file left from routes it no longer has is removed.
pub(super) fn routes_args(
    way: &Proxied,
    by: Secrets,
) -> anyhow::Result<(Option<PathBuf>, Option<String>)> {
    let file = routes_file(way.name)?;
    if way.proxy.routes.is_empty() {
        let _ = std::fs::remove_file(&file);
        return Ok((None, None));
    }
    if by == Secrets::Handed {
        // A file left by a run of before goes: handed, none is read.
        let _ = std::fs::remove_file(&file);
        return Ok((None, Some(ROUTES_HANDED.into())));
    }
    // Never a value in the file: names, which each route's bridge reads
    // from the store at each call.
    Routed {
        routes: way.proxy.routes.iter().map(Duplicate::duplicate).collect(),
        ..Default::default()
    }
    .names_only()?;
    let routed = routed(way.name, way.proxy)?;
    link_files::write_private(&file, &link_files::saved::print_json(&routed)?)?;
    Ok((Some(file), None))
}

/// Where the proxy of sandbox `name` reads its routes.
pub fn routes_file(name: &str) -> anyhow::Result<PathBuf> {
    rules_file(name)?;
    let [routes, _] = link_sandbox_client::secret_files(name);
    Ok(routes)
}

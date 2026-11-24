//! The route to this machine's model runtimes (`/_link/here/<port>`, and
//! their ports on Linux), for inference only, which a sandbox's spec opens
//! (`[localhost] models`): its prefix, what its bridge is given, and the
//! bridge that serves it, `lnk model bridge`. The models' own routes and
//! facts are the models module's contract.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{Bridge, Route, SECRET_PREFIX};

/// Where this machine's model runtimes are on its proxy, each by its port.
pub const LOCAL: &str = "/_link/here/";

/// The bridge that serves the model routes here: `lnk model bridge`.
pub fn bridge() -> anyhow::Result<Bridge> {
    Bridge::of("model", None)
}

/// What the local route's bridge is given: the runtimes' ports.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Local {
    pub ports: BTreeSet<u16>,
}

/// The local route of this machine's runtimes on `ports`, served by
/// `bridge`: each reached on its proxy behind its URL secret
/// (`LNK_LOCAL_MODEL_<port>`), and on Linux at its own port too.
pub fn local(bridge: Bridge, ports: BTreeSet<u16>) -> anyhow::Result<Route> {
    let given = serde_json::to_string(&Local {
        ports: ports.clone(),
    })?;
    let env = ports
        .iter()
        .map(|port| {
            (
                format!("LNK_LOCAL_MODEL_{port}"),
                format!("{}{port}", crate::url_of(LOCAL, true)),
            )
        })
        .collect();
    Ok(Route {
        name: "local-model".into(),
        prefix: LOCAL.into(),
        needs_url_secret: true,
        ports,
        bridge,
        given: given.into(),
        env,
        handed: false,
    })
}

/// Where a program behind a proxy reaches the model runtime on this
/// machine's `model` port (one of [`local`]'s), its proxy on `port`
/// whose URL secret is `secret`: the runtime's own address as a client
/// takes it (no `/v1`), for inference only.
pub fn local_url(port: u16, model: u16, secret: &str) -> String {
    let rest = LOCAL.strip_prefix("/_link/").unwrap_or(LOCAL);
    format!("http://127.0.0.1:{port}{SECRET_PREFIX}{secret}/{rest}{model}")
}

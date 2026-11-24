//! A sandbox's spec: a TOML file whose sections are its only grants
//! (`docs/config/specs/sandbox.md`). A section left out stays closed,
//! `[commands]` too: a spec with no sections reads none of your files,
//! reaches no network and runs no program.
//!
//! The type is here in its independent contract, since more than the
//! sandbox module keeps one (an agent's file, a harness's grants). What a
//! spec opens on a machine is the confinement resolver; its refusals
//! that need the machine itself are the sandbox module's: another module
//! has a spec checked by running `lnk sandbox`.

use std::path::Path;

use anyhow::ensure;

// The spec and its sections, generated from the module's `types.toml`,
// which documents each section and key once: the JSON Schema
// ([`Spec::schema`]), the generated Sandbox Spec page, the field flags'
// help and `lnk sandbox permission list` ([`Spec::sections`]) are made from
// it.
include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// A sandbox spec: the `sandbox` kind's type, by the name its readers use.
pub type Spec = Sandbox;

impl Spec {
    /// Whether its network is reached through Link's proxy: it opens the
    /// internet, and isn't on the network as it is (`[network] mode =
    /// "host"`).
    pub fn behind_proxy(&self) -> bool {
        self.internet.is_some() && !self.host_network()
    }

    /// Whether it has this machine's network as it is (`[network] mode =
    /// "host"`): no proxy of its own, nothing filtered.
    pub fn host_network(&self) -> bool {
        self.network.as_ref().and_then(|n| n.mode) == Some(NetworkMode::Host)
    }
}

/// An OS a table can be for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Os {
    MacOs,
    Linux,
}

impl Os {
    pub fn here() -> Self {
        match cfg!(target_os = "macos") {
            true => Os::MacOs,
            false => Os::Linux,
        }
    }
}

/// Each key of `over` given replaces `base`'s, but for `hide`, which
/// adds up.
macro_rules! layer {
    ($base:expr, $over:expr, [$($key:ident),*], [$($sum:ident),*]) => {{
        let mut base = $base;
        let over = $over;
        $(if over.$key.is_some() { base.$key = over.$key.clone(); })*
        $(if let Some(more) = &over.$sum {
            let mut all = base.$sum.clone().unwrap_or_default();
            for item in more {
                if !all.contains(item) {
                    all.push(item.clone());
                }
            }
            base.$sum = Some(all);
        })*
        base
    }};
}

/// A section as it is on `os`: its keys, an OS table's replacing them.
/// A table in an OS's table is refused.
macro_rules! on_os {
    ($section:expr, $os:expr, $name:literal, $ty:ident, [$($key:ident),*], [$($sum:ident),*]) => {{
        match $section {
            None => Ok(None),
            Some(section) => {
                let section: &$ty = section;
                for (os, table) in [("macos", &section.macos), ("linux", &section.linux)] {
                    if let Some(table) = table {
                        ensure!(
                            table.macos.is_none() && table.linux.is_none(),
                            "[{}.{os}] can't hold another OS's table",
                            $name
                        );
                    }
                }
                let base = $ty { macos: None, linux: None, ..section.clone() };
                let mine = match $os {
                    Os::MacOs => section.macos.as_deref(),
                    Os::Linux => section.linux.as_deref(),
                };
                let merged = match mine {
                    Some(over) => layer!(base, over, [$($key),*], [$($sum),*]),
                    None => base,
                };
                let empty = $ty::default();
                // Only another OS's table: closed here.
                Ok::<_, anyhow::Error>((merged != empty).then_some(merged))
            }
        }
    }};
}

mod grants;
mod resolve;
mod schema;
mod teams;
pub use grants::{Item, union};
pub use schema::Section;
pub use teams::{WHOLE, in_paths as team_paths};
#[cfg(test)]
mod tests;

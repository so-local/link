//! Where a harness hosts its agent's work: one definition, read by the
//! harness and named by `lnk-agent`, so the bindings its tool calls carry
//! are only ever ones its tools' sources can check.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::{STORE_VAR, Store};

/// The store a harness binds its agent's work in, and whether its tools'
/// sources check those bindings there.
#[derive(Clone, Debug)]
pub struct Host {
    pub store: Store,
    /// Whether `store` is the machine's, the one its tools' sources and
    /// its owner's controls use: then each tool call carries its work's
    /// binding. No: the store is the harness's own, which nothing outside
    /// it sees, and its calls carry none, reaching their sources as calls
    /// of no tracked work, under each source's own policy.
    pub shared: bool,
}

impl Host {
    /// The machine's store, shared with every source.
    pub fn shared(store: Store) -> Self {
        Self {
            store,
            shared: true,
        }
    }

    /// A harness's: the machine's store when `lnk-agent` names it, an
    /// absolute [`STORE_VAR`], which it does where the harness reaches
    /// Link's settings (no sandbox reaches them); else the one in the
    /// harness's own `HOME`.
    pub fn of_harness() -> anyhow::Result<Self> {
        Self::named(std::env::var_os(STORE_VAR), Store::machine)
    }

    fn named(
        named: Option<OsString>,
        own: impl FnOnce() -> anyhow::Result<Store>,
    ) -> anyhow::Result<Self> {
        match named.map(PathBuf::from).filter(|dir| dir.is_absolute()) {
            Some(dir) => Ok(Self::shared(Store::at(dir))),
            None => Ok(Self {
                store: own()?,
                shared: false,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Named, the machine's store is shared with the harness's sources;
    /// unnamed, or named by a relative path, its own is its alone.
    #[test]
    fn a_harness_shares_only_the_store_it_is_handed() {
        let own = || Ok(Store::at("/home/h/.lnk/safe/approvals"));
        let named = Host::named(Some("/m/state/interactions/v1".into()), own).unwrap();
        assert!(named.shared);
        assert_eq!(
            named.store.dir(),
            std::path::Path::new("/m/state/interactions/v1")
        );
        for unnamed in [None, Some("relative".into())] {
            let host = Host::named(unnamed, own).unwrap();
            assert!(!host.shared);
            assert_eq!(
                host.store.dir(),
                std::path::Path::new("/home/h/.lnk/safe/approvals")
            );
        }
    }
}

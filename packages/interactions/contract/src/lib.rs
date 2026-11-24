//! Versioned data shared by interaction hosts, sources and delivery adapters.

use serde::{Deserialize, Serialize};

pub mod approval;
pub mod control;
/// The shapes this contract declares in its `types.toml`.
pub mod declared {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}
pub mod delivery;
pub mod identity;
pub mod question;
pub use identity::*;
pub use question::*;

/// The interaction wire, which each envelope and question states, read
/// before its payload ([`link_version::Protocol::read`]): an older or a
/// newer version is refused by name. Version 2 says an actor's kind by
/// its tag ([`TaggedActor`]).
pub static PROTOCOL: std::sync::LazyLock<link_version::Protocol> =
    std::sync::LazyLock::new(|| link_version::Protocol::shipped(include_str!("protocol.toml")));

/// A schedule definition's identity, independent of its display name.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct ScheduleId(pub String);

/// One scheduled instant of a definition. Retries retain this reference.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledOccurrence {
    pub schedule: ScheduleId,
    pub revision: u64,
    pub scheduled_at: u64,
}

#[cfg(test)]
link_test_kit::program!(unit);

//! Agent opinion schema and team resolution.

pub use link_agent_identity::access;
pub use link_agent_identity::owner as identity;
pub use link_connection_policy as connection;
pub use link_core::*;
pub use link_harness_base as harness;
pub use link_machine::machine;
pub use link_model_contract as models;
pub use link_store_contract::storage;
mod agent_spec;
pub use agent_spec::*;

/// The least of a tool's answer a harness keeps, in bytes: what a spec's
/// `answer` may be at least, and what's kept where it sets none.
pub const ANSWER_LEAST: u64 = 1 << 10;

pub use link_store_contract::store;

#[cfg(test)]
link_test_kit::program!(unit);

/// The agent module's shapes, generated from its declarations.
#[allow(
    clippy::large_enum_variant,
    reason = "generated whole: a tagged choice holds each variant as written"
)]
pub mod declared {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}

/// The checks the declarations name that their TOML can't express.
mod checks {
    /// An agent spec's values held to each other and to what they name
    /// ([`crate::check`]).
    pub(crate) fn agent(spec: &crate::declared::Agent) -> Result<(), String> {
        crate::check(spec).map_err(|err| format!("{err:#}"))
    }

    /// A team's file held to its rules ([`crate::agent_spec::check_team`]).
    pub(crate) fn team(file: &crate::declared::Team) -> Result<(), String> {
        crate::agent_spec::check_team(file).map_err(|err| format!("{err:#}"))
    }
}

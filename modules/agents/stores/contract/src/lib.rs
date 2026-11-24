//! Store authority, storage and files bridge contracts.

pub use link_core::*;
mod constants;
pub mod bridge {
    pub use link_sandbox_binding::*;
}
pub use link_agent_identity::{names as machine, owner as identity};
pub use link_connection_policy as connection;
pub use link_harness_base as harness;
pub use link_sandbox_bridge::socket;
#[cfg(feature = "files-client")]
pub mod files;
pub mod storage;

/// Says a fact about the run from code with no `Out`, `err`'s words
/// as its why.
pub(crate) fn warned(headline: link_format::Sentence, err: &anyhow::Error) {
    use link_format::{Prose, Report, Sentence, aside};
    aside(Report::warning(headline).why(Prose::from(Sentence::adopt(&format!("{err:#}")))));
}
pub mod store;
/// The store protocol's shapes, generated from its declarations
/// (`actions.toml` and `types.toml`), which every store's program names.
pub mod declared {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
// Store interfaces shared by the drivers and their clients.
pub mod authority;

#[cfg(test)]
link_test_kit::program!(system);

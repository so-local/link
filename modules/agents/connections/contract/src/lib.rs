//! Connection settings and message bridge protocol.

pub use link_agent_identity::{access, names as machine, owner as identity};
pub use link_connection_policy as policy;
pub use link_core::*;
pub use link_harness_base as harness;
pub use link_sandbox_binding as bridge;
pub use link_sandbox_bridge::socket;
pub use link_secret_contract as secrets;
/// The connection protocol's shapes and each command's arguments,
/// generated from its declarations (`actions.toml` and `types.toml`):
/// what every connection's program names, and the command line
/// `lnk agent` builds for each.
pub mod declared {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}
mod connection;
pub use connection::*;
pub use link_interactions_contract::TransportEventId;
pub mod constants;
#[cfg(test)]
pub use link_agent_spec as agent_spec;

#[cfg(test)]
link_test_kit::program!(system);

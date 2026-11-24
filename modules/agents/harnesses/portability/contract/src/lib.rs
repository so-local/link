//! Harness program interfaces.

pub use link_harness_wire::skill;

pub use link_harness_wire::message;

pub use link_agent_identity::{access, names as machine, owner as identity};
pub use link_agent_spec as agent_spec;
pub use link_agent_state_protocol as asp;
pub use link_connection_contract as connection;
pub use link_core::*;
pub use link_harness_base;
pub use link_model_contract as models;
pub use link_sandbox_binding as bridge;
pub use link_sandbox_client as sandbox;
pub use link_store_contract::{files, storage, store};
pub mod harness;
pub use harness::*;
/// Agent folders on the host.
pub fn agents_dir() -> std::path::PathBuf {
    link_files::layout::Home::Agents.path()
}

pub use link_harness_base::find_program;

#[cfg(test)]
link_test_kit::program!(unit);

/// The harness protocol's shapes, generated from its declarations
/// (`actions.toml` and `types.toml`), which every harness's program names.
pub mod declared {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}

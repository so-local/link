//! Harness folder vocabulary, endpoint proof and linked state.

pub use link_agent_identity::names as machine;
pub use link_core::*;
pub use link_sandbox_contract as sandbox;
mod base;
pub use base::*;

pub use base::find_program;

#[cfg(test)]
link_test_kit::program!(unit);

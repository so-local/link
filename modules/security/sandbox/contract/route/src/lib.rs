//! Sandbox route wire protocol and transport.

pub use link_core::*;
pub use link_sandbox_bridge::socket;
pub use link_sandbox_contract::net;
pub mod desktop;
pub mod local_model;
mod route;
pub use route::*;

#[cfg(test)]
link_test_kit::program!(system);

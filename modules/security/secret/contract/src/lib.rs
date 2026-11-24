//! Secret grants, keeper protocol and secret-route clients.

pub use link_core::*;
pub mod keychain;
pub use link_machine::owner as identity;
pub use link_sandbox_binding as bridge;
pub mod place;
pub use link_machine::machine;
pub use link_sandbox_bridge::socket;
pub use link_sandbox_route::net;
mod secrets;
pub use secrets::*;

use link_secretive::Secret;

use link_secretive as secret;

#[cfg(test)]
link_test_kit::program!(unit);

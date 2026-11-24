//! The sandbox's command contract: what a module that confines a program
//! asks of `lnk sandbox`. The routes and secrets it names are their own
//! crates'.

use link_core::*;
use link_sandbox_binding as bridge;
use link_sandbox_route as route;
use link_sandbox_route::{desktop, net};
use link_secret_contract as secrets;
pub mod declared;
mod handoff;
mod sandbox;
mod wires;
pub use handoff::RunHandoff;
pub use sandbox::*;
pub use sandbox::{ensure, spec};
pub use wires::*;

#[cfg(test)]
link_test_kit::program!(system);

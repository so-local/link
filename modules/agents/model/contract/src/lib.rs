//! Model facts and model-route client protocol.

pub use link_core::*;
mod models;
pub use models::*;

/// The model namespace's actions, as `lnk model` and an agent's commands
/// offer them: its module's `actions.toml`.
pub fn actions() -> link_component::actions::Actions {
    link_component::actions::parse(include_str!("../../actions.toml"))
        .expect("the model module's actions.toml parses")
}

/// A namespace route granted to a sandboxed program, as the shared
/// action registry describes it. The proxy authenticates the program.
pub fn program_route(path: &str) -> Option<link_component::actions::Action> {
    link_component::actions::route(&actions(), path, link_component::actions::Caller::Program)
        .map(|(_, action)| action.clone())
}

/// The model module's shapes, generated from its `types.toml`: what
/// `lnk model` answers, named by every program that reads it.
pub mod declared {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}

pub mod chat;

pub mod api;

pub mod context;

pub mod caching;

#[cfg(test)]
link_test_kit::program!(system);

//! The agent module's own contract: an agent's folder ([`folder`]), who
//! it is and whose ([`identity`]), its sandbox file ([`agent_sandbox`]),
//! the stream it moves in ([`moving`]) and the folders every
//! sandbox hides. What other modules provide, a dependent names from
//! their own crates.

pub mod agent_sandbox;
#[cfg(test)]
mod bridge;
pub mod folder;
pub mod identity;
#[cfg(feature = "moving")]
pub mod moving;
#[cfg(test)]
mod routes;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

/// The actions of an agent's own namespaces the agent module holds,
/// `chat` and `skill`, as an agent's commands offer them: its
/// `actions.toml`.
/// Read once per process: a start asks for them for each namespace it
/// defines, and reading them is most of what that costs.
pub fn actions() -> link_component::actions::Actions {
    static ACTIONS: std::sync::LazyLock<link_component::actions::Actions> =
        std::sync::LazyLock::new(|| {
            link_component::actions::parse(include_str!("../../actions.toml"))
                .expect("the agent module's actions.toml parses")
        });
    ACTIONS.clone()
}

/// The folders every sandbox hides are the sandbox's: an agent's is one
/// of them.
pub use link_sandbox_contract::{agents_dir, archive_dir, hidden_dirs, teams_dir};

pub mod namespace;

pub use link_component::actions as action_registry;

#[cfg(test)]
link_test_kit::program!(system);

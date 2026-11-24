//! The agent's routes are served where their callers ask: by the path
//! each declares, as an agent's tools, its harness's slash commands and
//! the action bridge name them.

use crate::action_registry::{Caller, Surface, command_name, route};

/// Each route at its own path, beside the command that forwards to it.
#[test]
fn every_route_is_served_at_its_path() {
    let actions = crate::actions();
    for (path, caller, named) in [
        ("/_link/question/create", Caller::Agent, "question:create"),
        ("/_link/tool/list", Caller::Agent, "tool:list"),
        ("/_link/browser/click", Caller::Agent, "browser:click"),
        ("/_link/memory/show", Caller::Program, "memory:show"),
        ("/_link/memory/search", Caller::Agent, "memory:search"),
        ("/_link/task/plan", Caller::Agent, "task:plan"),
        ("/_link/web/fetch", Caller::Agent, "web:fetch"),
    ] {
        let (at, _) = route(&actions, path, caller).unwrap_or_else(|| panic!("{path}"));
        assert_eq!(command_name(at), named, "{path}");
    }
    assert!(actions["agent browser click"].on(Surface::Cli));
    assert!(!actions["browser click"].on(Surface::Cli));
}

/// A chat's own commands keep their names, `/chat:ask` in a harness.
#[test]
fn a_chats_commands_keep_their_names() {
    let actions = crate::actions();
    for name in ["chat ask", "chat compact", "skill list", "skill write"] {
        assert!(actions[name].on(Surface::Command), "{name}");
    }
}

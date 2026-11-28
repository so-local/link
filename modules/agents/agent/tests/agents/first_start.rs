//! The first start on a machine with no agent: Onboarding, created
//! from the archive the agent module carries.

use super::*;

/// The first start on a machine with no agent, naming none, creates
/// `onboarding` from the archive the module carries, with Link MCP, file
/// and scheduler tools on. Once a second agent is here, a start naming
/// none is refused, saying how to name one or start them all.
#[test]
fn a_first_start_creates_onboarding_and_two_agents_need_a_name() {
    let world = World::new("first");
    // Created whether or not its harness then runs here: the fake honors
    // no scheduler, so the start itself may be refused.
    let started = world.start("onboarding", &["start", "--harness", "fake"]);
    let said = String::from_utf8_lossy(&started.stderr);
    assert!(said.contains("onboarding is your first agent"), "{said}");
    let settings = read(&world.home.join("Link/Safe/Agents/onboarding/agent.toml"));
    let table: toml::Table = settings.parse().unwrap();
    for tool in ["mcp", "file", "scheduler"] {
        assert_eq!(
            table["behavior"]["tool"][tool]["enabled"].as_bool(),
            Some(true),
            "{tool}: {settings}"
        );
    }
    assert!(!world.home.join("Link/Safe/Agents/main").exists());

    ok(&world.start("second", &["create", "second", "--harness", "fake"]));
    let refused = failed(&world.start("onboarding", &["start"]));
    assert!(
        refused.contains("--agent <name>") && refused.contains("--all"),
        "{refused}"
    );
}

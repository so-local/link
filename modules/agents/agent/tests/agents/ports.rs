//! Each agent's block of ports, `main`'s too: never its adapter's own
//! port, which another user's agent on the machine may hold.

use super::*;

/// An adapter whose own port, without `LNK_PORT`, is `HELD`: every user
/// of a machine runs the same adapter, so another user's agent holds it.
/// It serves its one port, as Link Harness does, and records the base
/// port it was given. Its own contract says it takes one port, so it's
/// written as it is, never with the scenario contract.
const DEFAULTED: &str = r#"#!/bin/sh
case "$1" in
  contract) printf '{"version":1,"ports":1}' ;;
  command) printf '{"argv":["/bin/sh","-c","echo ${LNK_PORT:-none} > \\"$LNK_FILES/port\\""],"ports":[%s]}' "${LNK_PORT:-HELD}" ;;
  *) exit 0 ;;
esac
"#;

/// Another user's agent on this machine listens on the adapter's own
/// port (held here by the test): a second user's agent, `main` too, gets
/// a block of its own, so it starts beside it, never refused for the
/// port the other holds, and keeps its block from one start to the next.
#[test]
fn an_agent_starts_beside_another_users_on_its_adapters_own_port() {
    let world = World::new("beside");
    let other = link_test_kit::Held::loopback();
    link_test_kit::write_program(
        &world.bin.join("lnk-defaulted"),
        DEFAULTED.replace("HELD", &other.port().to_string()),
    )
    .unwrap();
    ok(&world.start("main", &["create", "main", "--harness", "defaulted"]));
    let given: u16 = read(&world.agent("main").join("home/port"))
        .trim()
        .parse()
        .unwrap();
    assert_ne!(given, other.port());
    let here = crate::common::record(&world.home, "main", "here.toml");
    assert_eq!(here["port"], given, "{here:?}");
    assert_eq!(here.get("block"), Some(&2.into()), "{here:?}");
    ok(&world.start("main", &["start", "-a", "main"]));
    assert_eq!(
        crate::common::record(&world.home, "main", "here.toml"),
        here
    );
}

/// A program listening on a port of the agent's own block, while the
/// agent doesn't run (another user's agent that took it, say): the agent
/// moves to a free block, says so, and starts.
#[test]
fn an_agent_moves_off_a_block_another_program_holds() {
    let world = World::new("moves");
    let other = link_test_kit::Held::loopback();
    link_test_kit::write_program(
        &world.bin.join("lnk-defaulted"),
        DEFAULTED.replace("HELD", &other.port().to_string()),
    )
    .unwrap();
    crate::common::write_agent(
        &world.home,
        "main",
        &format!(
            "harness = \"defaulted\"\nport = {}\nblock = 2\n",
            other.port()
        ),
    );
    let said = ok(&world.start("main", &["start", "-a", "main"]));
    assert!(
        said.contains(&format!("Port {} is another program's", other.port())),
        "{said}"
    );
    let given: u16 = read(&world.agent("main").join("home/port"))
        .trim()
        .parse()
        .unwrap();
    assert!(
        !(other.port()..other.port() + 2).contains(&given),
        "{given}"
    );
    let here = crate::common::record(&world.home, "main", "here.toml");
    assert_eq!(here["port"], given, "{here:?}");
}

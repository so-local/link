//! One move holds an agent at a time: a start here waits for a move this
//! computer runs, a second computer's move is refused while the first
//! holds the agent where it is, and a move's calls name the agent by ID.

use super::*;

/// A move's ID, as another computer's move names itself.
const OTHER: &str = "fedcba9876543210fedcba9876543210";

/// A move's ID this computer's journal holds.
const MINE: &str = "0123456789abcdef0123456789abcdef";

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// While another `lnk agent move` of it here holds its lock, `lnk agent
/// start` refuses rather than take it out of its pause midway, and so
/// does a start naming a move this computer's journal doesn't hold; the
/// move's own start, whose move the journal holds, is let through.
#[test]
fn a_start_waits_for_a_move() {
    let world = World::new("start-waits");
    an_agent_with_files(&world);
    let moving = world.here.join(".lnk/safe/agents/transfers/moving");
    std::fs::create_dir_all(&moving).unwrap();
    let held = std::fs::File::create(moving.join(format!("{ID}.lock"))).unwrap();
    held.lock().unwrap();
    let refused = world.lnk_agent(&["start"]);
    assert!(!refused.status.success());
    assert!(
        said(&refused).contains("main is being moved by another lnk agent move"),
        "{}",
        said(&refused)
    );
    let unknown = world.lnk_agent(&["start", "--during-move", OTHER]);
    assert!(!unknown.status.success());
    assert!(said(&unknown).contains("being moved"), "{}", said(&unknown));
    write(
        &moving.join(format!("{ID}.toml")),
        &format!(
            "schema = 1\n\n[[move]]\nname = \"main\"\nid = \"{ID}\"\nmove_id = \"{MINE}\"\n\
             from = {{ kind = \"here\" }}\nto = {{ kind = \"box\", name = \"b1\" }}\n\
             step = \"switched\"\n"
        ),
    );
    let own = world.lnk_agent(&["start", "--during-move", MINE]);
    assert!(!said(&own).contains("being moved"), "{}", said(&own));
    held.unlock().unwrap();
    drop(held);
}

/// Another computer's move holds the agent on its box: this computer's
/// move is refused at its pause, leaves the agent on the box, and its
/// take-back never takes the agent out of the other move's pause; the
/// other move's own calls still pass.
#[test]
fn two_computers_cant_move_one_agent_at_once() {
    let world = World::new("two-computers");
    an_agent_with_files(&world);
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    ok(&world.lnk_agent_there(&["-a", "main", "pause", "--move", OTHER, "--agent-id", ID]));
    let refused = world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]);
    let text = said(&refused);
    assert!(!refused.status.success(), "{text}");
    assert!(text.contains("held here by another move"), "{text}");
    assert!(
        world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
    assert!(!world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    let claim = world
        .there
        .join(format!(".lnk/safe/agents/transfers/moving/{ID}.claim"));
    assert_eq!(std::fs::read_to_string(&claim).unwrap(), OTHER);
    // Every call of another move is refused there; the holder's pass.
    for args in [
        &[
            "-a",
            "main",
            "state-send",
            "--phase",
            "copy",
            "--move",
            MINE,
        ][..],
        &["-a", "main", "resume", "--move", MINE],
        &["-a", "main", "left", "--on", "c1", "--move", MINE],
        &["-a", "main", "pause", "--move", MINE],
    ] {
        let other = world.lnk_agent_there(args);
        assert!(!other.status.success(), "{args:?}: {}", said(&other));
    }
    ok(&world.lnk_agent_there(&["-a", "main", "pause", "--move", OTHER]));
    assert!(
        world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
}

/// A move names the agent by its ID as well as its name: another agent
/// made since under the name is never stopped, paused, sent, resumed or
/// removed for it.
#[test]
fn a_moves_calls_name_the_agent_by_id() {
    let world = World::new("by-id");
    an_agent_with_files(&world);
    let another = "f6e5d4c3b2a1";
    for args in [
        &["stop", "--agent-id", another][..],
        &["pause", "--agent-id", another],
        &["resume", "--agent-id", another],
        &["state-send", "--phase", "copy", "--agent-id", another],
        &["left", "--on", "b1", "--agent-id", another],
    ] {
        let refused = world.lnk_agent(args);
        assert!(!refused.status.success(), "{args:?}");
        assert!(
            said(&refused).contains("another agent has its name"),
            "{args:?}: {}",
            said(&refused)
        );
    }
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    ok(&world.lnk_agent(&["stop", "--agent-id", ID]));
}

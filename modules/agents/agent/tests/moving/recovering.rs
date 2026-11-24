//! A move that ends at each of its boundaries without finishing: killed
//! (the computer off, out of battery), the network dropped, the box lost
//! or gone dark. Each ends with the agent answering in one place, at once
//! or at the next `lnk agent move` or `lnk agent start` of it here.

use super::*;

impl World {
    fn mark(&self, name: &str) {
        write(&self.root.join(name), "");
    }

    fn unmark(&self, name: &str) {
        std::fs::remove_file(self.root.join(name)).unwrap();
    }

    fn journal(&self) -> PathBuf {
        self.here
            .join(format!(".lnk/safe/agents/transfers/moving/{ID}.toml"))
    }

    fn paused_here(&self) -> bool {
        self.here.join(".lnk/safe/agents/main/paused").exists()
    }

    fn here_has_it(&self) -> bool {
        self.here.join("Link/Safe/Agents/main/agent.toml").exists()
    }

    fn there_has_it(&self) -> bool {
        self.there.join("Link/Safe/Agents/main/agent.toml").exists()
    }
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Killed in its copy: the agent answered here throughout, and the next
/// move goes from the start.
#[test]
fn a_move_killed_in_its_copy_moves_on_the_next_try() {
    let world = World::new("kill-copy");
    an_agent_with_files(&world);
    world.mark("kill-at-copy");
    let killed = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    assert!(!killed.status.success(), "{}", said(&killed));
    assert!(world.journal().exists());
    assert!(world.here_has_it() && !world.paused_here());
    world.unmark("kill-at-copy");
    let moved = ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    assert!(moved.contains("It was still copying"), "{moved}");
    assert!(moved.contains("main runs on your box b1 now"), "{moved}");
    assert!(world.there_has_it() && !world.here_has_it());
    assert!(!world.journal().exists());
}

/// Killed once the agent is paused for its switch: it answers nowhere
/// until the next start here, which starts it, nothing of it left on the
/// box.
#[test]
fn a_move_killed_after_its_pause_answers_here_at_the_next_start() {
    let world = World::new("kill-switch");
    an_agent_with_files(&world);
    world.mark("kill-at-switch");
    let killed = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    assert!(!killed.status.success(), "{}", said(&killed));
    assert!(world.journal().exists());
    world.unmark("kill-at-switch");
    let started = said(&world.lnk_agent(&["start"]));
    assert!(started.contains("An earlier move of main"), "{started}");
    assert!(
        started.contains("nothing of it is left on your box b1"),
        "{started}"
    );
    assert!(!world.paused_here() && !world.journal().exists());
    assert!(world.here_has_it() && !world.there_has_it());
}

/// Killed after the agent landed on the box, before this computer heard
/// it answer there: the next start here finishes the move rather than
/// start a second copy, so it answers on the box alone.
#[test]
fn a_move_killed_before_its_acknowledgement_is_finished_not_started_twice() {
    let world = World::new("kill-answer");
    an_agent_with_files(&world);
    world.mark("kill-at-answer");
    let killed = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    assert!(!killed.status.success(), "{}", said(&killed));
    assert!(world.there_has_it());
    let started = world.lnk_agent(&["start"]);
    let text = said(&started);
    assert!(!started.status.success(), "{text}");
    assert!(
        text.contains("main answers on your box b1; this computer deleted its copy"),
        "{text}"
    );
    assert!(text.contains("main runs on your box b1"), "{text}");
    assert!(!world.here_has_it() && world.there_has_it());
    assert!(
        world
            .here
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml"))
            .exists()
    );
    assert!(!world.journal().exists());
}

/// The network dropped once the switch landed on the box: the agent
/// answers here again at once, and its stopped copy there is removed
/// once the box answers, at the next start here.
#[test]
fn a_network_drop_after_the_switch_leaves_a_stopped_copy_removed_later() {
    let world = World::new("drop-switch");
    an_agent_with_files(&world);
    world.mark("drop-after-switch");
    let failed = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    let text = said(&failed);
    assert!(!failed.status.success(), "{text}");
    assert!(
        text.contains("main answers on this computer again"),
        "{text}"
    );
    assert!(text.contains("the next lnk agent move"), "{text}");
    assert!(world.here_has_it() && !world.paused_here());
    assert!(
        world.there_has_it(),
        "the copy that landed there stays, stopped"
    );
    assert!(world.journal().exists());
    world.unmark("dark-b1");
    let started = said(&world.lnk_agent(&["start"]));
    assert!(
        started.contains("nothing of it is left on your box b1"),
        "{started}"
    );
    assert!(!world.there_has_it() && !world.journal().exists());
    assert!(world.here_has_it());
}

/// The box lost as the switch starts: the agent answers here again, and
/// nothing is left to finish.
#[test]
fn a_box_lost_in_the_switch_leaves_the_agent_answering_here() {
    let world = World::new("lost");
    an_agent_with_files(&world);
    world.mark("lose-at-switch");
    let failed = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    let text = said(&failed);
    assert!(!failed.status.success(), "{text}");
    assert!(
        text.contains("main answers on this computer again"),
        "{text}"
    );
    assert!(world.here_has_it() && !world.paused_here());
    assert!(!world.journal().exists());
}

/// The box stops answering once the agent started there: it may answer
/// there, so it stays stopped here rather than answer in both; once the
/// box answers, the next start here finds it not running there, takes
/// it back and starts it here.
#[test]
fn a_box_gone_dark_after_the_start_holds_the_agent_stopped_here() {
    let world = World::new("dark");
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    an_agent_with_files(&world);
    world.mark("dark-after-start");
    let failed = world.lnk_agent(&["move", "--on", "b1", "--yes"]);
    let text = said(&failed);
    assert!(!failed.status.success(), "{text}");
    assert!(text.contains("stays stopped on this computer"), "{text}");
    assert!(world.here_has_it() && world.journal().exists());
    // Asked again while the box is dark, it still holds.
    let held = world.lnk_agent(&["start"]);
    assert!(!held.status.success(), "{}", said(&held));
    assert!(world.journal().exists());
    world.unmark("dark-after-start");
    world.unmark("dark-b1");
    let started = said(&world.lnk_agent(&["start"]));
    assert!(
        started.contains("nothing of it is left on your box b1"),
        "{started}"
    );
    assert!(!world.paused_here() && !world.journal().exists());
    assert!(world.here_has_it() && !world.there_has_it());
}

/// A move that ended `back`, its stopped copy on a box gone dark, then
/// another move of the agent: the journal of the first stays beside the
/// second's, so the stopped copy is still removed once the box answers.
#[test]
fn a_move_after_one_ended_back_keeps_its_stopped_copy_to_remove() {
    let world = World::new("back-kept");
    an_agent_with_files(&world);
    world.mark("drop-after-switch");
    let failed = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    assert!(!failed.status.success(), "{}", said(&failed));
    assert!(world.there_has_it() && world.journal().exists());
    // Moved elsewhere while the box is dark: its copy there is kept.
    let moved = ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
    assert!(
        moved.contains("is removed once your box b1 answers"),
        "{moved}"
    );
    assert!(world.journal().exists(), "the back journal stays");
    assert!(world.there_has_it());
    world.unmark("dark-b1");
    // Its stopped copy on the box is listed beside it until then.
    let back = ok(&world.lnk_agent(&["move", "--here", "--agent", "main@host", "--no-start"]));
    assert!(
        back.contains("nothing of it is left on your box b1"),
        "{back}"
    );
    assert!(!world.there_has_it() && !world.journal().exists());
    assert!(world.here_has_it());
}

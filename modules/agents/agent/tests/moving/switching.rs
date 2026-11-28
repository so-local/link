//! The copy and the switch: only what changed goes back, and a refusal,
//! a failure, a silence or ctrl-c leaves the agent where it was.

use super::*;

/// A move copies the agent's files while it answers, then switches in
/// seconds, saying how long the switch took; moving back within 30 days,
/// what it left here starts the copy, so only what changed is sent. Its
/// conversation lands in its own folder, beside Home.
#[test]
fn a_move_copies_then_switches_and_back_sends_only_what_changed() {
    let world = World::new("two-phases");
    an_agent_with_files(&world);
    let said = ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    assert!(said.contains("Copied 5 files"), "{said}");
    assert!(said.contains("while main answered"), "{said}");
    assert!(said.contains("Switched in "), "{said}");
    // Nothing waits on the box once it switched.
    assert!(
        !world
            .there
            .join(format!(".lnk/safe/agents/transfers/receiving/{ID}"))
            .exists()
    );

    // On the box it changes one file; back here, only that one comes of
    // its files, with its harness's state, which isn't kept here.
    write(
        &world.files(&world.there).join("notes.txt"),
        "edited on the box",
    );
    let said = ok(&world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]));
    assert!(said.contains("Copied 2 files,"), "{said}");
    let files = every_file(&world.files(&world.here));
    assert_eq!(files["notes.txt"], "edited on the box");
    assert_eq!(files["taxes/2025.txt"], "a tax report");
    assert!(!files.contains_key("chats/telegram-42/events.jsonl"));
    let own = every_file(&world.here.join("Link/Agents/main/agent"));
    assert_eq!(
        own["chats/telegram-42/events.jsonl"],
        "{\"seq\":1,\"event\":1}\n"
    );
    assert!(
        !world
            .here
            .join(format!(".lnk/safe/agents/transfers/receiving/{ID}"))
            .exists()
    );
}

/// Between machines the copy carries the harness's state too, so the
/// switch sends only what changed of it since, and what the harness
/// deleted meanwhile is gone where it lands.
#[test]
fn the_switch_sends_only_what_changed_of_the_harness_state() {
    let world = World::new("state-in-copy");
    an_agent_here(&world);
    let memory = world.agent(&world.here).join("memory");
    write(&memory.join("same"), "unchanged");
    write(&memory.join("changed"), "before");
    write(&memory.join("gone"), "deleted after the copy");
    let staged = world.there.join(format!(
        ".lnk/safe/agents/transfers/receiving/{ID}/harness/.fake/memory"
    ));
    write(
        &world.root.join("before-switch"),
        &format!(
            "echo after > '{memory}/changed'\nrm '{memory}/gone'\necho staged > '{staged}/same'\n",
            memory = memory.display(),
            staged = staged.display()
        ),
    );
    let said = ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    assert!(said.contains("Copied 3 files"), "{said}");
    let landed = world.agent(&world.there).join("memory");
    // Held the same since the copy: not sent again.
    assert_eq!(
        std::fs::read_to_string(landed.join("same")).unwrap(),
        "staged\n"
    );
    assert_eq!(
        std::fs::read_to_string(landed.join("changed")).unwrap(),
        "after\n"
    );
    assert!(!landed.join("gone").exists());
}

/// An adapter whose harness's state the box refuses to say, when
/// `refused` is in the box's home: the box can't take the agent's switch.
const PICKY: &str = r#"#!/bin/sh
case "$1" in
  state) [ -e "$HOME/refused" ] && { echo "refused here" >&2; exit 1; }
         echo '{"include": ["~/.fake"], "exclude": []}' ;;
  traces) echo '[]' ;;
  *) exit 0 ;;
esac
"#;

/// A switch the other end refuses leaves the agent answering where it
/// was, and the other end with nothing of it: no settings, no copy.
#[test]
fn a_switch_refused_leaves_the_agent_where_it_was() {
    let world = World::new("switch-refused");
    crate::common::write_adapter(&world.bin.join("lnk-picky"), PICKY).unwrap();
    crate::common::write_agent(
        &world.here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"picky\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(&world.here).display()
        ),
    );
    write(&world.files(&world.here).join("notes.txt"), "mine");
    write(&world.there.join("refused"), "");
    let refused = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    assert!(!refused.status.success());
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(
        said.contains("lnk-picky can't say what of its home"),
        "{said}"
    );
    assert!(said.contains("answers on this computer again"), "{said}");
    // Its take-back's start is the move's own, let through, and lets go
    // of the move's hold.
    assert!(!said.contains("being moved"), "{said}");
    assert!(said.contains("Setting up picky"), "{said}");
    assert!(
        !world
            .here
            .join(format!(".lnk/safe/agents/transfers/moving/{ID}.claim"))
            .exists()
    );
    // Here, as it was: its settings, its files, and no longer paused.
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(!world.here.join(".lnk/safe/agents/main/paused").exists());
    assert_eq!(
        std::fs::read_to_string(world.files(&world.here).join("notes.txt")).unwrap(),
        "mine"
    );
    assert!(
        !world
            .here
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml"))
            .exists()
    );
    // There, nothing.
    assert!(
        !world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
    assert!(
        !world
            .there
            .join(format!(".lnk/safe/agents/transfers/receiving/{ID}"))
            .exists()
    );
}

/// A cloud that hosts agents refusing a move in: nothing moves, and the
/// move says why, and when the next is allowed, as this machine's time.
#[test]
fn a_move_refused_by_where_its_going_moves_nothing_and_says_why_and_when() {
    let world = World::new("hosted-refused");
    an_agent_with_files(&world);
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    write(
        &world.root.join("host/refuse.json"),
        &format!(r#"{{"why": "no more moves today", "retry_at": {at}}}"#),
    );
    let refused = world.lnk_agent(&["move", "--cloud", "host", "--yes"]);
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        said.contains("host refused the move (no more moves today)"),
        "{said}"
    );
    let when = said
        .split("the next move is allowed at ")
        .nth(1)
        .unwrap_or_else(|| panic!("{said}"));
    let clock: Vec<char> = when.chars().take(5).collect();
    assert!(
        clock[2] == ':'
            && clock
                .iter()
                .enumerate()
                .all(|(i, c)| i == 2 || c.is_ascii_digit()),
        "{said}"
    );
    assert_eq!(host_log(&world), ["agent-in"]);
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(!world.root.join(format!("host/agents/{ID}")).exists());
    assert!(!world.root.join(format!("host/staging/{ID}")).exists());
}

/// A failure at any step of a move to a cloud that hosts agents, or from
/// one, ends the move there (`agent-abort`): the agent answers where it
/// was, and nothing of it is left where it was going.
#[test]
fn a_failure_in_each_step_leaves_the_agent_where_it_was() {
    for step in ["agent-receive-copy", "agent-receive-switch"] {
        let world = World::new(&format!("in-{step}"));
        an_agent_with_files(&world);
        // What it says, shown cleaned: no escape reaches the terminal.
        write(
            &world.root.join(format!("host/fail-{step}")),
            "\x1b]0;owned\x07said\n",
        );
        let failed = world.lnk_agent(&["move", "--cloud", "host", "--yes"]);
        assert!(!failed.status.success(), "{step}");
        let said = String::from_utf8_lossy(&failed.stderr);
        assert!(said.contains("]0;owned"), "{step}: {said}");
        assert!(
            !said.contains('\x1b') && !said.contains('\x07'),
            "{step}: {said}"
        );
        assert!(
            world.here.join("Link/Safe/Agents/main/agent.toml").exists(),
            "{step}"
        );
        assert!(!world.here.join(".lnk/safe/agents/main/paused").exists());
        assert!(
            world.files(&world.here).join("notes.txt").exists(),
            "{step}"
        );
        assert!(
            host_log(&world).contains(&"agent-abort".to_string()),
            "{step}"
        );
        assert!(
            !world.root.join(format!("host/agents/{ID}")).exists(),
            "{step}"
        );
        assert!(
            !world.root.join(format!("host/staging/{ID}")).exists(),
            "{step}"
        );
    }
    for step in ["agent-send-copy", "agent-send-switch", "receive"] {
        let world = World::new(&format!("out-{step}"));
        an_agent_with_files(&world);
        ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
        match step {
            // Here, another agent of its name: here refuses it.
            "receive" => crate::common::write_agent(
                &world.here,
                "main",
                "id = \"agent:b2c3d4e5f6a1\"\nservice = \"service:o2t3h4e5r6s7\"\nharness = \"fake\"\n",
            ),
            _ => write(&world.root.join(format!("host/fail-{step}")), ""),
        }
        let failed = world.lnk_agent(&["move", "--here", "--agent", "main@host", "--no-start"]);
        assert!(!failed.status.success(), "{step}");
        assert!(
            host_log(&world).contains(&"agent-abort".to_string()),
            "{step}"
        );
        let agent =
            std::fs::read_to_string(world.root.join(format!("host/agents/{ID}/agent.json")))
                .unwrap();
        assert!(agent.contains("\"running\""), "{step}: {agent}");
        let here = std::fs::read_to_string(world.here.join("Link/Safe/Agents/main/agent.toml"))
            .unwrap_or_default();
        assert!(!here.contains(ID), "{step}: {here}");
    }
}

/// A move that ends with the agent not answering where it went, from a
/// cloud that hosts agents: its copy there goes again, the cloud ends the
/// move (`agent-abort`), so the agent answers there and a move tried again
/// isn't blocked, and `lnk` says it failed.
#[test]
fn a_move_whose_agent_never_answers_where_it_went_takes_it_back() {
    let world = World::new("never-answers");
    an_agent_with_files(&world);
    ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
    write(&world.root.join("never-answers"), "");
    let failed = world.lnk_agent(&["move", "--here", "--agent", "main@host", "--no-start"]);
    let said = crate::moving::text(&failed);
    assert!(!failed.status.success(), "{said}");
    assert!(
        said.contains("main answers on your cloud account host again"),
        "{said}"
    );
    let log = host_log(&world);
    assert!(log.contains(&"agent-abort".to_string()), "{log:?}");
    assert!(!log.contains(&"agent-left".to_string()), "{log:?}");
    let agent =
        std::fs::read_to_string(world.root.join(format!("host/agents/{ID}/agent.json"))).unwrap();
    assert!(agent.contains("\"running\""), "{agent}");
    let here = std::fs::read_to_string(world.here.join("Link/Safe/Agents/main/agent.toml"))
        .unwrap_or_default();
    assert!(!here.contains(ID), "{here}");
    // Tried again, it moves.
    std::fs::remove_file(world.root.join("never-answers")).unwrap();
    ok(&world.lnk_agent(&["move", "--here", "--agent", "main@host", "--no-start"]));
}

/// A move from this computer whose agent doesn't answer where it went:
/// it answers here again, no longer paused, and `lnk` says it failed.
#[test]
fn a_move_whose_agent_never_answers_on_a_box_resumes_it_here() {
    let world = World::new("never-answers-box");
    an_agent_with_files(&world);
    write(&world.root.join("never-answers-b1"), "");
    let failed = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    let said = crate::moving::text(&failed);
    assert!(!failed.status.success(), "{said}");
    assert!(
        said.contains("main answers on this computer again"),
        "{said}"
    );
    assert!(!world.here.join(".lnk/safe/agents/main/paused").exists());
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(
        !world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists(),
        "its copy on b1 stays"
    );
}

/// A move whose agent started on the box but whose status there can't be
/// read, and whose copy there can't be removed: it may run there, so it
/// stays stopped here, never answering in both; never taken for absent.
#[test]
fn a_copy_whose_status_fails_and_stays_holds_the_agent_here() {
    let world = World::new("status-fails");
    an_agent_with_files(&world);
    write(&world.root.join("never-answers-b1"), "");
    write(&world.root.join("never-leaves-b1"), "");
    let failed = world.lnk_agent(&["move", "--on", "b1"]);
    let said = crate::moving::text(&failed);
    assert!(!failed.status.success(), "{said}");
    assert!(said.contains("may run on your box b1"), "{said}");
    assert!(!said.contains("answers on this computer again"), "{said}");
    // Kept for the next move to finish or take back once b1 answers.
    assert!(
        world
            .here
            .join(format!(".lnk/safe/agents/transfers/moving/{ID}.toml"))
            .exists(),
        "{said}"
    );
}

/// Ctrl-C after the agent is paused for its switch, as the move asks what
/// its harness needs where it's going: the move ends as a failure does,
/// never lnk midway, so it answers here again and nothing is left there.
#[test]
fn ctrl_c_during_the_switch_leaves_the_agent_where_it_was() {
    let world = World::new("ctrl-c");
    an_agent_with_files(&world);
    write(&world.root.join("stop-at-needs"), "");
    let failed = world.lnk_agent(&["move", "--on", "b1"]);
    let said = crate::moving::text(&failed);
    assert!(!failed.status.success(), "{said}");
    assert!(
        said.contains("main answers on this computer again"),
        "{said}"
    );
    assert!(!world.here.join(".lnk/safe/agents/main/paused").exists());
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(
        !world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
}

/// Two moves of one agent at once: the second is refused, saying why,
/// and moves nothing.
#[test]
fn a_second_move_of_an_agent_moving_is_refused() {
    let world = World::new("twice");
    an_agent_with_files(&world);
    let lock = world
        .here
        .join(format!(".lnk/safe/agents/transfers/moving/{ID}.lock"));
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let held = std::fs::File::create(&lock).unwrap();
    held.lock().unwrap();
    let refused = world.lnk_agent(&["move", "--on", "b1"]);
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("main is being moved already"), "{said}");
    assert!(
        !world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
    // Let go of by name, never only dropped: a lock is its open file's,
    // which a child another test forks shares until it runs its program,
    // so a dropped one may be held a while yet.
    held.unlock().unwrap();
    drop(held);
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
}

/// A restore whose buckets are an agent's here already, under another
/// name: refused, and that agent keeps its keys, which are granted by
/// its ID.
#[test]
fn a_restore_of_an_agent_here_already_is_refused() {
    let world = World::new("restore-twice");
    an_agent_here(&world);
    let refused = world.lnk_agent(&["restore", "other", "--no-start"]);
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{said}");
    assert!(
        said.contains("the buckets restored are main's, an agent here already"),
        "{said}"
    );
    // What it pulled for it is gone again.
    assert!(!world.here.join("Link/Agents/other").exists());
    assert_eq!(
        crate::common::kept_key(
            &world.here,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .as_deref(),
        Some("sk-ant-test")
    );
}

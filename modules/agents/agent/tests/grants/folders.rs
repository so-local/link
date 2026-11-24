//! An agent's folders: its prompt, conversations and skills.

use super::*;

/// `lnk agent show --instructions` runs its harness's `instructions` in a
/// sandbox that reads the agent's own folder, where its instructions and
/// skills are, as its turns read them.
#[test]
fn its_prompt_is_read_from_its_own_folder() {
    let world = World::new("prompt");
    if world.skips() {
        return;
    }
    ok(&world.start("main", &["create", "main", "--harness", "tidy"]));
    let own = world.home.join("Link/Agents/main/agent");
    std::fs::write(own.join("instructions/instruction.md"), "Be brief.\n").unwrap();
    std::fs::create_dir_all(own.join("skills/tides")).unwrap();
    std::fs::write(own.join("skills/tides/SKILL.md"), "Read a tide table.\n").unwrap();
    let said = ok(&world.lnk(&["show", "--instructions"]));
    assert!(
        said.contains("Be brief.") && said.contains("Read a tide table."),
        "{said}"
    );
}

/// `lnk agent chat export --format` runs its harness's step writing the
/// agent's conversations alone: never Home, whatever the harness.
#[test]
fn moving_conversations_writes_them_alone() {
    let world = World::new("conversations");
    if world.skips() {
        return;
    }
    ok(&world.start("main", &["create", "main", "--harness", "tidy"]));
    let said = ok(&world.lnk(&["chat", "export", "--format", "log"]));
    assert!(said.contains("HOME-READ-ONLY"), "{said}");
    let own = world.home.join("Link/Agents/main/agent");
    assert!(own.join("chats/moved").is_file());
    assert!(!world.home.join("Link/Agents/main/home/planted").exists());
}

/// An agent made on this release is laid out as two folders from its
/// first start: a repository it's given as Home keeps its own `skills/`,
/// `instructions/` and `chats/`, never moved into the agent's.
#[test]
fn a_new_agents_home_keeps_its_own_folders() {
    let world = World::new("new-home");
    if world.skips() {
        return;
    }
    let repo = world.home.join("repo");
    for path in [
        "skills/lint/SKILL.md",
        "instructions/style.md",
        "chats/notes.md",
    ] {
        std::fs::create_dir_all(repo.join(path).parent().unwrap()).unwrap();
        std::fs::write(repo.join(path), "the repository's").unwrap();
    }
    let files = repo.display().to_string();
    let made = [
        "create",
        "main",
        "--harness",
        "tidy",
        "--foreground",
        "--run.files",
        &files,
    ];
    ok(&world.lnk(&made));
    ok(&world.lnk(&["start", "--foreground"]));
    let own = world.home.join("Link/Agents/main/agent");
    assert!(own.is_dir());
    for path in [
        "skills/lint/SKILL.md",
        "instructions/style.md",
        "chats/notes.md",
    ] {
        assert_eq!(read(&repo.join(path)), "the repository's", "{path}");
        assert!(!own.join(path).exists(), "{path}");
    }
}

/// An agent first saved by `lnk agent harness set`, before it ever started, is
/// laid out as two folders already: a repository given at its first start
/// keeps its own folders.
#[test]
fn an_agent_made_by_use_keeps_a_repositorys_folders() {
    let world = World::new("use-home");
    if world.skips() {
        return;
    }
    let repo = world.home.join("repo");
    std::fs::create_dir_all(repo.join("skills/lint")).unwrap();
    std::fs::write(repo.join("skills/lint/SKILL.md"), "the repository's").unwrap();
    let used = world.lnk(&["harness", "set", "tidy"]);
    assert!(
        world
            .home
            .join("Link/Safe/Agents/main/agent.toml")
            .is_file(),
        "{}",
        text(&used)
    );
    let own = world.home.join("Link/Agents/main/agent");
    assert!(own.is_dir(), "{}", text(&used));
    // Saved, though it couldn't start here (no systemd): run by the
    // harness that keeps to the layout.
    let settings = world.home.join("Link/Safe/Agents/main/agent.toml");
    std::fs::write(
        &settings,
        read(&settings).replace("harness = \"link\"", "harness = \"tidy\""),
    )
    .unwrap();
    let files = repo.display().to_string();
    ok(&world.lnk(&["start", "--foreground", "--run.files", &files]));
    assert_eq!(read(&repo.join("skills/lint/SKILL.md")), "the repository's");
    assert!(!own.join("skills/lint").exists());
}

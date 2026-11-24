//! The settings, instructions and skills a prompt is made of.

use super::*;

/// Instructions that are a link, a folder or a pipe are left out,
/// never followed or waited on, and the turn goes on; missing or
/// empty, there are none.
#[test]
fn instructions_are_a_file_of_their_own() {
    let (root, agents) = world("instructions");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let files = root.join("main/own");
    let standing = "instructions/instruction.md";
    std::fs::create_dir_all(files.join("instructions")).unwrap();
    let path = files.join(standing);
    assert_eq!(store.instruction(&main, standing).unwrap(), None);
    std::fs::write(&path, " \n").unwrap();
    assert_eq!(store.instruction(&main, standing).unwrap(), None);
    std::fs::remove_file(&path).unwrap();
    std::fs::write(root.join("other.md"), "Be a pirate.").unwrap();
    std::os::unix::fs::symlink(root.join("other.md"), &path).unwrap();
    assert_eq!(store.instruction(&main, standing).unwrap(), None);
    std::fs::remove_file(&path).unwrap();
    fifo(&path);
    assert_eq!(store.instruction(&main, standing).unwrap(), None);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(store.instruction(&main, standing).unwrap(), None);
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(root.join("other.md"), &path).unwrap();
    assert_eq!(
        store.instruction(&main, standing).unwrap().as_deref(),
        Some("Be a pirate.")
    );
    // Never outside the agent's files, nor through a folder that's a
    // link.
    for outside in ["../other.md", "/etc/passwd", "instructions//x", ""] {
        assert!(store.instruction(&main, outside).is_err(), "{outside}");
    }
    std::fs::create_dir_all(root.join("elsewhere")).unwrap();
    std::fs::write(root.join("elsewhere/base.md"), "Be a parrot.").unwrap();
    std::fs::create_dir_all(root.join("main/files")).unwrap();
    std::os::unix::fs::symlink(root.join("elsewhere"), root.join("main/files/linked")).unwrap();
    assert_eq!(store.instruction(&main, "linked/base.md").unwrap(), None);
    // `memory/` is Home's documents, as the agent sees them, never
    // memory's index in its own folder.
    std::fs::create_dir_all(root.join("main/files/memory")).unwrap();
    std::fs::create_dir_all(root.join("main/own/memory")).unwrap();
    std::fs::write(root.join("main/files/memory/a.md"), "Home's.").unwrap();
    std::fs::write(root.join("main/own/memory/a.md"), "the index's").unwrap();
    assert_eq!(
        store.instruction(&main, "memory/a.md").unwrap().as_deref(),
        Some("Home's.")
    );
    // Another agent's are its own.
    assert_eq!(
        store
            .instruction(&Agent::new("other").unwrap(), standing)
            .unwrap(),
        None
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Skills are the folders of `skills/` whose `SKILL.md` says their
/// name, by name: never one through a link, one named otherwise, or
/// another agent's.
#[test]
fn skills_are_the_folders_that_say_so() {
    let (root, agents) = world("skills");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let skills = root.join("main/own/skills");
    assert!(store.skills(&main).unwrap().is_empty());
    let skill = |name: &str, front: &str| {
        std::fs::create_dir_all(skills.join(name)).unwrap();
        std::fs::write(skills.join(name).join("SKILL.md"), front).unwrap();
    };
    skill(
        "tides",
        "---\nname: tides\ndescription: Read a tide table.\n---\n",
    );
    skill(
        "bread",
        "---\nname: bread\ndescription: >\n  Bake\n  bread.\n---\n# Bread\n",
    );
    // Named otherwise than its folder, with no front matter, without
    // a SKILL.md, a name the standard doesn't take, hidden.
    skill("other", "---\nname: tides\ndescription: Not this.\n---\n");
    skill("plain", "# Just markdown\n");
    std::fs::create_dir_all(skills.join("empty")).unwrap();
    skill("Big", "---\nname: Big\ndescription: No.\n---\n");
    skill(".hidden", "---\nname: hidden\ndescription: No.\n---\n");
    // A link to a skill elsewhere, and a SKILL.md that's a link.
    let elsewhere = root.join("elsewhere/linked");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(
        elsewhere.join("SKILL.md"),
        "---\nname: linked\ndescription: No.\n---\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&elsewhere, skills.join("linked")).unwrap();
    std::fs::create_dir_all(skills.join("pointer")).unwrap();
    std::os::unix::fs::symlink(elsewhere.join("SKILL.md"), skills.join("pointer/SKILL.md"))
        .unwrap();
    let found: Vec<(String, String)> = store
        .skills(&main)
        .unwrap()
        .into_iter()
        .map(|s| (s.name, s.description))
        .collect();
    assert_eq!(
        found,
        [
            ("bread".to_string(), "Bake bread.".to_string()),
            ("tides".to_string(), "Read a tide table.".to_string()),
        ]
    );
    assert!(
        store
            .skills(&Agent::new("other").unwrap())
            .unwrap()
            .is_empty()
    );
    // Kept as read until its file changes: then read again.
    skill(
        "tides",
        "---\nname: tides\ndescription: Read the tides, in feet.\n---\n",
    );
    skill("plain", "---\nname: plain\ndescription: Now one.\n---\n");
    let found: Vec<String> = store
        .skills(&main)
        .unwrap()
        .into_iter()
        .map(|s| s.description)
        .collect();
    assert_eq!(
        found,
        ["Bake bread.", "Now one.", "Read the tides, in feet."]
    );
    // The folder of skills itself a link: refused.
    std::fs::rename(&skills, root.join("moved")).unwrap();
    std::os::unix::fs::symlink(root.join("moved"), &skills).unwrap();
    assert!(store.skills(&main).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

/// Settings are kept and read back, each agent's its own; a change
/// is read at once, and a link is never followed.
#[test]
fn settings_are_kept_per_agent() {
    let (root, agents) = world("config");
    let store = Files::new(agents);
    let (main, other) = (Agent::new("main").unwrap(), Agent::new("other").unwrap());
    let err = store.config(&main).unwrap_err();
    assert!(format!("{err:#}").contains("lnk agent start"), "{err:#}");
    let config = |key: &str| Config {
        endpoint_key: Some(key.to_string().into()),
        ..Config::default()
    };
    store.set_config(&main, &config("first")).unwrap();
    assert_eq!(store.config(&main).unwrap(), config("first"));
    // Read again once it's written again, by any process.
    store.set_config(&main, &config("again")).unwrap();
    assert_eq!(store.config(&main).unwrap(), config("again"));
    assert!(store.config(&other).is_err());
    store.set_config(&other, &config("other's")).unwrap();
    assert_eq!(store.config(&main).unwrap(), config("again"));
    assert_eq!(store.config(&other).unwrap(), config("other's"));
    use std::os::unix::fs::PermissionsExt;
    let path = root.join("main/home/config.json");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    std::fs::rename(&path, root.join("elsewhere.json")).unwrap();
    std::os::unix::fs::symlink(root.join("elsewhere.json"), &path).unwrap();
    assert!(store.config(&main).is_err());
    assert!(store.config(&Agent::new("stranger").unwrap()).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

/// Settings a harness was bound with are held in memory: no file is
/// read or written for them, and they're only their own agent's.
#[test]
fn settings_held_are_never_a_file() {
    let (root, agents) = world("held");
    let (main, other) = (Agent::new("main").unwrap(), Agent::new("other").unwrap());
    let config = |key: &str| Config {
        endpoint_key: Some(key.to_string().into()),
        ..Config::default()
    };
    let store = Files::new(agents).holding(&main, config("bound"));
    assert_eq!(store.config(&main).unwrap(), config("bound"));
    store.set_config(&main, &config("again")).unwrap();
    assert_eq!(store.config(&main).unwrap(), config("again"));
    assert!(!root.join("main/home/config.json").exists());
    assert!(store.config(&other).is_err());
    // Held for an agent it doesn't answer for: still none.
    let stranger = Agent::new("stranger").unwrap();
    let store = store.holding(&stranger, config("x"));
    assert!(store.config(&stranger).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

/// A folder on the way to a file of the prompt, or a skill, that's a
/// link: left out, never followed, as is the agent's own folder that
/// is one.
#[test]
fn a_linked_folder_of_the_prompt_is_left_out() {
    let (root, agents) = world("prompt-links");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let own = root.join("main/own");
    let elsewhere = root.join("elsewhere");
    std::fs::create_dir_all(elsewhere.join("sub")).unwrap();
    std::fs::write(elsewhere.join("instruction.md"), "Be a pirate.").unwrap();
    std::fs::write(elsewhere.join("sub/a.md"), "Be a parrot.").unwrap();
    std::fs::write(
        elsewhere.join("SKILL.md"),
        "---\nname: tides\ndescription: Not this.\n---\n",
    )
    .unwrap();
    // `instructions/` itself, and a folder in it.
    std::fs::create_dir_all(&own).unwrap();
    std::os::unix::fs::symlink(&elsewhere, own.join("instructions")).unwrap();
    let standing = "instructions/instruction.md";
    assert_eq!(store.instruction(&main, standing).unwrap(), None);
    std::fs::remove_file(own.join("instructions")).unwrap();
    std::fs::create_dir_all(own.join("instructions")).unwrap();
    std::os::unix::fs::symlink(elsewhere.join("sub"), own.join("instructions/sub")).unwrap();
    assert_eq!(
        store.instruction(&main, "instructions/sub/a.md").unwrap(),
        None
    );
    // A skill's folder.
    std::fs::create_dir_all(own.join("skills")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, own.join("skills/tides")).unwrap();
    assert!(store.skills(&main).unwrap().is_empty());
    // The agent's own folder: refused, never followed.
    std::fs::rename(&own, root.join("own-moved")).unwrap();
    std::os::unix::fs::symlink(root.join("own-moved"), &own).unwrap();
    assert!(store.instruction(&main, standing).is_err());
    assert!(store.skills(&main).is_err());
    assert!(
        store
            .append(
                &main,
                &Thread::new("telegram:42").unwrap(),
                &said(1, "Hi"),
                &[]
            )
            .is_err()
    );
    assert!(!root.join("own-moved/chats").exists());
    let _ = std::fs::remove_dir_all(&root);
}

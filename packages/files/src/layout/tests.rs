use super::*;

#[test]
fn an_owners_own_place_is_under_a_system_root_and_never_leaves_it() {
    #[derive(serde::Deserialize)]
    struct Owned {
        config: SystemPlace,
    }
    let owned: Owned = system_places("config = { root = \"settings\", path = \"owned.json\" }");
    assert_eq!(owned.config.path(), Path::new("/etc/lnk/owned.json"));
    let state: Owned = system_places("config = { root = \"state\", path = \"owned\" }");
    assert_eq!(
        state.config.paths_for(SystemOs::Linux),
        [
            PathBuf::from("/var/lib/lnk/owned"),
            PathBuf::from("/var/lib/private/lnk/owned")
        ]
    );
    for wrong in ["../owned", "/etc/owned"] {
        let text = format!("config = {{ root = \"settings\", path = \"{wrong}\" }}");
        assert!(toml::from_str::<Owned>(&text).is_err(), "{wrong}");
    }
}

/// An owner outside the catalogue declares its folders in `~/.lnk`: each
/// in the part its kind is in, in a folder no owner of `[owners]` has.
#[test]
fn an_outside_owners_place_is_in_its_own_folder_of_its_part() {
    #[derive(serde::Deserialize)]
    struct Owned {
        state: HiddenPlace,
    }
    let read = |path: &str, kind: &str| {
        toml::from_str::<Owned>(&format!(
            "state = {{ path = \"{path}\", kind = \"{kind}\" }}"
        ))
    };
    let owned = read("safe/outside/state.toml", "control").unwrap();
    assert_eq!(
        owned.state.at(Path::new("/h/.lnk")),
        Path::new("/h/.lnk/safe/outside/state.toml")
    );
    assert_eq!(
        owned.state.in_home(Path::new("/h")),
        Path::new("/h/.lnk/safe/outside/state.toml")
    );
    assert!(read("data/outside", "data").is_ok());
    for (path, kind) in [
        ("data/outside", "control"),
        ("safe/agents/x", "control"),
        ("safe/link", "control"),
        ("safe", "control"),
        ("../outside", "control"),
        ("/safe/outside", "control"),
    ] {
        assert!(read(path, kind).is_err(), "{path} as {kind}");
    }
}

#[test]
fn system_paths_have_platform_state_roots_and_checked_kinds() {
    assert_eq!(
        System::SecretStore.path_for(SystemOs::Linux),
        Path::new("/var/lib/lnk/secret")
    );
    assert_eq!(
        System::SecretStore.path_for(SystemOs::Macos),
        Path::new("/var/db/lnk/secret")
    );
    assert_eq!(
        System::GuestService.path(),
        Path::new("/etc/lnk/service.toml")
    );
    assert_eq!(System::CoreProgram.kind(), Kind::Exec);
    let safes = system_safes();
    assert!(safes.contains(&SystemRoot::Settings.path()));
    assert!(safes.contains(&System::SecretStore.path()));
    assert!(safes.contains(&System::WorkerJournal.path()));
    assert_eq!(
        System::SecretStore.aliases_for(SystemOs::Linux),
        [PathBuf::from("/var/lib/private/lnk/secret")]
    );
    assert!(!safes.contains(&System::Data.path()));
    let shipped = include_str!("../../layout.toml");
    for (from, to) in [
        (
            "root = \"settings\", path = \"kind.toml\", kind = \"control\"",
            "root = \"exec\", path = \"kind.toml\", kind = \"control\"",
        ),
        ("settings = \"/etc/lnk\"", "settings = \"../etc/lnk\""),
        ("shared-mode = 0o755", "shared-mode = 0o700"),
        ("shared-mode = 0o755", "shared-mode = 0o777"),
        (
            "linux-private = \"/var/lib/private/lnk\"",
            "linux-private = \"/var/lib/lnk/private\"",
        ),
        ("exec = \"/usr/local/lib/lnk\"", "exec = \"/etc/lnk/exec\""),
    ] {
        let layout: Layout = toml::from_str(&shipped.replace(from, to)).unwrap();
        assert!(layout.validate().is_err());
    }
}

#[test]
fn memory_runtime_access_does_not_include_service_control_files() {
    let hidden = Path::new("/home/me/.lnk");
    let folder = Hidden::SharedMemory.at(hidden).join("me@here");
    let service = MemoryService::at(&folder);
    let input = service.input("0123456789abcdef").unwrap();
    for path in [service.working(), service.working().join("memory.sock")] {
        assert!(in_memory_runtime(&path, hidden, true));
        assert!(in_memory_runtime(&path, hidden, false));
    }
    assert!(in_memory_runtime(&input, hidden, false));
    assert!(!in_memory_runtime(&input, hidden, true));
    for path in [
        folder.clone(),
        folder.join("agents/main.json"),
        folder.join("generation.json"),
        folder.join("bridges/main/storage.sock"),
        service.working().join("../agents/main.json"),
    ] {
        assert!(
            !in_memory_runtime(&path, hidden, false),
            "{}",
            path.display()
        );
        assert!(
            !in_memory_runtime(&path, hidden, true),
            "{}",
            path.display()
        );
    }
    for generation in ["", "../config", "a/b", "not-hex"] {
        assert!(service.input(generation).is_err());
    }
}

/// A harness's home, an agent's or its owner's shared one, and what's in
/// it, is one; the folder of them, the rest of its agent's folder and a
/// path climbing out are not.
#[test]
fn a_harness_home_is_found_in_its_agent_s_folder() {
    let hidden = Path::new("/home/me/.lnk");
    let homes = harness_homes_at(hidden, "main");
    assert_eq!(homes, hidden.join("data/agents/main/harnesses"));
    assert!(in_harness_home(&homes.join("hermes"), hidden));
    assert!(in_harness_home(&homes.join("hermes/.env"), hidden));
    // The home its owner's agents share.
    let shared = hidden.join("data/agents/shared-harnesses");
    assert!(in_harness_home(&shared.join("me@here"), hidden));
    assert!(in_harness_home(&shared.join("me@here/.hermes"), hidden));
    for path in [
        shared.clone(),
        shared.join("me@here/../../memory"),
        homes.clone(),
        hidden.join("state/agents/main"),
        hidden.join("state/agents/main/proxy/socket"),
        hidden.join("config/home.toml"),
        homes.join("hermes/../../proxy"),
        PathBuf::from("/elsewhere/state/agents/main/harnesses/hermes"),
    ] {
        assert!(!in_harness_home(&path, hidden), "{}", path.display());
    }
}

#[test]
fn home_names_refuse_hidden_components_and_escape() {
    for path in ["Agents/main", "Config/settings.toml", "index.sqlite"] {
        visible(Path::new(path)).unwrap();
    }
    for path in [
        "",
        "/Link",
        "../Link",
        ".lnk",
        "Agents/.moving",
        "Agents/../config",
    ] {
        assert!(visible(Path::new(path)).is_err(), "{path}");
    }
    for area in Home::ALL {
        visible(Path::new(area.relative())).unwrap();
    }
}

#[test]
fn discovery_honors_a_moved_home_without_creating_an_unused_root() {
    let temp = link_test_kit::TempDir::new("layout-find");
    let user = temp.canonicalize().unwrap();
    let hidden = user.join(".lnk");
    assert_eq!(link_in(&user, &hidden).unwrap(), user.join("Link"));
    assert!(!hidden.exists());
    assert!(!user.join("Link").exists());
    crate::write_private(
        &Hidden::Map.at(&hidden),
        b"schema = 1\nlink = 'Documents/Link'\n",
    )
    .unwrap();
    assert_eq!(
        link_in(&user, &hidden).unwrap(),
        user.join("Documents/Link")
    );
    assert_eq!(
        Hidden::Installation.at(&hidden),
        hidden.join("safe/installation.toml")
    );
    assert_eq!(
        Hidden::OpenclawDoctorRelease.in_home(&user.join("isolated")),
        user.join("isolated/.lnk/data/harnesses/openclaw/doctor-release")
    );
    crate::write_private(
        &Hidden::Map.at(&hidden),
        b"schema = 1\nlink = '.private/Link'\n",
    )
    .unwrap();
    assert!(link_in(&user, &hidden).is_err());
}

#[test]
fn every_home_location_is_visible_and_every_hidden_location_is_relative() {
    for area in Home::ALL {
        visible(Path::new(area.relative())).unwrap();
    }
    for area in Hidden::ALL {
        relative(Path::new(area.relative())).unwrap();
    }
}

#[test]
fn locations_cannot_put_control_or_programs_in_granted_data() {
    let shipped = include_str!("../../layout.toml");
    for (from, to) in [
        ("safe/installation.toml", "data/installation.toml"),
        ("exec/bin/lnk", "data/lnk"),
        ("Safe/Agents/defaults.toml", "Agents/defaults.toml"),
        ("\"data/agents\"", "\"safe/agents/homes\""),
        ("\"seen.toml\"", "\"../seen.toml\""),
    ] {
        let layout: Layout = toml::from_str(&shipped.replace(from, to)).unwrap();
        assert!(layout.validate().is_err(), "accepted {to}");
    }
    LAYOUT.validate().unwrap();
}

#[test]
fn protected_roots_exist_before_a_child_can_make_them() {
    use std::os::unix::fs::PermissionsExt;
    let user = link_test_kit::TempDir::new("safe-roots");
    let hidden = user.join("hidden");
    let home = user.join("Link");
    std::fs::create_dir(&home).unwrap();
    prepare_at(&hidden, &home).unwrap();
    for folder in [hidden.join("safe"), hidden.join("exec"), home.join("Safe")] {
        assert_eq!(
            std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let elsewhere = user.join("elsewhere");
    std::fs::rename(home.join("Safe"), &elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, home.join("Safe")).unwrap();
    assert!(prepare_at(&hidden, &home).is_err());
}

#[test]
fn preparing_protected_roots_does_not_recreate_a_moved_home() {
    let user = link_test_kit::TempDir::new("moved-home");
    let hidden = user.join("hidden");
    let home = user.join("Link");
    prepare_at(&hidden, &home).unwrap();
    assert!(!home.exists(), "startup must not create an unused Home");
    std::fs::create_dir(&home).unwrap();
    prepare_at(&hidden, &home).unwrap();
    let settings = Home::Safe.at(&home).join("kept.toml");
    std::fs::write(&settings, "owned state").unwrap();
    let moved = user.join("moved");
    std::fs::rename(&home, &moved).unwrap();
    prepare_at(&hidden, &home).unwrap();
    assert!(!home.exists(), "startup must leave the former Home absent");
    assert_eq!(
        std::fs::read_to_string(Home::Safe.at(&moved).join("kept.toml")).unwrap(),
        "owned state"
    );
    assert!(hidden.join("safe").is_dir());
    assert!(hidden.join("exec").is_dir());
}

#[test]
fn the_core_folder_cannot_be_claimed_by_a_module() {
    for invalid in ["link", "../link", "agent/other", "", "Agent", "-agent"] {
        assert!(check_owner(invalid).is_err(), "{invalid}");
    }
    for owner in ["agent", "sandbox", "file-store"] {
        check_owner(owner).unwrap();
    }
}

/// Each owner's folder is its plural word at the same path in every part,
/// and every path has its twin in the others or is declared one-sided.
#[test]
fn every_owner_has_one_folder_in_each_part_it_declares() {
    let shipped = include_str!("../../layout.toml");
    assert_eq!(Home::AgentSettings.relative(), "Safe/Agents");
    assert_eq!(Hidden::Agents.relative(), "safe/agents");
    assert_eq!(Hidden::AgentData.relative(), "data/agents");
    assert_eq!(Home::Agents.relative(), "Agents");
    assert_eq!(Hidden::Bin.relative(), "exec/bin");
    for (from, to) in [
        // An agent's folder named otherwise in one part.
        (
            "path = \"safe/agents/journal\"",
            "path = \"safe/journals/journal\"",
        ),
        // A folder in a part the owner doesn't declare.
        (
            "path = \"safe/secrets/secrets.toml\"",
            "path = \"data/secrets/secrets.toml\"",
        ),
        // A declared part with no folder: the twin is missing.
        (
            "module = \"secret\", parts = [\"safe\"]",
            "module = \"secret\", parts = [\"safe\", \"data\"]",
        ),
        // Link's own in an owner's folder.
        (
            "path = \"safe/releases/update.json\"",
            "path = \"safe/agents/update.json\"",
        ),
        // A folder directly in ~/Link is Title Case...
        (
            "path = \"Teams\", kind = \"data\"",
            "path = \"teams\", kind = \"data\"",
        ),
        // ...as one directly in its mirror ~/Link/Safe is.
        (
            "path = \"Safe/Models/models.toml\"",
            "path = \"Safe/models/models.toml\"",
        ),
        (
            "path = \"Safe/Sandboxes/presets\"",
            "path = \"Safe/sandboxes/presets\"",
        ),
        // An owner that isn't declared.
        ("owner = \"measures\"", "owner = \"measure\""),
    ] {
        assert!(shipped.contains(from), "{from}");
        let layout: Layout = toml::from_str(&shipped.replacen(from, to, 1)).unwrap();
        assert!(layout.validate().is_err(), "accepted {to}");
    }
    assert!(reserved("agents").contains("journal"));
    assert!(reserved("agents").contains("defaults.toml"));
    assert!(!reserved("agents").contains("main"));
}

#[test]
fn state_a_catalogue_doesnt_name_is_closed_but_its_data() {
    let dir = link_test_kit::TempDir::new("system-state");
    let root = dir.join("state");
    for name in ["spare", "cache", "extra"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
    let data = [root.join("cache")];
    assert_eq!(
        system::unnamed(std::slice::from_ref(&root), &data),
        [root.join("extra"), root.join("spare")]
    );
    // A root not there closes nothing more.
    assert!(system::unnamed(&[dir.join("missing")], &data).is_empty());
}

/// Where a thing's state isn't yet one folder of its own under `~/Link`,
/// named for the item that removes it: the list shrinks, never grows.
const NOT_YET_STATE: &[(&str, Part, &str)] = &[
    // A team's settings are a file each, `Safe/Teams/<team>.toml`, not a
    // folder each, until teams are archive owners.
    ("teams", Part::LinkSafe, "Safe/Teams"),
];

/// State kept in `~/.lnk` instead of `~/Link`, until its owner moves it.
const STATE_IN_HIDDEN: &[(&str, &str)] = &[
    // A box's spec, in one file for every box, until boxes are archive
    // owners.
    ("boxes", "safe/boxes"),
];

/// Every catalogued location is either state, under `~/Link`, or what a
/// thing needs to run, in `~/.lnk`: each owner's folders under `~/Link`
/// are its state, no catalogued location there holds programs or is named
/// for credentials, and the secret module has no folder there. What's
/// inside a thing's folder is its owner's to keep to the rule, as an
/// agent's `archive.check` refuses an unknown control file. Each
/// exception is still true, so it leaves the list when it's fixed.
#[test]
fn only_state_is_under_link() {
    for (word, owner) in owners() {
        for part in &owner.parts {
            let excepted = NOT_YET_STATE.iter().any(|(w, p, _)| w == word && p == part);
            let visible = matches!(part, Part::Link | Part::LinkSafe | Part::Archive);
            if visible && !excepted {
                assert!(
                    owner.state.contains(part),
                    "{word}'s folder in {part:?} is under ~/Link, so it's state"
                );
            }
        }
    }
    for (word, part, path) in NOT_YET_STATE {
        assert!(
            owners()[*word].parts.contains(part) && !owners()[*word].state.contains(part),
            "{word}'s {part:?} is fixed: take it off the list"
        );
        assert!(
            LAYOUT
                .home
                .values()
                .any(|l| l.path == *path && l.owner == *word),
            "~/Link/{path} is gone: take it off the list"
        );
    }
    for (word, path) in STATE_IN_HIDDEN {
        assert!(
            LAYOUT
                .hidden
                .values()
                .any(|l| l.path == *path && l.owner == *word),
            "~/.lnk/{path} is gone: take it off the list"
        );
    }
    for (key, location) in &LAYOUT.home {
        assert!(
            location.kind != Kind::Exec,
            "~/Link/{} ({key:?}) holds programs, which belong in ~/.lnk/exec",
            location.path
        );
    }
    // Credentials are the secret module's, and agents', in ~/.lnk/safe.
    assert!(owners()["secrets"].parts.iter().all(|part| !part.in_link()));
    for (key, location) in &LAYOUT.home {
        assert!(
            !["credential", "secret", "token"]
                .iter()
                .any(|word| location.path.to_ascii_lowercase().contains(word)),
            "~/Link/{} ({key:?}) holds credentials, which belong in ~/.lnk/safe",
            location.path
        );
    }
}

#[test]
fn an_owners_state_is_only_under_link_and_its_kind_is_its_own() {
    let shipped = include_str!("../../layout.toml");
    for (from, to) in [
        // State in ~/.lnk.
        (
            "state = [\"link\", \"link-safe\"] }",
            "state = [\"link\", \"safe\"] }",
        ),
        // State in a part it has no folder in.
        ("state = [\"link-safe\"] }", "state = [\"link\"] }"),
        // A kind two owners share.
        ("kind = \"team\"", "kind = \"agent\""),
        // A kind with no state.
        (
            "kind = \"box\", parts = [\"link\", \"safe\"], state = [\"link\"]",
            "kind = \"box\", parts = [\"link\", \"safe\"]",
        ),
        // A kind with the selection's separator.
        ("kind = \"box\"", "kind = \"b.x\""),
    ] {
        assert!(shipped.contains(from), "{from}");
        let layout: Layout = toml::from_str(&shipped.replacen(from, to, 1)).unwrap();
        assert!(layout.validate().is_err(), "accepted {to}");
    }
    assert_eq!(owner_of_kind("agent").map(|(word, _)| word), Some("agents"));
    assert!(owner_of_kind("agents").is_none());
}

#[test]
fn an_agents_state_is_its_two_folders_under_link() {
    assert_eq!(
        state_locations("agents"),
        [
            (Part::Link, Home::Agents),
            (Part::LinkSafe, Home::AgentSettings)
        ]
    );
    assert_eq!(state_locations("boxes"), [(Part::Link, Home::Boxes)]);
    assert!(state_locations("secrets").is_empty());
}

/// A temporary name is drawn at random, never this process's ID and a
/// count alone: a PID repeats in every PID namespace, so two sandboxes
/// writing one folder would share names and write into each other's
/// files.
#[test]
fn a_temporary_name_is_random_whatever_the_process() {
    let names: Vec<String> = (0..64).map(|_| temporary_name()).collect();
    for name in &names {
        assert!(is_temporary(name), "{name}");
        let random = name
            .strip_prefix(&LAYOUT.temporary.prefix)
            .and_then(|rest| rest.split('-').next())
            .unwrap_or_default();
        assert!(link_id::is_candidate(random), "{name}");
    }
    let randoms: std::collections::BTreeSet<&str> = names
        .iter()
        .filter_map(|n| n.strip_prefix(&LAYOUT.temporary.prefix)?.split('-').next())
        .collect();
    assert_eq!(randoms.len(), names.len());
}

/// The longest a socket's path may be: macOS's `sun_path` holds 104 bytes
/// with its ending zero, Linux's 108.
const SOCKET_MOST: usize = 103;

/// A storage bridge's socket for memory fits a socket's path, with the
/// longest owner's name and bridge key, in an ordinary runtime
/// folder and in a long home's fallback (`<home>.run` under `~/.cache`): the
/// owner is named by its digest, never in full.
#[test]
fn a_memory_bridge_socket_fits_whatever_its_owner() {
    let owner = format!("{}@{}", "o".repeat(40), "m".repeat(23));
    assert_eq!(owner.len(), 64, "the longest owner's name");
    let key = "a".repeat(link_id::limits().candidate_length);
    // The system's runtime folder, the fallback without one, and a long
    // home's, each for the largest user ID.
    let fallback = LAYOUT.runtime_root.fallback_for(u32::MAX);
    for root in [
        Path::new("/run/user/4294967295/lnk"),
        fallback.as_path(),
        Path::new("/home/lnk-ci-10/.cache/ci-home.XXXXXX.run/lnk"),
    ] {
        let socket = MemoryBridges::in_runtime(root, &owner).socket(&key);
        assert!(socket.starts_with(Runtime::MemorySockets.at(root)));
        assert!(
            socket.as_os_str().len() <= SOCKET_MOST,
            "{} is {} bytes",
            socket.display(),
            socket.as_os_str().len()
        );
    }
    // Two owners, two folders; one owner, one, wherever it's asked.
    let root = Path::new("/run/user/1000/lnk");
    let mine = MemoryBridges::in_runtime(root, "me@here");
    assert_eq!(mine, MemoryBridges::in_runtime(root, "me@here"));
    assert_ne!(mine, MemoryBridges::in_runtime(root, "you@here"));
    assert_eq!(
        MemoryBridges::at(mine.folder().to_path_buf()).socket("main"),
        mine.socket("main")
    );
}

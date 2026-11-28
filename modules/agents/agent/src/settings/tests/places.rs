//! Ports, files folders, the sandbox's default and the harness's module,
//! as each agent here gets them.

use super::*;

#[test]
fn local_ports() {
    assert_eq!(local_port("http://localhost:11434"), Some(11434));
    assert_eq!(local_port("http://127.0.0.1:1234/v1"), Some(1234));
    assert_eq!(local_port("http://[::1]:8080"), Some(8080));
    assert_eq!(local_port("http://localhost"), Some(80));
    assert_eq!(local_port("https://api.openai.com"), None);
    assert_eq!(local_port("nonsense"), None);
}

#[test]
fn files_folders_that_are_a_way_out_are_refused() {
    if !own_config() {
        return;
    }
    let home = link_files::home_dir();
    for bad in [
        home.clone(),
        "/".into(),
        home.join(".ssh"),
        home.join(".config"),
        home.join(".local/bin/agent"),
        home.join("Library/LaunchAgents"),
        home.join("Library"),
        link_files::installation_dir(),
    ] {
        assert!(unsafe_files(&bad).is_some(), "{} is allowed", bad.display());
    }
    for good in [
        home.join("Link/Agents/main/home"),
        home.join("Documents/agent"),
    ] {
        assert_eq!(unsafe_files(&good), None, "{}", good.display());
    }
}

/// Of the agent's folder, only its Home and what's in it: never its
/// own folder (its scheduler's jobs, its memory's index), nor another
/// agent's.
#[test]
fn a_files_folder_keeps_out_of_what_link_keeps() {
    if !own_config() {
        return;
    }
    let agent = Settings::new("main");
    for bad in [
        agent.dir().join("agent"),
        agent.dir().join("agent/memory/link"),
        agents().join("other/home"),
    ] {
        assert!(
            agent.others_folder(&bad).is_some(),
            "{} is allowed",
            bad.display()
        );
    }
    for good in [agent.home(), agent.home().join("notes")] {
        assert_eq!(agent.others_folder(&good), None, "{}", good.display());
    }
}

/// Each agent's block, `main`'s too, is apart from every other agent's
/// here, at a random place from the machine's first port and below the
/// system's range for outgoing connections: two users of the machine
/// making agents at once, each allocating from 20000, rarely pick the
/// same, where the first free block would be the same for both.
#[test]
fn each_agent_gets_a_block_of_ports_of_its_own() {
    let others = [(FIRST_PORT, 200), (FIRST_PORT + 400, 2)];
    let blocks: std::collections::BTreeSet<u16> = (0..20)
        .map(|_| random_free_block(FIRST_PORT, &others, 2).unwrap())
        .collect();
    assert!(blocks.len() > 10, "{blocks:?}");
    for &base in &blocks {
        assert!(fits(FIRST_PORT, base, 2, &others), "{base}");
        assert!(base + 2 <= EPHEMERAL, "{base}");
    }
    // A block set by hand, not on the grid, still counts.
    assert!(!fits(FIRST_PORT, 20199, 2, &[(20000, 200)]));
    assert!(fits(FIRST_PORT, 20200, 2, &[(20000, 200)]));
    assert!(!fits(FIRST_PORT, 65535, 2, &[]));
    assert!(!fits(FIRST_PORT, 1024, 2, &[]));
}

/// A random step through the candidates visits each once, from any
/// start, however many there are.
#[test]
fn a_random_step_visits_every_place_once() {
    for count in [1, 2, 6, 12_768] {
        for seed in [0, 1, 5, 99_999] {
            let step = coprime(seed, count);
            let seen: std::collections::BTreeSet<u64> =
                (0..count).map(|k| (3 + k * step) % count).collect();
            assert_eq!(seen.len() as u64, count, "{count} {seed} {step}");
        }
    }
}

/// A block another program holds a port of, another user's agent say,
/// is passed over, as another agent's is.
#[test]
fn a_block_another_program_holds_is_passed_over() {
    let held = link_test_kit::Held::loopback();
    let port = held.port();
    // Blocks of one from the held port up, the held one among them: it's
    // never the one taken.
    for _ in 0..20 {
        let base = random_free_block(port, &[], 1).unwrap();
        assert_ne!(base, port);
        assert!(base > port, "{port} {base}");
    }
}

#[test]
fn a_harness_says_how_many_ports_it_takes() {
    let mut contract = link_harness_contract::harness::Contract::new(true);
    assert_eq!(block_for(&contract), PORT_BLOCK);
    contract.ports = Some(1);
    assert_eq!(block_for(&contract), 2);
    contract.ports = Some(0);
    assert_eq!(block_for(&contract), 2);
    contract.ports = Some(u16::MAX);
    assert_eq!(block_for(&contract), BLOCK_MAX);
}

#[test]
fn the_sandbox_is_on_unless_saved_off() {
    let settings: Settings = toml::from_str("harness = \"my-harness\"").unwrap();
    assert!(settings.sandbox);
    assert_eq!(settings.harness(), "my-harness");
    assert_eq!(Settings::default().harness(), DEFAULT_HARNESS);
}

#[test]
fn no_sandbox_lasts_until_sandbox_is_turned_back_on() {
    if !own_config() {
        return;
    }
    if !link_test_kit::gate::settings(&crate::machine_settings::path().unwrap()) {
        return;
    }
    let path = Settings::path_of("main");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut agent = blank("main");
    agent.harness = Some("my-harness".into());
    agent.sandbox = false;
    agent.save().unwrap();
    assert!(!Settings::load_without_keys("main").unwrap().sandbox);
    assert!(!Settings::load_without_keys("main").unwrap().sandbox);
    // On again: kept on.
    let mut agent = Settings::load_without_keys("main").unwrap();
    agent.sandbox = true;
    agent.save().unwrap();
    assert!(Settings::load_without_keys("main").unwrap().sandbox);
}

#[test]
fn a_typed_name_finds_its_module() {
    assert_eq!(module_of("link"), "harness");
    assert_eq!(module_of("my-harness"), "my-harness");
    assert_eq!(module_of("mine"), "mine");
}

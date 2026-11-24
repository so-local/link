//! Link's own tools an agent is given: `run` and `file` from its sandbox
//! file, and the browser and desktop it adds.

use super::*;

/// A harness that keeps to the layout gets `run` and `file`, each call
/// in a sandbox made from the agent's sandbox file: the files folder
/// read, `[files] write` written, `[commands]` this machine has; its
/// environment names those programs. `[commands] allow = []` takes
/// `run` away, and a harness that doesn't keep to the layout has
/// neither.
#[test]
fn run_and_file_are_made_from_the_agents_sandbox_file() {
    let world = World::new("run");
    if world.skips() {
        return;
    }
    let spec = world.everything_on();
    ok(&world.start(
        "main",
        &["create", "main", "--harness", "tidy", "--spec", &spec],
    ));
    let files = world.home.join("Link/Agents/main/home");
    let pair = |flag: &str, value: &Path| [flag.to_string(), value.display().to_string()];
    let run = defined(&world, "run-").expect("run is defined");
    let at = run.iter().position(|a| a == "--").unwrap();
    let (serve, each) = (&run[..at], &run[at + 1..]);
    assert_eq!(serve[1], "serve");
    assert!(serve.windows(2).any(|w| w == ["--program", "*"]), "{run:?}");
    assert!(!serve.iter().any(|a| a == "no-such-program"), "{run:?}");
    assert_eq!(each[1..3], ["sandbox", "run"]);
    for (flag, folder) in [
        ("--files.read", &files),
        ("--files.write", &files.join("work")),
    ] {
        assert!(each.windows(2).any(|w| w == pair(flag, folder)), "{run:?}");
    }
    assert!(
        each.windows(2).any(|w| w == ["--internet.hosts", "*"]),
        "{run:?}"
    );
    assert!(
        each.windows(2).any(|w| w == ["--commands.allow", "*"]),
        "{run:?}"
    );
    // Its own folder's instructions and skills: written, as Link's
    // sandbox file grants; its conversations hidden from every call.
    let own = world.home.join("Link/Agents/main/agent");
    assert!(
        each.windows(2)
            .any(|w| w == pair("--files.hide", &own.join("chats"))),
        "{run:?}"
    );
    for name in ["instructions", "skills"] {
        assert!(
            each.windows(2)
                .any(|w| w == pair("--files.write", &own.join(name))),
            "{run:?}"
        );
        assert!(
            !each
                .windows(2)
                .any(|w| w == pair("--files.read", &own.join(name))),
            "{run:?}"
        );
    }
    // Background commands: kept in its data folder, told through its
    // bridge's way in, to the caps of Link's spec.
    let state = world.home.join(".lnk/data/agents/main/run");
    assert!(
        serve.windows(2).any(|w| w == pair("--state", &state)),
        "{run:?}"
    );
    for pair in [
        ["--background", "4"],
        ["--background-timeout", "1h"],
        ["--background-output", "16777216"],
        ["--background-keep", "1d"],
        ["--background-per-day", "100"],
        ["--hops", "8"],
    ] {
        assert!(serve.windows(2).any(|w| w == pair), "{pair:?} in {run:?}");
    }
    assert!(
        serve
            .windows(2)
            .any(|w| w[0] == "--way-in" && w[1].ends_with("/agents/main/way-in.sock")),
        "{run:?}"
    );
    let file = defined(&world, "file-").expect("file is defined");
    assert!(!file.iter().any(|a| a == "--internet.hosts"), "{file:?}");
    assert!(
        file.windows(2).any(|w| w == ["--write", "work"]),
        "{file:?}"
    );
    // Its environment names what it may run.
    let configured = read(
        &world
            .home
            .join(".lnk/data/agents/main/harnesses/tidy/configure.in"),
    );
    let configured: serde_json::Value = serde_json::from_str(&configured).unwrap();
    let commands = configured["environment"]["commands"].as_array().unwrap();
    assert!(commands.iter().any(|c| c == "*"), "{configured}");

    // No program here: no run, and none named; file stays.
    let file = world.root.join("sandbox.toml");
    std::fs::write(
        &file,
        "schema = 1\n[commands]\nallow = [\"no-such-program\"]\n\n[files]\nwrite = [\"notes\"]\n",
    )
    .unwrap();
    let file = file.display().to_string();
    ok(&world.start("main", &["start", "--sandbox", &file]));
    assert!(defined(&world, "run-").is_none());
    let file = defined(&world, "file-").expect("file is defined");
    assert!(
        file.windows(2).any(|w| w == ["--write", "notes"]),
        "{file:?}"
    );
    assert!(files.join("notes").is_dir());
    let configured = read(
        &world
            .home
            .join(".lnk/data/agents/main/harnesses/tidy/configure.in"),
    );
    let configured: serde_json::Value = serde_json::from_str(&configured).unwrap();
    assert!(
        configured["environment"].get("commands").is_none(),
        "{configured}"
    );

    // A harness of its own files: neither. It honors none of the tools'
    // features, so a switch is refused while its spec turns them on, and
    // goes once they're removed.
    let refused = world.lnk(&["harness", "set", "plain", "--yes", "--foreground"]);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("which plain doesn't honor"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    for tool in ["run", "file", "code"] {
        ok(&world.lnk(&["-a", "main", "tool", "remove", tool]));
    }
    ok(&world.lnk(&["harness", "set", "plain", "--yes", "--foreground"]));
    assert!(defined(&world, "run-").is_none() && defined(&world, "file-").is_none());
}

/// `lnk agent tool add browser` defines the browser for the agent alone,
/// shared by its sessions, its profile in the agent's folder and the
/// browser in `lnk sandbox run`; its harness is handed it as `browser`,
/// and `remove` takes it away, the profile kept.
#[test]
fn a_browser_is_the_agents_own() {
    let world = World::new("browser");
    if world.skips() {
        return;
    }
    ok(&world.start("main", &["create", "main", "--harness", "tidy"]));
    assert!(defined(&world, "browser-").is_none());
    ok(&world.lnk(&["-a", "main", "tool", "add", "browser"]));
    let browser = defined(&world, "browser-").expect("the browser is defined");
    let profile = world.home.join(".lnk/data/agents/main/browser");
    let at = browser.iter().position(|a| a == "--").unwrap();
    // It's handed the agent's floor and its sandbox file's internet
    // hosts, everything for an agent that names `*`.
    assert_eq!(
        browser[1..at],
        [
            "serve".to_string(),
            "--floor".into(),
            "owner".into(),
            "--internet-host".into(),
            "*".into(),
            "--profile".into(),
            profile.display().to_string()
        ]
    );
    assert_eq!(browser[at + 2..], ["sandbox", "run"]);
    assert!(profile.is_dir());
    let tools = world.home.join(".lnk/safe/sandboxes/tools");
    let definition = std::fs::read_dir(&tools)
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("browser-"))
        .unwrap();
    let definition: serde_json::Value = serde_json::from_str(&read(&definition.path())).unwrap();
    assert_eq!(definition["shared"], true, "{definition}");
    let listed = ok(&world.lnk(&["-a", "main", "tool", "list"]));
    assert!(
        listed
            .lines()
            .any(|l| l.trim_start().starts_with("browser") && l.contains(" on ")),
        "{listed}"
    );
    // With `--json`, its granted namespaces instead, typed.
    let granted = ok(&world.lnk(&["-a", "main", "tool", "list", "--json"]));
    let granted: serde_json::Value = serde_json::from_str(&granted).unwrap();
    assert!(granted["namespaces"].is_array(), "{granted}");
    // Handed to its harness by its plain name.
    ok(&world.start("main", &["start"]));
    let configured = read(
        &world
            .home
            .join(".lnk/data/agents/main/harnesses/tidy/configure.in"),
    );
    let configured: serde_json::Value = serde_json::from_str(&configured).unwrap();
    let handed = configured["actions"]["urls"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    assert!(handed.contains_key("browser"), "{configured}");
    assert!(
        !handed.keys().any(|k| k.starts_with("browser-")),
        "{configured}"
    );

    ok(&world.lnk(&["-a", "main", "tool", "remove", "browser"]));
    assert!(defined(&world, "browser-").is_none());
    assert!(profile.is_dir());
}

/// `lnk agent tool add desktop` gives the agent a display of its own,
/// its number kept; each program `run` starts is told it and reads its
/// socket and cookie alone, the variables handed in the sandbox's context.
#[cfg(target_os = "linux")]
#[test]
fn a_desktop_is_the_agents_own_and_its_programs_show_there() {
    let world = World::new("desktop");
    if world.skips() {
        return;
    }
    let spec = world.everything_on();
    ok(&world.start(
        "main",
        &["create", "main", "--harness", "tidy", "--spec", &spec],
    ));
    ok(&world.lnk(&["-a", "main", "tool", "add", "desktop"]));
    let dir = world.home.join(".lnk/data/agents/main/desktop");
    let display = read(&dir.join("display")).trim().to_string();
    let n: u32 = display.parse().unwrap();
    assert!(n >= 100, "{n}");
    let auth = dir.join("auth").display().to_string();
    let desktop = defined(&world, "desktop-").expect("the desktop is defined");
    // Xvfb runs in a sandbox of its own, after `--`.
    assert_eq!(
        desktop[1..6],
        ["serve", "--display", &display, "--xauthority", &auth]
    );
    // Its X server's sandbox, which names its programs itself.
    assert_eq!(desktop[6..].first().map(String::as_str), Some("--"));
    assert_eq!(desktop[8..], ["sandbox", "run"]);
    // Its next start: run's programs are told the display.
    ok(&world.start("main", &["start"]));
    let run = defined(&world, "run-").expect("run is defined");
    let at = run.iter().position(|a| a == "--").unwrap();
    let (serve, each) = (&run[..at], &run[at + 1..]);
    let named = format!("DISPLAY=:{n}");
    assert!(serve.windows(2).any(|w| w == ["--var", &named]), "{run:?}");
    let cookie = format!("XAUTHORITY={auth}");
    assert!(serve.windows(2).any(|w| w == ["--var", &cookie]), "{run:?}");
    let socket = format!("/tmp/.X11-unix/X{n}");
    assert!(
        each.windows(2).any(|w| w == ["--files.read", &socket]),
        "{run:?}"
    );
    assert!(
        each.windows(2).any(|w| w == ["--files.read", &auth]),
        "{run:?}"
    );
    assert!(!each.iter().any(|a| a == "/tmp/.X11-unix"), "{run:?}");
    // Its number is kept: defined again, the same.
    ok(&world.lnk(&["-a", "main", "tool", "add", "desktop"]));
    assert_eq!(read(&dir.join("display")).trim(), display);
    ok(&world.lnk(&["-a", "main", "tool", "remove", "desktop"]));
    assert!(defined(&world, "desktop-").is_none());
    ok(&world.start("main", &["start"]));
    let run = defined(&world, "run-").expect("run is defined");
    assert!(!run.iter().any(|a| a.starts_with("DISPLAY=")), "{run:?}");
}

/// A solo agent's scheduler tool acts as the agent (`--as`), as its
/// runner fires, so a schedule's firings and retries keep one
/// identity; it's told where a schedule's answer may go too.
#[test]
fn a_solo_agents_scheduler_acts_as_the_agent() {
    let world = World::new("scheduler-as");
    if world.skips() {
        return;
    }
    let spec = world.root.join("scheduler-on.toml");
    std::fs::write(
        &spec,
        "schema = 1\nextends = \"link\"\n[tool.scheduler]\nenabled = true\n",
    )
    .unwrap();
    ok(&world.start(
        "main",
        &[
            "create",
            "main",
            "--harness",
            "tidy",
            "--spec",
            &spec.display().to_string(),
        ],
    ));
    let scheduler = defined(&world, "scheduler-").expect("the scheduler is defined");
    let at = scheduler
        .iter()
        .position(|a| a == "--as")
        .unwrap_or_else(|| panic!("no --as: {scheduler:?}"));
    let id = &scheduler[at + 1];
    assert!(link_machine::machine::check_id(id).is_ok(), "{scheduler:?}");
    assert!(
        defined(&world, &format!("scheduler-{id}")).is_some(),
        "acts as the agent whose tool it is: {scheduler:?}"
    );
}

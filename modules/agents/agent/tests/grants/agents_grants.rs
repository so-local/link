//! What an agent is granted: its own, following it, never widened by a
//! deny, and its sandbox turned off only where the machine allows.

use super::*;

/// What you grant an agent is its own, in its sandbox file: another agent
/// on the same harness doesn't have it, a harness that needs what it has
/// asks nothing, and `--sandbox-file` writes it whole.
#[test]
fn an_agents_grants_are_its_own_and_follow_it() {
    // `--sandbox-file` is a sandbox's: it needs bubblewrap.
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    let world = World::new("own");
    ok(&world.start("a", &["create", "a", "--harness", "plain"]));
    ok(&world.start("b", &["create", "b", "--harness", "plain"]));
    let allowed = ok(&world.lnk(&["-a", "a", "permission", "add", "--files.read", "~"]));
    assert!(
        allowed.contains("The grants of a now include --files.read '~'"),
        "{allowed}"
    );
    ok(&world.lnk(&[
        "-a",
        "a",
        "permission",
        "add",
        "--internet.tcp",
        "github.com:22",
    ]));
    ok(&world.lnk(&[
        "-a",
        "a",
        "permission",
        "add",
        "--internet.per_minute",
        "600",
    ]));
    let own = read(&world.sandbox("a"));
    for granted in ["\"~\"", "github.com:22", "per_minute = 600"] {
        assert!(own.contains(granted), "{granted}: {own}");
    }
    assert_eq!(
        status(&world, "a")["beyond_defaults"],
        serde_json::json!(["--files.read '~'", "--internet.tcp github.com:22"])
    );
    assert_eq!(
        status(&world, "b")["beyond_defaults"],
        serde_json::json!([])
    );
    assert!(!world.grants("plain").exists(), "the harness's changed");
    let again = ok(&world.lnk(&["-a", "a", "permission", "add", "--files.read", "~"]));
    assert!(again.contains("a has --files.read '~' already"), "{again}");
    // A permission's old name is no flag: refused.
    let old = failed(&world.lnk(&["-a", "a", "permission", "add", "read-home"]));
    assert!(old.contains("unexpected argument"), "{old}");
    let shown = ok(&world.lnk(&["-a", "a", "show", "--permissions"]));
    assert!(
        shown.contains("allowed, beyond the defaults") && shown.contains("yours"),
        "{shown}"
    );

    // A harness that needs what the agent has asks nothing; for an agent
    // that doesn't have it, it asks. (`use` runs it sandboxed.)
    world.needs(r#"{"needs": [{"spec": {"files": {"read": ["~"]}}, "why": "it reads"}]}"#);
    if world.sandboxed {
        let started = ok(&world.lnk(&["-a", "a", "harness", "set", "needy", "--foreground"]));
        assert!(!started.contains("asks to do more"), "{started}");
        let refused = failed(&world.lnk(&["-a", "b", "harness", "set", "needy", "--foreground"]));
        assert!(refused.contains("--files.read '~'"), "{refused}");
    }

    // What isn't an agent's: no sandbox at all, or a risk's name.
    let refused = failed(&world.lnk(&["-a", "a", "permission", "add", "unsandboxed"]));
    assert!(refused.contains("unexpected argument"), "{refused}");
    let refused =
        failed(&world.lnk(&["harness-permission", "remove", "host", "--harness", "plain"]));
    assert!(refused.contains("isn't a risk's name"), "{refused}");
    let refused = failed(&world.lnk(&["-a", "a", "permission", "add"]));
    assert!(refused.contains("takes a spec's flags"), "{refused}");

    // A harness is one here, and never an agent command's name.
    let nosuch = failed(&world.lnk(&["harness-permission", "list", "--harness", "nosuch"]));
    assert!(nosuch.contains("no command or harness"), "{nosuch}");
    let module = failed(&world.lnk(&[
        "harness-permission",
        "remove",
        "bridge",
        "--harness",
        "harness",
    ]));
    assert!(module.contains("the module of harness link"), "{module}");
    assert!(!world.grants("harness").exists());
    let command = failed(&world.lnk(&["-a", "a", "harness", "set", "show"]));
    assert!(command.contains("is an lnk agent command"), "{command}");

    // An agent running in a terminal isn't restarted: said.
    let terminal = world.home.join(".lnk/safe/agents/a");
    let lock = std::fs::File::create(terminal.join("foreground.lock")).unwrap();
    lock.lock().unwrap();
    let denied = ok(&world.lnk(&["-a", "a", "permission", "remove", "--files.read", "~"]));
    drop(lock);
    assert!(
        denied.contains("The grants of a no longer include --files.read '~'")
            && denied.contains("a runs in a terminal"),
        "{denied}"
    );

    // A file that can't be read stops none of the commands, and deny
    // writes it anew, never wider than nothing.
    std::fs::write(world.sandbox("a"), "permissions = 7\n").unwrap();
    ok(&world.lnk(&["-a", "a", "show", "--permissions"]));
    let denied = ok(&world.lnk(&["-a", "a", "permission", "remove", "--files.read", "~"]));
    assert!(denied.contains("a's grants can't be read"), "{denied}");
    let anew = read(&world.sandbox("a"));
    assert!(
        anew.contains("What a may do") && !anew.contains("\n["),
        "{anew}"
    );

    // `--sandbox`: a file read strictly, kept whole.
    let file = world.root.join("sandbox.toml");
    std::fs::write(&file, "schema = 1\nunsandboxed = true\n").unwrap();
    let refused = failed(&world.start(
        "b",
        &["-a", "b", "start", "--sandbox", file.to_str().unwrap()],
    ));
    assert!(refused.contains("unsandboxed"), "{refused}");
    std::fs::write(
        &file,
        "schema = 1\n[network]\nmode = \"host\"\n[internet]\nhosts = [\"*\"]\n",
    )
    .unwrap();
    let kept = ok(&world.start(
        "b",
        &["-a", "b", "start", "--sandbox", file.to_str().unwrap()],
    ));
    assert!(
        kept.contains("Its grants: --network.mode host; --internet.hosts '*'."),
        "{kept}"
    );
    assert_eq!(
        status(&world, "b")["beyond_defaults"],
        serde_json::json!(["--network.mode host"])
    );
}

/// `lnk agent permission remove` never widens what an agent may do: a limit lifted,
/// one of any (`"*"`) taken, or an OS's table emptied so that OS falls
/// back to the section's own keys is refused, the file as it was; and
/// `allow` beside any (`"*"`) keeps it alone.
#[test]
fn a_deny_never_widens() {
    let world = World::new("widens");
    ok(&world.start("a", &["create", "a", "--harness", "plain"]));
    ok(&world.lnk(&["-a", "a", "permission", "add", "--internet.hosts", "*"]));
    ok(&world.lnk(&[
        "-a",
        "a",
        "permission",
        "add",
        "--internet.per_minute",
        "60",
    ]));
    let kept = read(&world.sandbox("a"));
    for args in [
        &["--internet.per_minute", "60"][..],
        &["--internet.hosts", "api.openai.com"],
    ] {
        let refused =
            failed(&world.lnk(&[&["-a", "a", "permission", "remove"][..], args].concat()));
        assert!(
            refused.contains("a deny never widens"),
            "{args:?}: {refused}"
        );
        assert_eq!(read(&world.sandbox("a")), kept, "{args:?}");
    }
    // Any absorbs what's allowed beside it: the file still starts.
    ok(&world.lnk(&[
        "-a",
        "a",
        "permission",
        "add",
        "--internet.hosts",
        "api.openai.com",
    ]));
    let any = read(&world.sandbox("a"));
    assert!(
        any.contains("hosts = [\"*\"]") && !any.contains("api.openai.com"),
        "{any}"
    );
    // An OS's table, each OS's alike.
    for (file, args) in [
        (
            "schema = 1\n[internet]\nhosts = [\"*\"]\n[internet.linux]\nhosts = [\"a.com\"]\n\
             [internet.macos]\nhosts = [\"a.com\"]\n",
            &["--internet.hosts", "a.com"][..],
        ),
        (
            "schema = 1\n[commands]\nallow = [\"sh\", \"git\"]\n[commands.linux]\nallow = [\"git\"]\n",
            &["--commands.allow", "git"],
        ),
        (
            "schema = 1\n[commands]\nallow = [\"sh\", \"git\"]\n[commands.macos]\nallow = [\"git\"]\n",
            &["--commands.allow", "git"],
        ),
    ] {
        std::fs::write(world.sandbox("a"), file).unwrap();
        let refused =
            failed(&world.lnk(&[&["-a", "a", "permission", "remove"][..], args].concat()));
        assert!(refused.contains("a deny never widens"), "{file}: {refused}");
        assert_eq!(read(&world.sandbox("a")), file);
    }
}

/// A script turns the sandbox off only by naming it, `--no-sandbox`;
/// `--yes` answers the rest, never that. A machine whose settings forbid
/// it (`unsandboxed = false` in `agents.toml`, a deployment's) refuses
/// it, and then a harness that needs no sandbox doesn't run.
#[test]
fn a_script_turns_the_sandbox_off_by_name_unless_the_machine_forbids_it() {
    let world = World::new("unsandboxed");
    if world.skips() {
        return;
    }
    world.needs(r#"{"unsandboxed": {"why": "it runs ps, which is setuid"}}"#);
    let refused = failed(&world.start("main", &["create", "main", "--harness", "needy", "--yes"]));
    assert!(refused.contains("--no-sandbox"), "{refused}");
    let off = ok(&world.start("main", &["start", "--no-sandbox"]));
    assert!(off.contains("There is no sandbox."), "{off}");

    // Never both, and no sandbox is never a grant, of a harness's or an
    // agent's.
    let both = failed(&world.lnk(&["start", "--no-sandbox", "--sandbox"]));
    assert!(both.contains("cannot be used with"), "{both}");
    for args in [
        &[
            "harness-permission",
            "remove",
            "unsandboxed",
            "--harness",
            "needy",
        ][..],
        &[
            "harness-permission",
            "add",
            "unsandboxed",
            "--harness",
            "needy",
        ],
    ] {
        let refused = failed(&world.lnk(args));
        assert!(refused.contains("never a grant"), "{args:?}: {refused}");
    }
    for args in [
        &["permission", "add", "unsandboxed"][..],
        &["permission", "remove", "unsandboxed"],
    ] {
        let refused = failed(&world.lnk(args));
        assert!(
            refused.contains("unexpected argument"),
            "{args:?}: {refused}"
        );
    }

    // Another need still takes --yes.
    world.needs(
        r#"{"needs": [{"spec": {"network": {"mode": "host"}}, "why": "its websockets"}],
            "unsandboxed": {"why": "it runs ps"}}"#,
    );
    let refused = failed(&world.start("main", &["start"]));
    assert!(refused.contains("add --yes"), "{refused}");
    let both = ok(&world.start("main", &["start", "--yes"]));
    assert!(both.contains("Allowed by --yes."), "{both}");

    // Forbidden on this machine: it doesn't run without a sandbox, and
    // it can't run in one.
    let machine = world.home.join(".lnk/safe/agents/policy.toml");
    // The machine's identity is its own file's, never the agent policy's.
    let identity = read(&link_files::layout::Hidden::Installation.at(&world.home.join(".lnk")));
    assert!(identity.contains("id = "), "{identity}");
    let settings = read(&machine);
    assert!(!settings.contains("id = "), "{settings}");
    crate::common::add_policy(&world.home, "unsandboxed = false\n");
    for args in [&["start"][..], &["start", "--yes", "--no-sandbox"]] {
        let refused = failed(&world.start("main", args));
        assert!(
            refused.contains("this machine forbids running with no sandbox"),
            "{args:?}: {refused}"
        );
    }
    let refused = failed(&world.start("main", &["start", "--sandbox"]));
    assert!(
        refused.contains("needy runs only with no sandbox"),
        "{refused}"
    );
}

/// `--no-sandbox` lasts until `--sandbox`: a command that restarts the
/// agent (`route`) restarts it as it was. And a service running a
/// harness with no sandbox reads the machine's switch at each restart:
/// set to forbid it after the service was installed, it stops for good.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn no_sandbox_lasts_until_sandbox_and_a_service_reads_the_switch() {
    let world = World::new("service");
    if world.skips() {
        return;
    }
    link_test_kit::write_program(&world.bin.join("systemctl"), SYSTEMCTL).unwrap();
    link_test_kit::write_program(&world.bin.join("loginctl"), "#!/bin/sh\necho yes\n").unwrap();
    let files = world.home.join("Link/Agents/main/home");
    let files = files.display().to_string();
    let units = world.home.join(".config/systemd/user");
    let unit = || -> (PathBuf, String) {
        let path = std::fs::read_dir(&units)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.to_string_lossy().contains("lnk-agent-"))
            .expect("no unit");
        let text = read(&path);
        (path, text)
    };
    ok(&world.lnk(&[
        "create",
        "main",
        "--harness",
        "plain",
        "--no-sandbox",
        "--run.files",
        &files,
    ]));
    let (path, text) = unit();
    assert!(
        text.contains("\"unsandboxed\" \"--machine-file\""),
        "{text}"
    );

    // Restarted by another command: as it was, with no sandbox.
    std::fs::write(world.bin.join("active"), "").unwrap();
    let routed = ok(&world.lnk(&["route", "--none"]));
    assert!(!routed.contains("sandbox is back on"), "{routed}");
    let (_, text) = unit();
    assert!(
        text.contains("\"unsandboxed\" \"--machine-file\""),
        "{text}"
    );

    // `--sandbox`: in its sandbox again, from now on.
    std::fs::remove_file(world.bin.join("active")).unwrap();
    ok(&world.lnk(&["start", "--sandbox", "--run.files", &files]));
    let (_, text) = unit();
    assert!(
        !text.contains("unsandboxed") && text.contains("bwrap"),
        "{text}"
    );

    // Without it again: what its service runs, as systemd runs it (the
    // harness's home as HOME).
    let installed = || {
        ok(&world.lnk(&["start", "--no-sandbox", "--run.files", &files]));
        let (_, text) = unit();
        let line = |key: &str| -> String {
            let line = text.lines().find(|l| l.starts_with(key)).unwrap();
            line[key.len()..].to_string()
        };
        let argv: Vec<String> = line("ExecStart=")
            .split("\" \"")
            .map(|arg| arg.trim_matches('"').to_string())
            .collect();
        let home = line("Environment=\"HOME=")
            .trim_end_matches('"')
            .to_string();
        (argv, home)
    };
    let service = |argv: &[String], home: &str| {
        Command::new(&argv[0])
            .args(&argv[1..])
            .env_clear()
            .env("HOME", home)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(std::path::Path::new(home)),
            )
            .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
            .output()
            .unwrap_or_else(|error| panic!("{argv:?}: {error}"))
    };
    let (argv, home) = installed();
    ok(&service(&argv, &home));

    // The agent's settings don't turn its sandbox off: refused, and its service removed.
    let at = argv.iter().position(|a| a == "--agent-settings").unwrap();
    let settings_file = PathBuf::from(&argv[at + 1]);
    let kept = read(&settings_file);
    std::fs::write(
        &settings_file,
        kept.replace("sandbox = false", "sandbox = true"),
    )
    .unwrap();
    let refused = failed(&service(&argv, &home));
    assert!(refused.contains("don't turn its sandbox off"), "{refused}");
    assert!(!path.exists(), "its service is still there");
    std::fs::write(&settings_file, &kept).unwrap();

    // Then the switch set: refused, and its service removed.
    let (argv, home) = installed();
    ok(&service(&argv, &home));
    crate::common::add_policy(&world.home, "unsandboxed = false\n");
    let refused = failed(&service(&argv, &home));
    assert!(refused.contains("this machine forbids"), "{refused}");
    assert!(!path.exists(), "its service is still there");
    let log = read(&world.bin.join("systemctl.log"));
    assert!(log.contains("stop --no-block lnk-agent-"), "{log}");
}

/// On a box, as root's `/etc/lnk/kind.toml` says (here a stand-in, as
/// `lnk box create` writes it), an explicit `--no-sandbox` choice runs the
/// agent without one. Its machine kind does not silently override the flag.
#[test]
fn a_box_runs_an_agent_with_no_sandbox_when_asked() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new("box");
    let folder = world.home.join("etc-lnk");
    std::fs::create_dir_all(&folder).unwrap();
    let kind = folder.join("kind.toml");
    std::fs::write(
        &kind,
        "kind = \"box\"\nos = \"ubuntu-24.04\"\narch = \"x86_64\"\n",
    )
    .unwrap();
    for (path, mode) in [(folder.as_path(), 0o755), (kind.as_path(), 0o644)] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    let files = world.home.join("Link/Agents/main/home");
    let files = files.display().to_string();
    let started = ok(&world.lnk_with(
        &[
            "create",
            "main",
            "--harness",
            "plain",
            "--no-sandbox",
            "--foreground",
            "--run.files",
            &files,
        ],
        &[("LNK_TEST_MACHINE_KIND", &kind)],
    ));
    assert!(started.contains("There is no sandbox."), "{started}");
}

/// `[commands]` binds the harness's own programs as it binds its `run`:
/// a program the agent isn't granted is refused to its harness, runs once
/// it's allowed, and is refused again once denied (AF7).
#[test]
fn a_command_denied_is_refused_to_the_harness_too() {
    let world = World::new("commands");
    if world.skips() {
        return;
    }
    // Link's default allows any program: this agent runs only its
    // toolchains'.
    world.toolchains_only("a");
    ok(&world.start("a", &["create", "a", "--harness", "plain"]));
    // A program no toolchain of Link's default names.
    let id = link_harness_base::find_program("nproc", "/usr/bin:/bin").unwrap();
    let id = id.display().to_string();
    let runs = || {
        let ran = world.lnk(&[
            "-a",
            "a",
            "harness",
            "run",
            "--",
            "/bin/sh",
            "-c",
            &format!("'{id}' >/dev/null 2>&1; echo \"ran: $?\""),
        ]);
        String::from_utf8_lossy(&ran.stdout).contains("ran: 0")
    };
    assert!(!runs(), "its harness ran {id}, which it isn't granted");
    ok(&world.lnk(&["-a", "a", "permission", "add", "--commands.allow", &id]));
    assert!(runs(), "its harness didn't run {id}, granted");
    ok(&world.lnk(&["-a", "a", "permission", "remove", "--commands.allow", &id]));
    assert!(!runs(), "its harness ran {id}, denied");
}

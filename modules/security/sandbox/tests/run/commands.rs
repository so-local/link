//! `run`, `wrap` and `check`: a workload's arguments pass unchanged, it
//! writes only what it was given, and nothing gets through.

use super::*;

#[test]
fn a_workloads_launch_arguments_are_passed_unchanged() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("launch-argument");
    for proxy in [false, true] {
        let mut args = vec!["run", "--commands.toolchains", "shell"];
        if proxy {
            args.extend(["--internet.hosts", "example.com"]);
        }
        args.extend(["--", "/bin/echo", "--launch", "/does/not/exist"]);
        let output = world.lnk(&args).output().unwrap();
        assert!(
            output.status.success(),
            "proxy={proxy}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"--launch /does/not/exist\n");
    }
}

/// A folder it writes that isn't there is made as it starts, so what
/// the program writes there stays, as an environment's first stage
/// writing `~/app` needs.
#[test]
fn run_makes_a_folder_it_writes_that_isnt_there() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("made");
    let app = world.home.join("app");
    let output = world
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--files.write",
            "~/app",
            "--",
            "/bin/sh",
            "-c",
            "echo kept > \"$HOME/app/marker\"",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read(&app.join("marker")), "kept\n");
}

#[test]
fn run_writes_only_what_it_was_given() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("run");
    let script = r#"echo made > "$HOME/work/out"
cat "$HOME/.ssh/id_test" > "$HOME/work/key" 2>/dev/null
cat "$HOME/.lnk/safe/token" > "$HOME/work/token" 2>/dev/null
echo "$GITHUB_TOKEN" > "$HOME/work/env"
echo x > "$HOME/escaped" 2>/dev/null
exec 3<>/dev/tcp/1.1.1.1/80 2>/dev/null && echo reached > "$HOME/work/net"
exit 7"#;
    let output = world
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--files.write",
            ".",
            "--",
            "/bin/bash",
            "-c",
            script,
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(7),
        "its exit status passes through: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read(&world.work.join("out")), "made\n");
    assert_eq!(read(&world.work.join("key")), "", "it read ~/.ssh");
    assert_eq!(
        read(&world.work.join("token")),
        "",
        "it read Link's settings"
    );
    assert_eq!(
        read(&world.work.join("env")),
        "\n",
        "it got the environment"
    );
    assert!(!world.home.join("escaped").exists(), "it wrote the home");
    assert!(!world.work.join("net").exists(), "it reached the network");
    // Nothing left of its rules (what bubblewrap's version allows is
    // kept, for the next run).
    let rules = world.home.join(".lnk/safe/sandboxes");
    let left = std::fs::read_dir(&rules)
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name() != "bwrap-version")
                .count()
        })
        .unwrap_or(0);
    assert_eq!(left, 0, "rules left in {}", rules.display());
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn wrap_prints_the_command_for_a_policy() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("wrap");
    let mut child = world
        .lnk(&["wrap"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let request = serde_json::json!({
        "confinement": {
            "name": "fake",
            "write": [world.work],
            "read": [],
            "commands": "any",
            "net": {"host": {"ports": [], "localhost": false}},
        },
        "argv": ["/bin/sh", "-c", "ls ~/.ssh"],
    });
    std::io::Write::write_all(
        child.stdin.as_mut().unwrap(),
        request.to_string().as_bytes(),
    )
    .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let argv: Vec<String> = serde_json::from_value(reply["argv"].clone()).unwrap();
    let joined = argv.join(" ");
    // bubblewrap, behind what holds the placeholders of the folder's dot
    // files while it runs.
    assert_eq!(argv[1], "placeholders", "{joined}");
    assert!(argv.iter().any(|a| a.ends_with("bwrap")), "{joined}");
    assert!(joined.ends_with("-- /bin/sh -c ls ~/.ssh"), "{joined}");
    assert!(
        !joined.contains("--unshare-net"),
        "network was allowed: {joined}"
    );
    // Link's settings are hidden whatever the policy says.
    let settings = world.home.join(".lnk");
    assert!(
        joined.contains(&format!("--tmpfs {}", settings.display())),
        "{joined}"
    );
    let ran = Command::new(&argv[0]).args(&argv[1..]).output().unwrap();
    assert!(!ran.status.success(), "it listed ~/.ssh");

    // A confinement whose name could be a path is refused.
    let mut bad = world
        .lnk(&["wrap"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(
        bad.stdin.as_mut().unwrap(),
        br#"{"confinement":{"name":"../x","write":[],"read":[],"commands":"any","net":"closed"},"argv":["true"]}"#,
    )
    .unwrap();
    drop(bad.stdin.take());
    assert!(!bad.wait_with_output().unwrap().status.success());
}

#[test]
fn check_finds_nothing_getting_through() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("check");
    let output = world
        .lnk(&[
            "check",
            "--commands.toolchains",
            "shell",
            "--files.write",
            ".",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for line in [
        "blocked  read a file in your home folder",
        "blocked  list your SSH keys",
        "blocked  read Link's settings",
        "blocked  write in your home folder",
        "allowed  write ~/work",
        "blocked  reach the internet",
        "blocked  read another program's environment",
    ] {
        assert!(text.contains(line), "{line:?} in\n{text}");
    }
    assert!(!text.contains("leaked"), "{text}");
    let left: Vec<_> = std::fs::read_dir(&world.home)
        .unwrap()
        .chain(std::fs::read_dir(&world.work).unwrap())
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("lnk-check"))
        .collect();
    assert!(left.is_empty(), "the check left {left:?}");
}

/// `check` with `[commands]` tries each way around the list: the memory
/// and `/proc/self/fd` ways are closed, the loader and `LD_PRELOAD` are
/// reported as known-open gaps, and nothing counts as a leak.
#[test]
fn check_probes_the_ways_around_commands() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    if !crate::landlock() {
        return;
    }
    if loader().is_none() {
        link_test_kit::gate::lacks("a dynamic loader", "it knows x86_64's and aarch64's", None);
        return;
    }
    if !link_test_kit::out_of_tmp(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))) {
        return;
    }
    let world = World::new("waysaround");
    let me = env!("CARGO_BIN_EXE_lnk-sandbox");
    let output = world
        .lnk(&[
            "check",
            "--files.write",
            ".",
            "--commands.allow",
            me,
            "--commands.allow",
            "/bin/sh",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "a probe leaked:\n{text}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for closed in [
        "blocked  run a program copied into memory",
        "blocked  run a memory program through /proc/self/fd",
        "blocked  run a memory program of huge pages through /proc/self/fd",
        "blocked  run a file it wrote through /proc/self/fd",
    ] {
        assert!(text.contains(closed), "{closed:?} in\n{text}");
    }
    // Known gaps: open here, but a machine may close them itself (GitHub's
    // Ubuntu runners stop the loader's way), and `check` says what this
    // machine does. Either answer, never leaked (below).
    for gap in [
        "run a file it wrote with the loader",
        "load a library it wrote into a listed program",
    ] {
        assert!(
            text.contains(&format!("open     {gap}")) || text.contains(&format!("blocked  {gap}")),
            "{gap:?} in\n{text}"
        );
    }
    assert!(!text.contains("leaked"), "{text}");
}

/// A descriptor what started the sandbox left open (a CI runner leaves
/// its worker's pipes open in every step) never reaches the program: a
/// write there would land in whatever holds the other end.
#[test]
fn a_descriptor_left_open_never_reaches_the_program() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("leftopen");
    let script =
        "[ -e /proc/self/fd/7 ] && echo open; printf x >&7 2>/dev/null && echo wrote; true";
    let (_reader, writer) = std::io::pipe().unwrap();
    let left = std::os::fd::AsRawFd::as_raw_fd(&writer);
    let mut command = world.lnk(&[
        "run",
        "--commands.toolchains",
        "shell",
        "--",
        "sh",
        "-c",
        script,
    ]);
    // SAFETY: dup2 and fcntl are async-signal-safe; the pipe is open.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut command, move || {
            // Already 7, it would be closed at exec unless kept.
            let kept = match left == 7 {
                true => libc::fcntl(left, libc::F_SETFD, 0),
                false => libc::dup2(left, 7),
            };
            match kept {
                -1 => Err(std::io::Error::last_os_error()),
                _ => Ok(()),
            }
        });
    }
    let output = command.output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(out, "");
}

/// `check`'s ways around `[commands]` copy a program into a folder the
/// sandbox writes; each copy is removed once tried, so `check` leaves
/// nothing behind in your folders.
#[test]
fn a_way_tried_leaves_no_copy_behind() {
    let Some(loader) = ["/lib64/ld-linux-x86-64.so.2", "/lib/ld-linux-aarch64.so.1"]
        .into_iter()
        .find(|p| Path::new(p).exists())
    else {
        return;
    };
    let dir = TempDir::new("try-exec");
    let lib = dir.join("lib.so");
    std::fs::copy("/bin/true", &lib).unwrap();
    let out = dir.join("out");
    std::fs::create_dir(&out).unwrap();
    for (how, extra) in [
        ("ldso", ["--loader", loader]),
        ("preload", ["--lib", lib.to_str().unwrap()]),
    ] {
        let status = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-sandbox"), &dir)
            .args([
                "try-exec",
                "--how",
                how,
                "--payload",
                "/bin/true",
                "--files.write",
            ])
            .arg(&out)
            .args(extra)
            .status()
            .unwrap();
        assert!(status.code().is_some(), "{how}");
        let left: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        assert!(left.is_empty(), "{how} left {left:?}");
    }
}

/// A run checks for the bubblewrap its wall runs, the system's, never
/// one `PATH` finds: with no folder of the system's on `PATH`, it runs,
/// as `lnk sandbox check --ready` says it can.
#[test]
fn a_run_finds_bubblewrap_where_its_wall_does() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::for_specs("bwrap-path");
    let spec = world.root.join("run.toml");
    // Named by its path: no `PATH` here finds a toolchain's programs.
    std::fs::write(
        &spec,
        "schema = 1\nname = \"run\"\nversion = 1\n\n[commands]\nallow = [\"/bin/true\"]\n",
    )
    .unwrap();
    let output = world
        .lnk(&[
            "run",
            "--spec",
            &spec.display().to_string(),
            "--yes",
            "--",
            "/bin/true",
        ])
        .env("PATH", world.root.join("nothing"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A flag where the program goes is the person's mistake: exit 2, and
/// the report says where the program goes.
#[test]
fn a_flag_in_place_of_the_program_is_a_usage_error() {
    let world = World::new("flagless");
    let output = world.lnk(&["run", "--", "--allow"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("--allow isn't a flag of this command"),
        "{err}"
    );
    assert!(err.contains("goes after --"), "{err}");
}

/// With nothing waiting, `question list --json` has no line to read, and a person
/// is told so.
#[test]
fn question_list_with_none_waiting_has_no_line_and_says_so() {
    let world = World::new("asks-none");
    let json = world.lnk(&["question", "list", "--json"]).output().unwrap();
    assert!(json.status.success() && json.stdout.is_empty());
    let shown = world.lnk(&["question", "list"]).output().unwrap();
    let text = String::from_utf8_lossy(&shown.stdout);
    assert!(text.contains("No questions are waiting."), "{text}");
}

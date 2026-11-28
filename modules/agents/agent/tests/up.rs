//! `lnk agent start` end to end with a fake harness program (`lnk-fake`, a shell
//! script): the adapter contract, the settings, the box every step runs in,
//! and the sandbox, which must keep the harness out of `~/.ssh` and its
//! installer from writing outside the box. Then `lnk agent show` and `lnk agent delete`.

#[cfg(target_os = "linux")]
use std::{path::Path, process::Command};

#[cfg(target_os = "linux")]
use crate::common::core_with_sandbox;
#[cfg(target_os = "linux")]
use link_test_kit::TempDir;

#[cfg(target_os = "linux")]
#[test]
fn a_refused_model_leaves_no_new_agent() {
    use link_test_kit::{env, link_program};

    let root = TempDir::new("refused-model");
    let bin = root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let program = bin.join("lnk-agent");
    link_program(Path::new(env!("CARGO_BIN_EXE_lnk-agent")), &program).unwrap();
    // The sandbox is checked before the model is asked for: a core that
    // runs it, and a start with none where it can't run.
    let core = bin.join("lnk");
    let sandbox = crate::common::sandbox_module();
    link_program(&sandbox, &bin.join("lnk-sandbox")).unwrap();
    link_test_kit::write_core(
        &core,
        format!(
            "#!/bin/sh\n[ \"$1\" = sandbox ] && shift && exec '{}' \"$@\"\n\
             [ \"$*\" = 'model runtime list --json --start' ] || exit 1\nprintf '[]\\n'\n",
            sandbox.display()
        ),
    )
    .unwrap();
    let gate = root.join("gate");
    std::fs::create_dir(&gate).unwrap();
    let sandboxed = link_test_kit::gate::sandbox(&sandbox, &gate);
    let spec = root.join("agent.toml");
    std::fs::write(&spec, "schema = 1\n[tool.scheduler]\nenabled = true\n").unwrap();
    for (label, args, agent, supplied_spec) in [
        ("default", vec!["create", "main"], "main", false),
        ("spec", vec!["create", "main"], "main", true),
        ("named", vec!["create", "work"], "work", true),
    ] {
        let home = root.join(label);
        std::fs::create_dir(&home).unwrap();
        let mut command = env::command(&program, &home);
        command
            .args(args)
            .args(["--model.default", "local/nope", "--yes", "--foreground"])
            .env("LNK_EXE", &core);
        if supplied_spec {
            command.arg("--spec").arg(&spec);
        }
        if !sandboxed {
            command.arg("--no-sandbox");
        }
        let output = command.output().unwrap();
        let said = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{label}: {said}");
        assert!(said.contains("No model named \"nope\" runs"), "{said}");
        for path in [
            home.join(format!("Link/Agents/{agent}")),
            home.join(format!("Link/Safe/Agents/{agent}")),
            home.join(format!(".lnk/safe/agents/{agent}")),
        ] {
            assert!(
                !path.exists(),
                "{label}: refused start wrote {}",
                path.display()
            );
        }
        let status = env::command(&program, &home)
            .args(["show"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&status.stderr).contains("no agent here yet"),
            "{label}: {}{}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
    }
}

#[cfg(target_os = "linux")]
const FAKE_ADAPTER: &str = r#"#!/bin/sh
# HOME is the box, ~/.lnk/data/agents/main/harnesses/fake: the real home is six up.
real="$HOME/../../../../../.."
case "$1" in
  install)
    touch "$HOME/installed"
    mkdir -p "$HOME/.fake" && touch "$HOME/.fake/native"
    # An installer spreading out of its box.
    touch "$real/escaped" 2>/dev/null; true ;;
  configure) cat > "$HOME/settings.json" ;;
  command)
    # The "harness": writes to the files folder and tries to read a key.
    printf '{"argv":["/bin/sh","-c","echo working > \\"$LNK_FILES/out\\"; cat %s/.ssh/id_test > \\"$LNK_FILES/stolen\\" 2>/dev/null; ls /run > \\"$LNK_FILES/run\\"; ls /proc | grep -c ^[0-9] > \\"$LNK_FILES/procs\\"; true"],"write":[],"read":[]}' "$real"
    ;;
  health) exit 0 ;;
  traces) echo '[{"kind":"home","path":"~/.fake"}]' ;;
  connections) echo '["telegram"]' ;;
esac
"#;

#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn up_runs_the_harness_in_the_sandbox() {
    use std::os::unix::fs::PermissionsExt;

    let sandboxed = link_test_kit::gate::sandbox_with_landlock();
    if !sandboxed {
        eprintln!("the wrapped sandbox can't run here; testing without it");
    }
    let root = TempDir::new("up");
    let home = root.join("home");
    let bin = root.join("bin");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(home.join("Link/Safe/Agents/main")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(home.join(".ssh/id_test"), "SECRET KEY").unwrap();
    crate::common::write_agent(&home, "main", "harness = \"fake\"\n");
    let adapter = bin.join("lnk-fake");
    crate::common::write_adapter(&adapter, FAKE_ADAPTER).unwrap();
    let core = core_with_sandbox(&bin);

    let files = home.join("Link/Agents/main/home");
    let boxed = home.join(".lnk/data/agents/main/harnesses/fake");
    let lnk = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-agent"));
        command
            .args(args)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("LNK_EXE", &core)
            .env("ANTHROPIC_API_KEY", "sk-ant-test-key");
        command
    };
    let mut up = lnk(&["start", "--foreground", "--run.files"]);
    up.arg(&files);
    if !sandboxed {
        up.arg("--no-sandbox");
    }
    let output = up.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // The adapter installed and configured it in its box, with the settings.
    assert!(boxed.join("installed").exists());
    assert!(boxed.join(".fake/native").is_file());
    fn assert_visible(root: &Path) {
        for entry in std::fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            assert!(
                !entry.file_name().as_encoded_bytes().starts_with(b"."),
                "hidden entry in Home: {}",
                entry.path().display()
            );
            if entry.file_type().unwrap().is_dir() {
                assert_visible(&entry.path());
            }
        }
    }
    assert_visible(&home.join("Link"));
    // The kill switch stops it too, however it runs.
    let on_stop = home.join(".lnk/safe/sandboxes/on-stop/agent-main.json");
    assert_eq!(
        std::fs::read_to_string(&on_stop).unwrap(),
        r#"{"args":["agent","stop","-a","main"],"schema":1}"#
    );
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(boxed.join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["model"]["provider"], "anthropic");
    assert_eq!(settings["model"]["api_key"], "sk-ant-test-key");
    assert_eq!(settings["files"], files.display().to_string());
    // ... and remembered, with your secrets, readable only by you.
    let saved = home.join(".lnk/safe/secrets/secrets.toml");
    assert_eq!(
        crate::common::kept_key(
            &home,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .as_deref(),
        Some("sk-ant-test-key")
    );
    assert!(
        !std::fs::read_to_string(home.join("Link/Safe/Agents/main/agent.toml"))
            .unwrap()
            .contains("sk-ant-test-key")
    );
    assert_eq!(
        std::fs::metadata(&saved).unwrap().permissions().mode() & 0o777,
        0o600
    );

    // The harness worked in the files folder...
    assert_eq!(read(&files.join("out")), "working\n");
    // ... and, sandboxed, could not read the key, nor its installer write
    // outside the box.
    if sandboxed {
        assert_eq!(
            read(&files.join("stolen")),
            "",
            "the sandbox let the harness read ~/.ssh"
        );
        assert!(
            !home.join("escaped").exists(),
            "the installer wrote outside its box"
        );
        // No sockets of the user's session, and only its own processes.
        assert_eq!(read(&files.join("run")), "", "/run is visible");
        let procs: usize = read(&files.join("procs")).trim().parse().unwrap();
        assert!(procs < 10, "it sees {procs} processes");
    }

    // Run in the foreground, it isn't running now: `lnk agent show` says
    // so, with what it runs with, and exits nonzero.
    let status = lnk(&["show"]).output().unwrap();
    let text = String::from_utf8_lossy(&status.stdout);
    assert!(!status.status.success(), "{text}");
    // Each row a label and its value, aligned to the longest label.
    let row = |label: &str, value: &str| {
        text.lines().any(|line| {
            line.trim_start()
                .strip_prefix(label)
                .is_some_and(|rest| rest.starts_with("  ") && rest.trim() == value)
        })
    };
    assert!(row("State", "stopped"), "{text}");
    assert!(row("Harness", "fake"), "{text}");
    assert!(
        row("Harness home", "~/.lnk/data/agents/main/harnesses/fake"),
        "{text}"
    );
    assert!(row("Model", "claude-haiku-5-5"), "{text}");
    assert!(row("From", "Anthropic"), "{text}");
    assert!(row("Files", "~/Link/Agents/main/home"), "{text}");

    // `lnk agent check` finds nothing getting through.
    if sandboxed {
        let check = lnk(&["harness", "check", "--harness", "fake"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&check.stdout);
        assert!(
            check.status.success(),
            "{text}\n{}",
            String::from_utf8_lossy(&check.stderr)
        );
        for blocked in [
            "blocked  read a file in your home folder",
            "blocked  list your home folder",
            "blocked  list your SSH keys",
            "blocked  write in your home folder",
            "blocked  read another program's environment",
            "blocked  read Link's settings",
            "blocked  read a file outside your home folder",
            "allowed  write ~/Link/Agents/main/home",
        ] {
            assert!(text.contains(blocked), "{blocked:?} in\n{text}");
        }
        assert!(!text.to_lowercase().contains("leak"), "{text}");
        let left: Vec<_> = std::fs::read_dir(&home)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("lnk-check"))
            .collect();
        assert!(left.is_empty(), "the check left {left:?}");
    }

    // `lnk agent run`: a command in the harness's sandbox, its home as
    // HOME, in the files folder; its exit status, and nothing of the real
    // home (the key) readable.
    let ran = lnk(&[
        "harness",
        "run",
        "--harness",
        "fake",
        "--",
        "/bin/sh",
        "-c",
        "echo $HOME; pwd; exit 3",
    ])
    .output()
    .unwrap();
    assert_eq!(
        ran.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let text = String::from_utf8_lossy(&ran.stdout);
    assert_eq!(
        text,
        format!("{}\n{}\n", boxed.display(), files.display()),
        "{text}"
    );
    if sandboxed {
        let key = home.join(".ssh/id_test");
        let peek = lnk(&["harness", "run", "--harness", "fake", "--", "/bin/cat"])
            .arg(&key)
            .output()
            .unwrap();
        assert!(!peek.status.success());
        assert!(!String::from_utf8_lossy(&peek.stdout).contains("SECRET"));
    }

    // Switching to a harness whose install fails leaves the agent on the one
    // it had, so the next start isn't another try at the broken one.
    crate::common::write_adapter(
        &bin.join("lnk-broken"),
        "#!/bin/sh\n[ \"$1\" = install ] && { echo 'curl: (56) 429' >&2; exit 1; }\nexit 0\n",
    )
    .unwrap();
    let switched = lnk(&["harness", "set", "broken", "--foreground"])
        .output()
        .unwrap();
    assert!(!switched.status.success());
    let stderr = String::from_utf8_lossy(&switched.stderr);
    assert!(stderr.contains("Still on fake"), "{stderr}");
    let saved = std::fs::read_to_string(home.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(saved.contains("harness = \"fake\""), "{saved}");

    // A trace outside its box: `lnk agent show` lists it, `lnk agent delete`
    // removes it with the box, and leaves the files.
    std::fs::create_dir_all(home.join(".fake/state")).unwrap();
    let show = lnk(&["harness", "show", "--harness", "fake"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&show.stdout);
    assert!(
        text.contains("~/.lnk/data/agents/main/harnesses/fake"),
        "{text}"
    );
    // Among what's outside: anything in /tmp named after it is too,
    // another test's folder named for its fake harness say.
    assert!(
        text.lines()
            .any(|l| l.trim_start().starts_with("~/.fake") && l.ends_with("home")),
        "{text}"
    );
    let removed = lnk(&["harness", "delete", "--harness", "fake", "--yes"])
        .output()
        .unwrap();
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(!boxed.exists() && !home.join(".fake").exists());
    assert_eq!(read(&files.join("out")), "working\n");
}

#[cfg(target_os = "linux")]
pub(crate) fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// `lnk agent start --model.default local/qwen3` with no key uses the model running here, found
/// through the models module (`lnk model runtime list --json`, faked).
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn up_uses_a_local_model_without_a_key() {
    let root = TempDir::new("up-local");
    let home = root.join("home");
    let bin = root.join("bin");
    std::fs::create_dir_all(home.join("Link/Safe/Agents/main")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    crate::common::write_agent(
        &home,
        "main",
        &format!("harness = \"fake\"\n{}", ports_of_its_own()),
    );
    crate::common::write_adapter(&bin.join("lnk-fake"), FAKE_ADAPTER).unwrap();
    // Ollama's stand-in: a port that answers, as a start needs its model's
    // runtime to.
    let ollama = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://localhost:{}", ollama.local_addr().unwrap().port());
    // The core, answering only `lnk model runtime list --json --start`, and
    // handing `lnk sandbox` to the sandbox module: with no sandbox, the
    // harness still runs beside its proxy.
    let lnk = bin.join("fake-lnk");
    link_test_kit::write_core(
        &lnk,
        format!(
            r#"#!/bin/sh
[ "$1" = sandbox ] && shift && exec '{}' "$@"
[ "$1" = --link-registry ] && exec '{}' "$@"
[ "$1 $2 $3 $4 $5" = "model runtime list --json --start" ] && echo '[{{"name":"Ollama","base_url":"{base_url}","models":["llama3:latest","qwen3:8b"]}}]'
"#,
            crate::common::sandbox_module().display(),
            link_test_kit::programs().join("lnk").display()
        ),
    )
    .unwrap();

    // The old default files folder, ~/Link/Files, moves to ~/Link/Agents/main/home.
    let old_files = home.join("Link/Files");
    std::fs::create_dir_all(&old_files).unwrap();
    std::fs::write(old_files.join("note.txt"), "mine").unwrap();
    crate::common::write_agent(
        &home,
        "main",
        &format!(
            "harness = \"fake\"\nfiles = {:?}\n{}",
            old_files.display().to_string(),
            ports_of_its_own()
        ),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args([
            "start",
            "--foreground",
            "--no-sandbox",
            "--model.default",
            "local/qwen3",
        ])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_EXE", &lnk)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read(&home.join("Link/Agents/main/home/note.txt")), "mine");
    assert!(!old_files.exists());
    let boxed = home.join(".lnk/data/agents/main/harnesses/fake");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(boxed.join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings["files"],
        home.join("Link/Agents/main/home").display().to_string()
    );
    let mut model = settings["model"].clone();
    let actions = model
        .as_object_mut()
        .unwrap()
        .remove("actions_url")
        .unwrap();
    let actions = reqwest::Url::parse(actions.as_str().unwrap()).unwrap();
    assert_eq!(actions.host_str(), Some("127.0.0.1"));
    let parts: Vec<_> = actions.path_segments().unwrap().collect();
    assert_eq!(&parts[..2], ["_link", "url"]);
    assert!(link_sandbox_route::is_url_secret(parts[2]));
    assert_eq!(parts[3], "model");
    assert_eq!(parts.len(), 4);
    assert_eq!(
        model,
        serde_json::json!({
            "provider": "ollama",
            "api_key": "",
            "name": "qwen3:8b",
            "base_url": base_url,
        })
    );

    // A hosted provider's key set on an agent with a local model is
    // never ignored: without a terminal to ask, it's said, with how.
    let start = |args: &[&str], key: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-agent"));
        command
            .args(["start", "--foreground", "--no-sandbox"])
            .args(args)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("LNK_EXE", &lnk);
        if let Some(key) = key {
            command.env("ANTHROPIC_API_KEY", key);
        }
        let output = command.output().unwrap();
        let said = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{said}");
        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(boxed.join("settings.json")).unwrap())
                .unwrap();
        (said, settings["model"].clone())
    };
    let (said, model) = start(&[], Some("sk-ant-from-env"));
    assert!(
        said.contains("ANTHROPIC_API_KEY is set, but your agent thinks with qwen3:8b"),
        "{said}"
    );
    assert_eq!(model["name"], "qwen3:8b");
    // A hosted model by name takes its provider's variable...
    let (_, model) = start(
        &["--model.default", "claude-sonnet-5-5"],
        Some("sk-ant-from-env"),
    );
    assert_eq!(model["provider"], "anthropic");
    assert_eq!(model["name"], "claude-sonnet-5-5");
    assert_eq!(model["api_key"], "sk-ant-from-env");
    // ... and another of the same provider, the key it kept.
    let (_, model) = start(&["--model.default", "claude-haiku-5-5"], None);
    assert_eq!(model["name"], "claude-haiku-5-5");
    assert_eq!(model["api_key"], "sk-ant-from-env");
}

/// A plain SIGTERM to `lnk agent start --foreground` stops the harness
/// with it, as Ctrl-C does, rather than leave it running.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_foreground_start_stopped_stops_its_harness() {
    let root = TempDir::new("up-stopped");
    let home = root.join("home");
    let bin = root.join("bin");
    let files = home.join("Link/Agents/main/home");
    std::fs::create_dir_all(home.join("Link/Safe/Agents/main")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    crate::common::write_agent(
        &home,
        "main",
        &format!("harness = \"fake\"\n{}", ports_of_its_own()),
    );
    // The "harness" says its process id and waits.
    let adapter = r#"#!/bin/sh
case "$1" in
  configure) cat > "$HOME/settings.json" ;;
  command)
    printf '{"argv":["/bin/sh","-c","echo $$ > \\"$LNK_FILES/pid\\"; exec sleep 300"],"write":[],"read":[]}' ;;
  health) exit 0 ;;
esac
"#;
    crate::common::write_adapter(&bin.join("lnk-fake"), adapter).unwrap();
    let core = core_with_sandbox(&bin);
    let mut up = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args(["start", "--foreground", "--no-sandbox", "--run.files"])
        .arg(&files)
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_EXE", &core)
        .env("LNK_SERVICES", "off")
        .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    let _pid = loop {
        if let Ok(pid) = read(&files.join("pid")).trim().parse::<i32>() {
            break pid;
        }
        assert!(up.try_wait().unwrap().is_none(), "the start ended");
        assert!(started.elapsed().as_secs() < 30, "the harness never ran");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    // SAFETY: a signal to the child this test started.
    unsafe { libc::kill(up.id() as i32, libc::SIGTERM) };
    let status = up.wait().unwrap();
    assert!(status.success(), "{status}");
    link_test_kit::eventually(
        "the harness to end",
        std::time::Duration::from_secs(5),
        || link_test_kit::process::left_under(&files).is_empty(),
    );
}

/// A memory engine that doesn't read its input still stops with its
/// session: killed, `lnk agent memory serve` takes it along.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_memory_engine_goes_with_its_session() {
    const REAPER: &str = "LNK_TEST_MEMORY_REAPER";
    if std::env::var_os(REAPER).is_none() {
        // A disposable test process adopts the engine when its session is
        // killed. PID 1 in a container may leave a dead engine as a zombie,
        // for which signal 0 still succeeds. Keep subreaping out of the
        // concurrently running tests in this process.
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "up::a_memory_engine_goes_with_its_session"])
            .env(REAPER, "1");
        // SAFETY: only an async-signal-safe prctl in the child before exec.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(&mut child, || {
                if libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("1 passed;"),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = TempDir::new("memory-session");
    let pid_file = root.join("pid");
    let script = format!("echo $$ > {}; exec sleep 300", pid_file.display());
    // No model runtime: the core answers only the release's history, and
    // the engine starts without one.
    let core = root.join("lnk");
    link_test_kit::write_core(&core, "#!/bin/sh\nexit 1\n").unwrap();
    let mut serving = Command::new(crate::common::harness(&root.join("bin")))
        .args(["memory", "serve", "--model", "none", "--"])
        .args(["/bin/sh", "-c", &script])
        .env_clear()
        .env("HOME", &*root)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&root))
        .env("PATH", "/usr/bin:/bin")
        .env("LNK_EXE", &core)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    let pid = loop {
        if let Ok(pid) = read(&pid_file).trim().parse::<i32>() {
            break pid;
        }
        assert!(serving.try_wait().unwrap().is_none(), "serve ended");
        assert!(started.elapsed().as_secs() < 30, "the engine never ran");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    // As the sandbox ends a tool's session: its process killed, the
    // input still open.
    serving.kill().unwrap();
    serving.wait().unwrap();
    let stopping = std::time::Instant::now();
    let mut status = 0;
    loop {
        // SAFETY: wait only for the engine adopted by this test process.
        let reaped = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        assert!(reaped >= 0, "{}", std::io::Error::last_os_error());
        if reaped == pid {
            break;
        }
        if stopping.elapsed().as_secs() >= 5 {
            // SAFETY: waitpid above confirms this live child is still ours.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
                libc::waitpid(pid, &mut status, 0);
            }
            panic!("the engine kept running");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(libc::WIFSIGNALED(status));
    assert_eq!(libc::WTERMSIG(status), libc::SIGTERM);
}

/// With no agent yet, a command that needs one says so, rather than act
/// on a new one.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn no_agent_yet_is_said() {
    let root = TempDir::new("up-none");
    const NONE: &str = "no agent here yet: lnk agent start creates your first";
    for (args, says) in [
        (&["stop"][..], NONE),
        (&["spec", "show"], NONE),
        (&["show", "--permissions"], NONE),
        (&["show", "--instructions"], NONE),
        (&["restore"], NONE),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
            .args(args)
            .env_clear()
            .env("HOME", &*root)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&root))
            .env("PATH", "/usr/bin:/bin")
            .env("LNK_SERVICES", "off")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?}");
        assert!(err.contains(says), "{args:?}: {err}");
    }
}

/// A word that's neither a command nor a harness, and `--agent` or
/// `--harness` where the command names its own, each get their own
/// error; `--help` there doesn't offer them.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_wrong_word_or_flag_says_what_is_wrong() {
    let root = TempDir::new("up-words");
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
            .args(args)
            .env_clear()
            .env("HOME", &*root)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&root))
            .env("PATH", "/usr/bin:/bin")
            .env("LNK_SERVICES", "off")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        (output.status.success(), said)
    };
    for (args, says) in [
        (&["gossip"][..], "unrecognized subcommand 'gossip'"),
        (
            &["delete", "main", "--agent", "work"],
            "--agent isn't one of this command's arguments",
        ),
        (
            &["--agent", "x", "list"],
            "--agent isn't one of this command's arguments",
        ),
        (&["harness", "delete"], "names the harness it removes"),
    ] {
        let (ok, said) = run(args);
        assert!(!ok && said.contains(says), "{args:?}: {said}");
    }
    let (_, help) = run(&["delete", "--help"]);
    assert!(!help.contains("--agent"), "{help}");
    let (_, help) = run(&["harness", "delete", "--help"]);
    assert!(help.contains("--harness"), "{help}");
    let (_, help) = run(&["stop", "--help"]);
    assert!(help.contains("--agent"), "{help}");
}

/// With services off, a start says nothing runs rather than wait for a
/// service it didn't start, and `logs` on an empty log says it's empty.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn services_off_is_said_not_waited_for() {
    let root = TempDir::new("up-services-off");
    let home = root.join("home");
    let bin = root.join("bin");
    std::fs::create_dir_all(home.join("Link/Safe/Agents/main")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    crate::common::write_agent(&home, "main", "harness = \"fake\"\n");
    let adapter = r#"#!/bin/sh
case "$1" in
  configure) cat > "$HOME/settings.json" ;;
  command) printf '{"argv":["/bin/sleep","300"],"write":[],"read":[]}' ;;
esac
"#;
    crate::common::write_adapter(&bin.join("lnk-fake"), adapter).unwrap();
    let core = core_with_sandbox(&bin);
    let lnk = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
            .args(args)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("LNK_EXE", &core)
            .env("LNK_SERVICES", "off")
            .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    };
    let started = std::time::Instant::now();
    let files = home.join("Link/Agents/main/home");
    let out = lnk(&[
        "start",
        "--no-sandbox",
        "--run.files",
        files.to_str().unwrap(),
    ]);
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let on_stderr = String::from_utf8_lossy(&out.stderr);
    assert!(on_stderr.contains("LNK_SERVICES=off"), "{on_stderr}");
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
    let logs = home.join(".lnk/safe/agents/main/logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("fake.log"), "").unwrap();
    let out = lnk(&["log", "show"]);
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("Nothing in its log yet"), "{said}");
}

/// A home that is a link starts its agent: every path under it is the
/// one it leads to, as the sandbox binds it.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_home_that_is_a_link_starts() {
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    let root = TempDir::new("up-linked-home");
    let home = root.join("home");
    let linked = root.join("linked");
    let bin = root.join("bin");
    std::fs::create_dir_all(home.join("Link/Safe/Agents/main")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&home, &linked).unwrap();
    crate::common::write_agent(&home, "main", "harness = \"fake\"\n");
    let adapter = r#"#!/bin/sh
case "$1" in
  configure) cat > "$HOME/settings.json" ;;
  command) printf '{"argv":["/bin/true"],"write":[],"read":[]}' ;;
  health) exit 0 ;;
esac
"#;
    crate::common::write_adapter(&bin.join("lnk-fake"), adapter).unwrap();
    let core = core_with_sandbox(&bin);
    let out = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args(["start", "--foreground", "--run.files"])
        .arg(linked.join("Link/Agents/main/home"))
        .env_clear()
        .env("HOME", &linked)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_EXE", &core)
        .env("LNK_SERVICES", "off")
        .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// An agent's ports of its own, its proxy's among them: with no sandbox
/// its proxy is on this machine's loopback, where `main`'s usual port
/// would be every test's at once. Its proxy binds them only once it
/// starts, so no two tests are given one block, of this program or any
/// other running at once (each held, [`link_test_kit::HeldNumber`]), and none
/// is among the first blocks, which other tests' agents take as theirs
/// (each from its own home, so none sees another's).
#[cfg(target_os = "linux")]
pub(crate) fn ports_of_its_own() -> String {
    use link_agent::settings::{EPHEMERAL, FIRST_PORT, PORT_BLOCK};
    // Held until this test program ends, against every other one at once
    // on the machine as well as this one's tests.
    static GIVEN: std::sync::Mutex<Vec<link_test_kit::net::HeldNumber>> =
        std::sync::Mutex::new(Vec::new());
    let mut given = GIVEN.lock().unwrap_or_else(|e| e.into_inner());
    let bases: Vec<_> = (FIRST_PORT + 10 * PORT_BLOCK..=EPHEMERAL - PORT_BLOCK)
        .step_by(usize::from(PORT_BLOCK))
        .collect();
    let random = link_files::random_token().unwrap();
    let start = usize::from(u16::from_str_radix(&random[..4], 16).unwrap()) % bases.len();
    for &base in bases[start..].iter().chain(&bases[..start]) {
        let Some(held) = link_test_kit::net::HeldNumber::hold("agent-port-block", u32::from(base))
        else {
            continue;
        };
        // The default adapter takes the full default block, with its
        // proxy at the end. Probe all of it below outgoing ephemeral
        // ports, which another request can claim during startup.
        let free: std::io::Result<Vec<_>> = (base..base + PORT_BLOCK)
            .map(|port| std::net::TcpListener::bind(("127.0.0.1", port)))
            .collect();
        if free.is_ok() {
            given.push(held);
            return format!("port = {base}\nblock = {PORT_BLOCK}\n");
        }
    }
    panic!("no complete test harness port block is free")
}

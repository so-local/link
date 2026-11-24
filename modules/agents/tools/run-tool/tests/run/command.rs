//! Link run's `command`, over MCP: a shell command runs in the files
//! folder and answers with what it printed; without the shell, or
//! breaking its declaration, it's refused; a command past its timeout is stopped with what it started;
//! and, on Linux with bubblewrap, under `lnk sandbox run` as Link
//! defines it, it writes only the folders it's given and reads the rest.

use std::path::Path;
use std::time::Duration;

use link_run_tool::{Run, answer};
use link_test_kit::TempDir;
use serde_json::{Value, json};

const PATH: &str = "/usr/bin:/bin";

pub(super) fn run(files: &Path, commands: &[&str], sandbox: Vec<String>) -> Run {
    Run {
        context: None,
        files: files.to_path_buf(),
        path: PATH.into(),
        commands: commands.iter().map(|c| c.to_string()).collect(),
        sandbox,
        env: Vec::new(),
        timeout: Duration::from_secs(20),
        output: 1024,
        background: None,
        conversations: None,
    }
}

pub(super) fn call(run: &Run, arguments: Value) -> Value {
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "command", "arguments": arguments}});
    answer(run, &message.to_string()).unwrap()["result"].clone()
}

pub(super) fn files(name: &str) -> TempDir {
    let dir = TempDir::new(&format!("command-{name}"));
    for folder in ["work", "chats"] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
    }
    dir
}

/// A call's structured answer, with what it printed as its text says it:
/// the output is sent once, in the text.
pub(super) fn seen(ran: &Value) -> Value {
    let mut value = ran["structuredContent"].clone();
    assert!(
        value.get("stdout").is_none(),
        "the output sent twice: {value}"
    );
    let text = ran["content"][0]["text"].as_str().unwrap_or_default();
    let (head, stderr) = text.split_once("\n\nerrors:\n").unwrap_or((text, ""));
    let stdout = head.split_once("\n\noutput:\n").map_or("", |(_, out)| out);
    // Each stream is fenced as a program's output.
    let unfenced = |stream: &str| {
        if stream.is_empty() {
            return String::new();
        }
        let fenced = link_untrusted::fenced(stream).expect("the stream fenced");
        assert_eq!(fenced.kind, "output", "{stream}");
        fenced.text.to_string()
    };
    let (stdout, stderr) = (unfenced(stdout), unfenced(stderr));
    value["stdout"] = json!(stdout);
    value["stderr"] = json!(stderr);
    value
}

#[test]
fn it_runs_a_shell_command_and_says_what_it_printed() {
    let files = files("prints");
    let run = run(&files, &["sh", "cat"], Vec::new());
    let listed = answer(&run, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
    let tool = &listed["result"]["tools"][0];
    assert_eq!(tool["name"], "command");
    assert_eq!(
        tool["inputSchema"]["properties"]["command"]["type"],
        "string"
    );
    let ran = call(
        &run,
        json!({"command": "echo hi; echo oops >&2; pwd; exit 3", "description": "Says hi"}),
    );
    let value = &seen(&ran);
    assert_eq!(value["exit"], 3);
    assert_eq!(value["stderr"], "oops\n");
    assert!(value["stdout"].as_str().unwrap().starts_with("hi\n"));
    let text = ran["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("exit 3"), "{text}");
    // In the files folder; elsewhere by a cd of its own, pipes and
    // quoting the shell's.
    let pwd = call(&run, json!({"command": "cd work && pwd"}));
    assert!(
        seen(&pwd)["stdout"]
            .as_str()
            .unwrap()
            .trim_end()
            .ends_with("/work")
    );
    let piped = call(&run, json!({"command": "printf 'fed in' | cat"}));
    assert_eq!(seen(&piped)["stdout"], "fed in");
    let quoted = call(&run, json!({"command": "echo '$HOME; x'"}));
    assert_eq!(seen(&quoted)["stdout"], "$HOME; x\n");
}

/// A call's arguments are held to their declaration before anything
/// runs, naming what's wrong, as the model called them.
#[test]
fn a_call_breaking_its_declaration_is_refused() {
    let files = files("declared");
    let run = run(&files, &["sh"], Vec::new());
    for (arguments, says) in [
        (json!({"command": ["true"]}), "invalid type"),
        (json!({"command": "true", "timeout": 0}), "timeout"),
        (
            json!({"command": "true", "run_in_background": "yes"}),
            "invalid type",
        ),
        (
            json!({"command": "true", "program": "sh"}),
            "unknown field `program`",
        ),
        (json!({"description": "nothing"}), "missing field `command`"),
    ] {
        let answer = call(&run, arguments.clone());
        assert_eq!(answer["isError"], true, "{arguments}: {answer}");
        let text = answer["content"][0]["text"].as_str().unwrap();
        assert!(text.contains(says), "{arguments}: {text}");
    }
}

/// Without the shell among its programs it offers no command, and a call
/// of one is refused as a tool it doesn't have.
#[test]
fn without_the_shell_there_is_no_command() {
    let files = files("refused");
    let run = run(&files, &["cat", "ls"], Vec::new());
    let listed = answer(&run, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
    assert_eq!(listed["result"]["tools"], json!([]), "{listed}");
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "command", "arguments": {"command": ": > ran"}}});
    let refused = answer(&run, &message.to_string()).unwrap();
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no tool"),
        "{refused}"
    );
    assert!(!files.join("ran").exists());
}

/// `--var`'s variables reach each command (without a sandbox, its own),
/// and one the sandbox would refuse is refused.
#[test]
fn a_command_gets_the_variables_it_was_given() {
    let files = files("env");
    let run = Run {
        env: vec![("DISPLAY".into(), ":120".into())],
        ..run(&files, &["sh"], Vec::new())
    };
    let ran = call(&run, json!({"command": "echo $DISPLAY"}));
    assert_eq!(seen(&ran)["stdout"], ":120\n");
    assert!(link_run_tool::variable("DISPLAY=:1").is_ok());
    for refused in [
        "DISPLAY",
        "1X=a",
        "A-B=c",
        "=x",
        "LD_PRELOAD=x.so",
        "HOME=/",
    ] {
        assert!(link_run_tool::variable(refused).is_err(), "{refused}");
    }
}

#[test]
fn a_command_past_its_timeout_is_stopped_with_what_it_started() {
    let files = files("timeout");
    let run = run(&files, &["sh"], Vec::new());
    // Stopped at the timeout its call gave, well under the most, with
    // what it printed by then.
    let ran = call(
        &run,
        json!({"command": "echo begun; sleep 30 & sleep 30", "timeout": 3000}),
    );
    assert_eq!(ran["structuredContent"]["stopped"], true, "{ran}");
    assert!(
        ran["structuredContent"]["seconds"].as_f64().unwrap() < 20.0,
        "{ran}"
    );
    assert_eq!(seen(&ran)["stdout"], "begun\n");
    // One it left running in the background is stopped too, and doesn't
    // hold the call: it exits, never stopped at the most.
    let ran = call(&run, json!({"command": "sleep 30 & echo left"}));
    assert_eq!(ran["structuredContent"]["stopped"], false, "{ran}");
    assert_eq!(ran["structuredContent"]["exit"], 0, "{ran}");
    // What it printed past the most kept: its start and its end.
    let big = Run { output: 64, ..run };
    let ran = call(&big, json!({"command": "seq 1 1000"}));
    let seen = seen(&ran);
    let stdout = seen["stdout"].as_str().unwrap();
    assert!(stdout.starts_with("1\n2\n"), "{stdout}");
    assert!(stdout.contains("bytes cut"), "{stdout}");
    assert!(stdout.ends_with("1000\n"), "{stdout}");
    assert_eq!(ran["structuredContent"]["stdout_bytes"], 3893);
}

/// `lnk-sandbox`, from `link-core` beside this folder, built there.
#[cfg(target_os = "linux")]
fn lnk_sandbox() -> std::path::PathBuf {
    let core = std::env::var_os("LNK_CORE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    link_test_kit::programs_in(&core.join("../modules/sandbox")).join("lnk-sandbox")
}

/// Under the sandbox Link defines for it: `work/` written,
/// `chats/` only read, a program it wasn't given refused by the
/// sandbox itself, and nothing outside the files folder read.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn each_call_runs_in_the_sandbox_it_is_given() {
    let program = lnk_sandbox();
    // Probed in a home of its own, never the shared temporary folder's.
    let probe = link_test_kit::TempDir::new("sandbox-probe");
    if !link_test_kit::gate::sandbox(&program, &probe) {
        return;
    }
    let files = files("sandbox");
    std::fs::write(files.join("chats/said.md"), "said").unwrap();
    let home = files.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("secret"), "secret").unwrap();
    let sandbox = vec![
        "/usr/bin/env".into(),
        format!("HOME={}", home.display()),
        program.display().to_string(),
        "run".into(),
        "--files.read".into(),
        files.display().to_string(),
        "--files.write".into(),
        files.join("work").display().to_string(),
        "--files.hide".into(),
        home.display().to_string(),
        "--commands.allow".into(),
        "sh".into(),
        "--commands.allow".into(),
        "cat".into(),
    ];
    let mut runner = run(&files, &["sh", "cat", "touch"], sandbox);
    // Its variables reach it through the sandbox, no program of their own
    // (`env`) allowed.
    runner.env = vec![("DISPLAY".into(), ":120".into())];
    let write = json!({"command": "echo made > work/made.txt && cat work/made.txt"});
    let mut wrote = seen(&call(&runner, write.clone()));
    // A finite executable policy must refuse before starting anything
    // when this kernel cannot enforce it. File and lifetime checks still
    // run with a separately authored unrestricted executable policy.
    let unavailable = ["syscall unavailable", "support disabled", "probe denied"]
        .iter()
        .any(|reason| {
            wrote["stderr"]
                .as_str()
                .unwrap_or_default()
                .contains(&format!("Landlock version probe: {reason}"))
        });
    if unavailable {
        assert_ne!(wrote["exit"], 0, "{wrote}");
        assert_eq!(wrote["stdout"], "", "{wrote}");
        assert!(!files.join("work/made.txt").exists());
        // Its refusal checked; files and lifetime are tested below.
        link_test_kit::gate::lacks(
            "its case of executable enforcement",
            format_args!("the sandbox has no Landlock: {wrote}"),
            Some(link_test_kit::gate::NATIVE),
        );
        runner.sandbox.truncate(runner.sandbox.len() - 4);
        runner
            .sandbox
            .extend(["--commands.toolchains".into(), "shell".into()]);
        wrote = seen(&call(&runner, write));
    }
    let sh = |script: &str| {
        let ran = call(&runner, json!({"command": script}));
        seen(&ran)
    };
    assert_eq!(wrote["exit"], 0, "{wrote}");
    assert_eq!(wrote["stdout"], "made\n");
    assert!(files.join("work/made.txt").is_file());
    let read = sh("cat chats/said.md");
    assert_eq!(read["stdout"], "said");
    let rewrote = sh("echo forged > chats/said.md");
    assert_ne!(rewrote["exit"], 0, "{rewrote}");
    assert_eq!(
        std::fs::read_to_string(files.join("chats/said.md")).unwrap(),
        "said"
    );
    assert_eq!(sh("echo \"$DISPLAY\"")["stdout"], ":120\n");
    let hidden = sh("cat home/secret");
    assert_ne!(hidden["exit"], 0, "{hidden}");
    if !unavailable {
        // `touch` is the run's to offer, but the sandbox wasn't given it.
        let touched = sh("touch work/touched");
        assert_ne!(touched["exit"], 0, "{touched}");
        assert!(!files.join("work/touched").exists());
    }
    // Each call starts afresh: nothing a call left running outlives it.
    // Looked for here, outside its sandbox, by a word of this test's own
    // in what it left's arguments: a process ID from within the call's
    // own process namespace names another process anywhere else.
    let left = format!(
        "lnk-left-{}-{}",
        std::process::id(),
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    );
    // Builtins alone (`sleep` isn't a program it was given), until the
    // test says stop: left running, it stops once the test's done.
    // The call ends only once it's running.
    let started = sh(&format!(
        "sh -c ': > work/began; while [ ! -e work/stop ]; do :; done' {left} & \
         while [ ! -e work/began ]; do :; done; echo started"
    ));
    assert_eq!(started["stdout"], "started\n", "{started}");
    let running = || {
        std::fs::read_dir("/proc").unwrap().flatten().any(|entry| {
            std::fs::read(entry.path().join("cmdline"))
                .is_ok_and(|line| line.split(|b| *b == 0).any(|arg| arg == left.as_bytes()))
        })
    };
    let ended = std::time::Instant::now();
    while running() && ended.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let outlived = running();
    std::fs::write(files.join("work/stop"), "").unwrap();
    assert!(!outlived, "what a call left running outlived it");
}

/// `serve` without a sandbox after `--` runs nothing: it fails at once,
/// never running commands unconfined.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn serve_without_a_sandbox_runs_nothing() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lnk-run-tool"))
        .args([
            "serve",
            "--files",
            "/tmp",
            "--programs-path",
            "/bin",
            "--timeout",
            "1s",
            "--output-bytes",
            "100",
            "--program",
            "sh",
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
}

/// A command still running when `serve` dies, however it dies, dies with
/// it, though it runs in a process group of its own.
#[cfg(target_os = "linux")]
#[test]
fn a_command_ends_with_its_server() {
    use std::io::Write;
    let files = files("orphaned");
    let home = TempDir::new("orphaned-home");
    let mut serve = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-run-tool"), &home);
    // A stand-in sandbox that runs what follows its `--` as it is.
    serve
        .args(["serve", "--files"])
        .arg(files.path())
        .args([
            "--programs-path",
            PATH,
            "--timeout",
            "60s",
            "--output-bytes",
            "1024",
        ])
        .args([
            "--program",
            "sh",
            "--",
            "sh",
            "-c",
            "shift; exec \"$@\"",
            "sandbox",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut server = link_test_kit::ChildGroup::spawn(&mut serve).unwrap();
    // Named for the files folder in its command line, so it's found.
    let left = files.join("left");
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "command", "arguments": {"command": format!(
            ": > work/up; while :; do sleep 1; done # {}", left.display())}}});
    let mut input = server.take_stdin().unwrap();
    writeln!(input, "{message}").unwrap();
    input.flush().unwrap();
    link_test_kit::eventually("its command running", Duration::from_secs(10), || {
        files.join("work/up").exists()
    });
    assert!(!link_test_kit::left_under(&files).is_empty());
    // Its server killed, its own group alone.
    server.stop();
    let ended = link_test_kit::within(
        "its command ending with its server",
        Duration::from_secs(10),
        || link_test_kit::left_under(&files).is_empty(),
    );
    let left = link_test_kit::end_under(&files);
    assert!(ended.is_ok(), "outlived its server: {left:?}");
}

//! An agent running in a terminal (`lnk agent start --foreground`), as
//! its other commands see it: `status`, `start`, `logs`, `memory start`
//! and `stop`, and the times kept of its start and stop.

use std::process::Command;

use crate::common::core_with_sandbox;
use crate::up::{ports_of_its_own, read};
use link_test_kit::TempDir;

/// One running in a terminal is that to `status`, a second `start` says
/// so rather than blame another program, `logs` shows its log, memory
/// started meanwhile waits for its next start, its start and stop are
/// timed, and `stop` stops it.
/// An agent `main` on harness `fake`, whose harness runs until stopped
/// and writes its pid into the files folder as `<harness>.pid`; `other`
/// is a second harness like it.
struct Terminal {
    root: TempDir,
    home: std::path::PathBuf,
    bin: std::path::PathBuf,
    files: std::path::PathBuf,
    core: std::path::PathBuf,
}

impl Terminal {
    fn new(test: &str) -> Self {
        let root = TempDir::new(test);
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
        for harness in ["fake", "other"] {
            // Its own line three times, then a line of Link's log as the
            // Link Harness writes one.
            let run = bin.join(format!("run-{harness}"));
            let program = r#"#!/bin/sh
echo qa-logged; echo qa-logged; echo qa-logged
echo '{"at":"2026-11-09T10:00:00.042Z","level":"warning","chat":"link:3e75d6ac33d355c208c3e1140708b8a9","call":"toolu_0123456789","text":"tool web left out","schema":1}'
echo $$ > "$LNK_FILES/HARNESS.pid"
exec sleep 300
"#
            .replace("HARNESS", harness);
            link_test_kit::write_program(&run, &program).unwrap();
            let adapter = r#"#!/bin/sh
case "$1" in
  configure) cat > "$HOME/settings.json" ;;
  command)
    printf '{"argv":["/bin/sh","RUN"],"write":[],"read":[]}' ;;
  health) exit 0 ;;
esac
"#
            .replace("RUN", &run.display().to_string());
            crate::common::write_adapter(&bin.join(format!("lnk-{harness}")), &adapter).unwrap();
        }
        let core = core_with_sandbox(&bin);
        Self {
            root,
            home,
            bin,
            files,
            core,
        }
    }

    fn lnk(&self, args: &[&str]) -> Command {
        let mut command = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-agent"), &self.home);
        command
            .args(args)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("LNK_EXE", &self.core)
            .env("LNK_SERVICES", "off")
            .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
            .stdin(std::process::Stdio::null());
        command
    }

    /// Started in a terminal, on `harness` once its pid is written.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn running(&self, mut start: Command, harness: &str) -> std::process::Child {
        let mut up = start.stdout(std::process::Stdio::null()).spawn().unwrap();
        let pid = self.files.join(format!("{harness}.pid"));
        let started = std::time::Instant::now();
        while read(&pid).trim().is_empty() {
            assert!(up.try_wait().unwrap().is_none(), "the start ended");
            assert!(started.elapsed().as_secs() < 30, "{harness} never ran");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        up
    }
}

fn said(output: std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// One running in a terminal is that to `status`, a second `start` says
/// so rather than blame another program, `logs` shows its log, memory
/// started meanwhile waits for its next start, its start and stop are
/// timed, and `stop` stops it.
#[test]
fn an_agent_in_a_terminal_is_seen_and_stopped() {
    let world = Terminal::new("up-terminal");
    let (root, home) = (&world.root, &world.home);
    let lnk = |args: &[&str]| world.lnk(args);
    let mut start = lnk(&["start", "--foreground", "--no-sandbox", "--run.files"]);
    start.arg(&world.files);
    let mut up = world.running(start, "fake");
    let status = said(lnk(&["show"]).output().unwrap());
    assert!(status.contains("in a terminal"), "{status}");
    // Its log is kept as a service's is: what it printed, since this
    // start (Q-agents-13/F1).
    // Copied into it as it's printed: waited for.
    // Shown for a person: the same line in a row once, with how many
    // times; Link's line with its time, mark and short IDs.
    link_test_kit::wait::eventually(
        "the harness's lines in lnk agent log show",
        std::time::Duration::from_secs(10),
        || {
            let logs = lnk(&["log", "show"]).output().unwrap();
            let logged = said(logs.clone());
            let shown = logged.contains("qa-logged  \u{d7}3")
                && logged.contains(
                    " ! tool web left out  chat link:3e75d6ac\u{2026} \u{b7} call toolu_01\u{2026}",
                );
            (logs.status.success() && shown).then_some(()).ok_or(logged)
        },
    );
    // Each line in the file is a record with its time and the agent's
    // ID; Link's own as the harness wrote it.
    let log = read(&home.join(".lnk/safe/agents/main/logs/fake.log"));
    let records: Vec<serde_json::Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|_| panic!("{line}")))
        .collect();
    assert_eq!(records.len(), 4, "{log}");
    for record in &records {
        assert!(
            record["at"].as_str().is_some_and(|at| at.ends_with('Z')),
            "{record}"
        );
        assert_eq!(record["schema"], 1, "{record}");
        assert!(record["agent"].is_string(), "{record}");
    }
    assert_eq!(records[3]["at"], "2026-11-09T10:00:00.042Z");
    assert_eq!(records[3]["call"], "toolu_0123456789");
    // With --json, every record, none collapsed.
    let json = lnk(&["log", "show", "--json"]).output().unwrap();
    let printed: Vec<serde_json::Value> = String::from_utf8_lossy(&json.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|_| panic!("{line}")))
        .collect();
    assert_eq!(printed.len(), 4, "{}", said(json));
    assert_eq!(printed[0]["text"], "qa-logged");
    assert_eq!(printed[3]["level"], "warning");
    let again = said(lnk(&["start", "--no-sandbox"]).output().unwrap());
    assert!(again.contains("main runs in a terminal"), "{again}");
    // Memory set up while it runs there waits for its next start, and
    // says so (X7).
    let memory = said(lnk(&["memory", "start"]).output().unwrap());
    assert!(
        memory.contains("memory is on")
            && memory.contains(
                "main runs in a terminal, so it runs with what it had until you start it again there"
            ),
        "{memory}"
    );
    // Its start timed until its health check answered, as a service's is,
    // and its stop until its terminal's process was gone (X4).
    let times = home
        .join(".lnk/safe/agents/main")
        .join(link_measure_contract::client::TIMES);
    link_test_kit::wait::eventually(
        "its start's time",
        std::time::Duration::from_secs(10),
        || read(&times).contains("link.time.start_"),
    );
    let stopped = lnk(&["stop"]).output().unwrap();
    assert!(stopped.status.success(), "{}", said(stopped));
    assert!(up.wait().unwrap().success());
    assert!(
        read(&times).contains("link.time.stop.own"),
        "{}",
        read(&times)
    );
    // Nothing of Link's started for it outlives it (K8).
    link_test_kit::eventually(
        "nothing of its left running",
        std::time::Duration::from_secs(10),
        || {
            let left = link_test_kit::left_under(root);
            left.is_empty().then_some(()).ok_or(left)
        },
    );
}

/// A switch while it runs in a terminal reads what its harness holds,
/// stops it there as `stop` would, and starts the new harness in the
/// switch's own terminal.
#[test]
fn a_switch_takes_over_from_an_agent_in_a_terminal() {
    let world = Terminal::new("use-terminal");
    let mut start = world.lnk(&["start", "--foreground", "--no-sandbox", "--run.files"]);
    start.arg(&world.files);
    let mut first = world.running(start, "fake");
    let mut switch = world.lnk(&["harness", "set", "other", "--yes", "--foreground"]);
    switch.stderr(std::process::Stdio::null());
    let mut second = world.running(switch, "other");
    assert!(
        first.wait().unwrap().success(),
        "the first terminal's agent didn't stop"
    );
    let status = said(world.lnk(&["show"]).output().unwrap());
    assert!(
        status.contains("other") && status.contains("in a terminal"),
        "{status}"
    );
    let stopped = world.lnk(&["stop"]).output().unwrap();
    assert!(stopped.status.success(), "{}", said(stopped));
    assert!(second.wait().unwrap().success());
}

//! The cron driver through the program as a person sets it up: `cron
//! install` writes a line of the user's crontab (a fake one, through
//! `LNK_CRONTAB`, never the machine's), and that line, run as cron would
//! each minute, fires what's due through the bridge's way in (the
//! test's Unix socket) and stops.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use link_connection_contract::AnswerTo;
use link_scheduler_tool::{Job, Kind, Store, now};
use link_test_kit::TempDir;
use serde_json::{Value, json};

/// A `crontab` keeping the crontab in `dir`.
fn crontab(dir: &Path) -> PathBuf {
    let program = dir.join("crontab");
    link_test_kit::write_program(
        &program,
        format!(
            "#!/bin/sh\ncase \"$1\" in\n\
             -l) cat '{tab}' 2>/dev/null || {{ echo \"no crontab for $USER\" >&2; exit 1; }} ;;\n\
             -) cat > '{tab}' ;;\n*) exit 2 ;;\nesac\n",
            tab = dir.join("tab").display()
        ),
    )
    .unwrap();
    program
}

/// The bridge's way in: what it's handed, kept, each answered. Each call is checked against the keyring beside
/// `socket` (`keys/`), and taken only as the agent the socket is named
/// for (`<agent>.sock`, or `way-in.sock` for `aaaaaaaaaaaa`).
fn way_in(socket: &Path) -> Arc<Mutex<Vec<Value>>> {
    let keys = socket.parent().unwrap().join("keys");
    let agent = agent_of(socket);
    link_store_contract::testing::bound(&keys, &agent);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    link_agent_contract::testing::stand_in_bridge(socket, &keys, move |who, firing: Value| {
        if who.agent != agent {
            return json!({"error": "not this agent's"});
        }
        kept.lock().unwrap().push(firing);
        json!({"answer": "Good morning to you."})
    });
    seen
}

/// The agent the way in at `socket` is for, as its name says.
fn agent_of(socket: &Path) -> String {
    match socket.file_stem().unwrap().to_str().unwrap() {
        "way-in" => "aaaaaaaaaaaa".to_string(),
        name => name.to_string(),
    }
}

/// What the runner takes to prove as the agent of the way in at
/// `socket`.
fn caller(socket: &Path) -> Vec<String> {
    vec![
        "--as".into(),
        agent_of(socket),
        "--keyring".into(),
        socket.parent().unwrap().join("keys").display().to_string(),
    ]
}

fn job(id: &str, due: u64) -> Job {
    Job {
        occurrence: None,
        portable: None,
        model: None,
        id: id.into(),
        kind: Kind::Schedule,
        thread: "telegram:42".into(),
        text: "Good morning.".into(),
        answer: AnswerTo::Chat,
        due,
        cron: None,
        hops: 1,
        created: due,
        tries: 0,
        tools: None,
        standing: link_connection_contract::access::Standing::Owner,
        to: None,
        parent: None,
        after: Vec::new(),
        description: None,
        outcome: None,
    }
}

/// Runs the command of crontab line `line` as cron does, once: the
/// shell, `\%` a `%`, from the home.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn minute(line: &str, home: &Path) {
    let command = line.splitn(6, ' ').nth(5).unwrap().replace(r"\%", "%");
    let mut child = Command::new("sh")
        .args(["-c", &command])
        .current_dir(home)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the runner didn't stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn the_calendar_wakes_the_runner_which_fires_and_stops() {
    let dir = TempDir::new("cron");
    let (state, socket) = (dir.join("state"), dir.join("way-in.sock"));
    let program = env!("CARGO_BIN_EXE_lnk-scheduler-tool");
    let seen = way_in(&socket);
    let flags: Vec<String> = [
        "--state",
        state.to_str().unwrap(),
        "--way-in",
        socket.to_str().unwrap(),
        "--per-day",
        "100",
        "--subagents",
        "5",
        "--hops",
        "8",
        "--late",
        "run",
        "--time-zone",
        "local",
        "--timeout",
        "30s",
    ]
    .map(String::from)
    .into_iter()
    .chain(caller(&socket))
    .collect();
    let cron = |args: &[&str], flags: &[String]| {
        Command::new(program)
            .arg("cron")
            .args(args)
            .args(flags)
            .env("LNK_CRONTAB", crontab(&dir))
            .output()
            .unwrap()
    };
    let installed = cron(&["install", "--label", "main"], &flags);
    assert!(installed.status.success(), "{installed:?}");
    let tab = std::fs::read_to_string(dir.join("tab")).unwrap();
    let line = tab.lines().next().unwrap().to_string();
    assert!(line.starts_with("* * * * * "), "{tab}");
    assert!(line.contains("'run'"), "{tab}");
    assert!(line.contains("'--driver' 'cron'"), "{tab}");

    // A job due a moment ago, and one in an hour.
    let store = Store::new(&state).unwrap();
    store.put(&job("aaaaaaaaaap1", now() - 1)).unwrap();
    store.put(&job("aaaaaaaaaap2", now() + 3600)).unwrap();
    minute(&line, &dir);
    let fired = seen.lock().unwrap().clone();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0]["from"], "schedule:aaaaaaaaaap1");
    assert_eq!(fired[0]["thread"], "telegram:42");
    assert_eq!(store.jobs().len(), 1);
    // The next minute, nothing due: nothing fired.
    minute(&line, &dir);
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(state.join("cron.log").exists());

    // Paths the calendar can't start it from, and removed.
    let mut relative = flags.clone();
    relative[1] = "state".into();
    let refused = cron(&["install", "--label", "main"], &relative);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("absolute"),
        "{refused:?}"
    );
    let removed = cron(&["uninstall", "--label", "main"], &[]);
    assert!(removed.status.success(), "{removed:?}");
    assert_eq!(std::fs::read_to_string(dir.join("tab")).unwrap().trim(), "");
}

//! A message between teammates, through the program as Link runs it:
//! `serve` takes the agent's `send`, and `run` carries it to the
//! teammate's bridge's way in (`--way-in-of`), then the teammate's answer
//! to the agent's own, as a message from the teammate. Both ways in are
//! the test's, each a Unix socket taking a bridge's call, proved as the
//! scout, and answering one line.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use link_test_kit::TempDir;
use serde_json::{Value, json};

const SCOUT: &str = "a1b2c3d4e5f6";
const WRITER: &str = "g7h8j9k0m1n2";

fn dir() -> TempDir {
    TempDir::new("teams")
}

/// A bridge's way in at `socket`, checking calls against the keyring
/// in `keys`: what it's handed, and by whom, kept; each answered with
/// `answer`.
fn way_in(socket: &Path, keys: &Path, answer: &'static str) -> Arc<Mutex<Vec<Value>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    link_agent_contract::testing::stand_in_bridge(socket, keys, move |who, mut firing: Value| {
        firing["by"] = json!(who.agent);
        kept.lock().unwrap().push(firing);
        json!({"answer": answer})
    });
    seen
}

/// The caps and teams both halves take.
fn args(state: &Path) -> Vec<String> {
    [
        "--state",
        &state.display().to_string(),
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
        "--agent-id",
        SCOUT,
        "--team-id",
        "research=t4m9q2x7k1ab",
        "--teammate",
        &format!("research/writer={WRITER}"),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_teammates_answer_comes_back_through_the_ways_in() {
    let dir = dir();
    let state = dir.join("state");
    let program = env!("CARGO_BIN_EXE_lnk-scheduler-tool");
    // The scout's `send`, in its Telegram conversation.
    let mut serve = Command::new(program)
        .arg("serve")
        .args(["--floor", "owner"])
        .args(args(&state))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = serve.stdin.take().unwrap();
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "send", "arguments": {"to": "writer", "message": "Draft the report."},
        "_meta": {"link.local/standing": "owner", "link.local/thread": "telegram:42", "link.local/hops": 0}}});
    writeln!(input, "{call}").unwrap();
    drop(input);
    let output = serve.wait_with_output().unwrap();
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(answer["result"]["isError"], Value::Null, "{answer}");

    // The runner: the scout's way in, and the writer's.
    let (own, theirs) = (dir.join("scout.sock"), dir.join("writer.sock"));
    let keys = dir.join("keys");
    link_store_contract::testing::bound(&keys, SCOUT);
    let scout = way_in(&own, &keys, "");
    let writer = way_in(&theirs, &keys, "Drafted: three pages.");
    let mut run = Command::new(program)
        .arg("run")
        .args(args(&state))
        .args(["--timeout", "30s", "--way-in", &own.display().to_string()])
        .args(["--as", SCOUT, "--keyring", &keys.display().to_string()])
        .args(["--way-in-of", &format!("{WRITER}={}", theirs.display())])
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while scout.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the answer never came back");
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = run.kill();
    let _ = run.wait();
    let writer = writer.lock().unwrap().clone();
    assert_eq!(writer.len(), 1);
    // Proved as the scout, its message from it.
    assert_eq!(writer[0]["by"], SCOUT);
    assert_eq!(writer[0]["from"], format!("agent:{SCOUT}"));
    assert_eq!(writer[0]["thread"], format!("agent-{SCOUT}"));
    assert_eq!(writer[0]["text"], "Draft the report.");
    assert_eq!(writer[0]["answer"], "nowhere");
    assert_eq!(writer[0]["hops"], 1);
    let scout = scout.lock().unwrap().clone();
    assert_eq!(scout.len(), 1);
    // The writer's answer, handed to the scout's own way in by the
    // scout's runner, as the scout.
    assert_eq!(scout[0]["by"], SCOUT);
    assert_eq!(scout[0]["from"], format!("agent:{WRITER}"));
    assert_eq!(scout[0]["thread"], "telegram:42");
    assert_eq!(scout[0]["text"], "Drafted: three pages.");
    assert_eq!(scout[0]["answer"], "chat");
    assert_eq!(scout[0]["hops"], 2);
}

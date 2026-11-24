//! One runner for many agents (`run --agents <file> --zone <zone>`), as
//! Link's bridge for all of a machine's agents runs it: each agent's jobs
//! fire through its own way in, never another's; an agent taken off the
//! list stops firing and the others go on; one in another zone isn't run;
//! with none left, the runner ends; and a runner, alone or for many,
//! ends once the process that started it is gone, or in order once a
//! SIGTERM asks it to.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use link_scheduler_tool::Store;
use link_test_kit::TempDir;
use serde_json::{Value, json};

const PROGRAM: &str = env!("CARGO_BIN_EXE_lnk-scheduler-tool");

/// A bridge's way in at `socket`: what it's handed, kept; each answered. Each call is checked against the keyring beside
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
        json!({"answer": ""})
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

/// The caps both halves take, for the agent whose store is `state`, in
/// time zone `zone`.
fn caps(state: &Path, zone: &str) -> Vec<String> {
    let state = state.display().to_string();
    [
        "--state",
        &state,
        "--per-day",
        "100",
        "--subagents",
        "5",
        "--hops",
        "8",
        "--late",
        "run",
        "--time-zone",
        zone,
    ]
    .map(String::from)
    .to_vec()
}

/// What the runner runs the agent whose store is `state` with alone.
fn flags(state: &Path, socket: &Path, zone: &str) -> Vec<String> {
    let mut flags = caps(state, zone);
    flags.extend(["--timeout", "30s", "--way-in"].map(String::from));
    flags.push(socket.display().to_string());
    flags.extend(caller(socket));
    flags
}

/// [`flags`], as the runner's agents file lists them: typed, as Link
/// writes them.
/// A `run --agents` file's bytes, listing `agents` as the shared bridge
/// writes it, naming its schema.
fn listing(agents: &[link_scheduler_tool::cli::Flags]) -> Vec<u8> {
    link_files::saved::print_json(&link_scheduler_tool_contract::listed::Listed {
        agents: agents.to_vec(),
    })
    .unwrap()
}

fn listed(state: &Path, socket: &Path, zone: &str) -> link_scheduler_tool::cli::Flags {
    link_scheduler_tool::cli::Flags {
        state: Some(state.to_path_buf()),
        per_day: Some(100),
        subagents: Some(5),
        hops: Some(8),
        late: Some(link_scheduler_tool::cli::FlagsLate::Run),
        time_zone: Some(zone.into()),
        timeout: Some(link_actions::value::Duration::seconds(30)),
        way_in: Some(socket.to_path_buf()),
        r#as: Some(agent_of(socket)),
        keyring: Some(socket.parent().unwrap().join("keys")),
        ..Default::default()
    }
}

/// A subagent asked for in the agent's store `state`, as its harness
/// would through the tool: due now.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn task(state: &Path, text: &str) {
    let mut serve = Command::new(PROGRAM)
        .arg("serve")
        .args(["--floor", "owner"])
        .args(caps(state, "local"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "task", "arguments": {"prompt": text},
        "_meta": {"link.local/standing": "owner", "link.local/thread": "telegram:42", "link.local/hops": 0}}});
    writeln!(serve.stdin.take().unwrap(), "{call}").unwrap();
    let output = serve.wait_with_output().unwrap();
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(answer["result"]["isError"], Value::Null, "{answer}");
}

/// Waits until `seen` holds a message saying `text`.
fn fired(seen: &Arc<Mutex<Vec<Value>>>, text: &str) {
    link_test_kit::eventually(
        &format!("the scheduler to fire {text:?}"),
        Duration::from_secs(20),
        || {
            let observed = texts(seen);
            observed
                .iter()
                .any(|f| f == text)
                .then_some(())
                .ok_or(observed)
        },
    );
}

fn texts(seen: &Arc<Mutex<Vec<Value>>>) -> Vec<String> {
    let seen = seen.lock().unwrap();
    seen.iter()
        .map(|f| f["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn one_runner_fires_each_agents_jobs_through_its_own_way_in() {
    let dir = TempDir::new("many");
    let (ada, bea, cleo) = (dir.join("ada"), dir.join("bea"), dir.join("cleo"));
    let sockets = [
        dir.join("ada.sock"),
        dir.join("bea.sock"),
        dir.join("cleo.sock"),
    ];
    let (to_ada, to_bea, to_cleo) = (
        way_in(&sockets[0]),
        way_in(&sockets[1]),
        way_in(&sockets[2]),
    );
    let file = dir.join("agents.json");
    let list = |agents: &[link_scheduler_tool::cli::Flags]| {
        std::fs::write(&file, listing(agents)).unwrap();
    };
    // Cleo is in another time zone: not this runner's to fire.
    list(&[
        listed(&ada, &sockets[0], "local"),
        listed(&bea, &sockets[1], "local"),
        listed(&cleo, &sockets[2], "Europe/Paris"),
    ]);
    let log = dir.join("runner.log");
    let mut command = link_test_kit::env::command(PROGRAM, &dir);
    command
        .args(["run", "--agents"])
        .arg(&file)
        .args(["--time-zone", "local"])
        .stderr(std::fs::File::create(&log).unwrap());
    let run = link_test_kit::ChildGroup::spawn(&mut command).unwrap();

    task(&ada, "Ada's errand.");
    task(&bea, "Bea's errand.");
    task(&cleo, "Cleo's errand.");
    fired(&to_ada, "Ada's errand.");
    fired(&to_bea, "Bea's errand.");
    assert_eq!(texts(&to_ada), ["Ada's errand."]);
    assert_eq!(texts(&to_bea), ["Bea's errand."]);
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(said.contains("isn't this runner's"), "{said}");

    // The lock stays held for the runner's whole lifetime, including
    // finishing any firing. Observe it released, rather than guessing
    // when this machine has reread the agent list.
    let bea_runner = std::fs::File::open(bea.join("runner.lock")).unwrap();
    assert!(matches!(
        bea_runner.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    // Bea off the list: hers waits, Ada's goes on.
    list(&[listed(&ada, &sockets[0], "local")]);
    link_test_kit::eventually("Bea's runner to stop", Duration::from_secs(20), || {
        bea_runner.try_lock()
    });
    // Let the test's observation go: it must not prevent a broken
    // runner from starting or firing while the assertions run.
    bea_runner.unlock().unwrap();
    task(&bea, "Bea's second.");
    task(&ada, "Ada's second.");
    fired(&to_ada, "Ada's second.");
    assert_eq!(texts(&to_bea), ["Bea's errand."]);
    let waiting = Store::new(&bea).unwrap().jobs();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].text, "Bea's second.");
    assert!(to_cleo.lock().unwrap().is_empty());

    // None left: it ends.
    list(&[]);
    let status = link_test_kit::eventually(
        "the runner to end with no agents left",
        Duration::from_secs(10),
        || run.try_status().unwrap(),
    );
    assert!(status.success(), "{status}");
}

/// Starts `run` with `args` from a shell that leaves it behind a moment
/// later, as a bridge killed, or ended, would: the runner's process ID.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn left_behind(args: &[String]) -> i32 {
    let output = Command::new("/bin/sh")
        .args([
            "-c",
            "\"$0\" \"$@\" >/dev/null 2>&1 & echo $!; sleep 1",
            PROGRAM,
            "run",
        ])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap()
}

/// Whether process `pid` runs: one ended but not yet reaped (a zombie,
/// whose parent left it to whoever reaps here) doesn't.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn alive(pid: i32) -> bool {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&output.stdout);
    !stat.trim().is_empty() && !stat.trim().starts_with('Z')
}

/// A runner, alone or for many, ends once the process that started it
/// is gone, so none outlives the agent's bridge: its terminal's `lnk
/// agent start`, its bridge's service, or the bridge it's woken by.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_runner_ends_once_what_started_it_is_gone() {
    let dir = TempDir::new("left");
    let (ada, bea) = (dir.join("ada"), dir.join("bea"));
    let socket = dir.join("ada.sock");
    let file = dir.join("agents.json");
    std::fs::write(
        &file,
        listing(&[listed(&bea, &dir.join("bea.sock"), "local")]),
    )
    .unwrap();
    let alone = left_behind(&flags(&ada, &socket, "local"));
    let many = left_behind(&[
        "--agents".into(),
        file.display().to_string(),
        "--time-zone".into(),
        "local".into(),
    ]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while (alive(alone) || alive(many)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let left: Vec<i32> = [alone, many].into_iter().filter(|&p| alive(p)).collect();
    for &pid in &left {
        // SAFETY: a runner this test started, left behind.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    assert!(
        left.is_empty(),
        "runners {left:?} outlived what started them"
    );
}

/// Asked to end by a SIGTERM, as its bridge stopping it asks, a runner,
/// alone or for many, ends in order: its firings ended, it exits as it
/// would at its end, not killed by the signal.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_runner_asked_to_end_ends_in_order() {
    let dir = TempDir::new("asked");
    let (ada, bea) = (dir.join("ada"), dir.join("bea"));
    let file = dir.join("agents.json");
    std::fs::write(
        &file,
        listing(&[listed(&bea, &dir.join("bea.sock"), "local")]),
    )
    .unwrap();
    let alone = flags(&ada, &dir.join("ada.sock"), "local");
    let many: Vec<String> = vec![
        "--agents".into(),
        file.display().to_string(),
        "--time-zone".into(),
        "local".into(),
    ];
    for args in [alone, many] {
        let mut run = Command::new(PROGRAM)
            .arg("run")
            .args(&args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // Up, and waiting on its jobs.
        std::thread::sleep(Duration::from_millis(500));
        assert!(run.try_wait().unwrap().is_none(), "{args:?}");
        // SAFETY: a signal to the runner this test started.
        unsafe { libc::kill(run.id() as i32, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = run.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                let _ = run.kill();
                panic!("{args:?}: the runner didn't end when asked");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(status.success(), "{args:?}: {status}");
    }
}

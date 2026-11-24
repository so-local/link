//! Link run's background commands, as real processes: one outlives the
//! call and the server that started it and is told once, through a way
//! in standing for the agent's bridge; a stop leaves nothing it started;
//! a supervisor killed or told the agent is stopping leaves its command
//! lost, nothing of it left, and told by a sweep; the caps hold; and,
//! on Linux with bubblewrap, all of it under `lnk sandbox run`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use link_connection_contract::Firing;
use link_run_tool::{Background, Run, answer, background};
use link_test_kit::TempDir;
use serde_json::{Value, json};

const PATH: &str = "/usr/bin:/bin";

/// The agent whose background commands these are, by its ID.
const AGENT: &str = "aaaaaaaaaaaa";
const PROGRAM: &str = env!("CARGO_BIN_EXE_lnk-run-tool");

/// A folder of its own for one test, with a files folder and a state.
fn place(name: &str) -> TempDir {
    let dir = TempDir::new(&format!("background-{name}"));
    for folder in ["files/work", "files/chats", "state"] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
    }
    dir
}

/// A way in standing for the agent's bridge: each message it took.
struct WayIn {
    path: PathBuf,
    taken: Arc<Mutex<Vec<Firing>>>,
}

impl WayIn {
    fn open(dir: &Path) -> Self {
        let path = dir.join("way-in.sock");
        let taken = Arc::new(Mutex::new(Vec::new()));
        Self::listen(&path, taken.clone());
        Self { path, taken }
    }

    /// Listens at `path`, taking each message proved as the test's agent
    /// ([`AGENT`], bound in `keys/` beside it).
    fn listen(path: &Path, taken: Arc<Mutex<Vec<Firing>>>) {
        let _ = std::fs::remove_file(path);
        let keys = path.parent().unwrap().join("keys");
        link_store_contract::testing::bound(&keys, AGENT);
        link_agent_contract::testing::stand_in_bridge(path, &keys, move |who, firing: Firing| {
            if who.agent == AGENT {
                taken.lock().unwrap().push(firing);
            }
            json!({"answer": ""})
        });
    }

    fn taken(&self) -> Vec<Firing> {
        self.taken.lock().unwrap().clone()
    }

    /// Waits for `n` messages, then gives every one taken. A message
    /// that must not come is counted with [`WayIn::taken`] once whatever
    /// could send it is done: a sweep tells before it returns, and the
    /// stand-in takes a message before it answers.
    fn wait_for(&self, n: usize) -> Vec<Firing> {
        link_test_kit::eventually("its messages", Duration::from_secs(30), || {
            self.taken().len() >= n
        });
        self.taken()
    }
}

fn run(dir: &Path, way_in: &Path, sandbox: Vec<String>) -> Run {
    Run {
        context: None,
        files: dir.join("files"),
        path: PATH.into(),
        commands: vec!["sh".into(), "seq".into(), "sleep".into()],
        sandbox,
        env: Vec::new(),
        timeout: Duration::from_secs(20),
        output: 1024,
        background: Some(Background {
            program: PROGRAM.into(),
            state: dir.join("state"),
            way_in: way_in.to_path_buf(),
            caller: background::Caller {
                agent: AGENT.into(),
                keyring: Some(way_in.parent().unwrap().join("keys")),
            },
            at_once: 2,
            timeout: Duration::from_secs(60),
            output: 1 << 20,
            keep: Duration::from_secs(3600),
            per_day: 100,
            hops: 8,
        }),
        conversations: None,
    }
}

fn call(run: &Run, name: &str, arguments: Value) -> Value {
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": arguments,
            "_meta": {"link.local/thread": "telegram:42", "link.local/hops": 1}}});
    answer(run, &message.to_string()).unwrap()["result"].clone()
}

/// [`call`], from conversation `thread`.
#[cfg(target_os = "linux")]
fn call_in(run: &Run, thread: &str, name: &str, arguments: Value) -> Value {
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": arguments,
            "_meta": {"link.local/thread": thread, "link.local/hops": 1}}});
    answer(run, &message.to_string()).unwrap()["result"].clone()
}

/// The arguments of a call running `script` in the background.
fn background_of(script: &str) -> Value {
    json!({"command": script, "run_in_background": true, "description": "In the background"})
}

fn start(run: &Run, script: &str) -> String {
    start_with(run, background_of(script))
}

/// Starts a background command as `arguments` say: its id.
fn start_with(run: &Run, arguments: Value) -> String {
    let started = call(run, "command", arguments);
    assert_ne!(started["isError"], true, "{started}");
    started["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// What a `read` gave, from its text alone: the output is sent once,
/// fenced as a program's.
fn read_text(read: &Value) -> String {
    assert!(
        read["structuredContent"].get("text").is_none(),
        "the output sent twice: {read}"
    );
    let text = read["content"][0]["text"].as_str().unwrap();
    let fenced = link_untrusted::fenced(text).expect("the output fenced");
    assert_eq!(fenced.kind, "output", "{text}");
    fenced.text.to_string()
}

fn record(run: &Run, id: &str) -> background::Record {
    background::load(&run.background.as_ref().unwrap().state, id).unwrap()
}

/// Whether process `pid` (of this machine's process list) runs.
#[cfg(target_os = "linux")]
fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only asks.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// The processes whose command line is `marker`, each a pid.
#[cfg(target_os = "linux")]
fn running_with(marker: &str) -> Vec<i32> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<i32>().ok())
        else {
            continue;
        };
        let Ok(line) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let line = String::from_utf8_lossy(&line).replace('\0', " ");
        // Not a zombie: it runs.
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let zombie = state
            .rsplit_once(')')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'));
        if line.trim_end() == marker && !zombie {
            found.push(pid);
        }
    }
    found
}

/// The supervisor of command `id`.
#[cfg(target_os = "linux")]
fn supervisor_of(id: &str) -> i32 {
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<i32>().ok())
        else {
            continue;
        };
        let Ok(line) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        // Its words, as its declaration writes them: `supervise <id>`,
        // then the flags.
        let words: Vec<String> = String::from_utf8_lossy(&line)
            .split('\0')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect();
        if words.iter().any(|w| w == "supervise") && words.iter().any(|w| w == id) {
            return pid;
        }
    }
    panic!("no supervisor of {id}")
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn it_outlives_the_call_and_its_server_and_is_told_once() {
    let dir = place("told");
    let way_in = WayIn::open(&dir);
    let server = run(&dir, &way_in.path, Vec::new());
    let listed = answer(&server, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["command", "list", "read", "stop"]);
    let started = Instant::now();
    let id = start(&server, "echo begun; sleep 2; echo done >&2; exit 3");
    // The call came back at once, and the server's gone: its turn ended.
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(server);
    assert!(record(&run(&dir, &way_in.path, Vec::new()), &id).state == background::State::Running);
    let told = way_in.wait_for(1);
    assert_eq!(told.len(), 1, "{told:?}");
    let firing = &told[0];
    assert_eq!(firing.thread, "telegram:42");
    assert_eq!(firing.from, format!("run:{id}"));
    assert_eq!(firing.hops, 2);
    assert!(firing.text.contains("exit 3"), "{}", firing.text);
    assert!(firing.text.contains("begun\ndone\n"), "{}", firing.text);
    // A new server, the next turn's, finds it ended and told; its output
    // whole in the files folder; a sweep tells nobody again.
    let next = run(&dir, &way_in.path, Vec::new());
    let ended = record(&next, &id);
    assert_eq!(ended.state, background::State::Exited);
    assert_eq!(ended.exit, Some(3));
    assert!(ended.told);
    assert_eq!(
        std::fs::read_to_string(dir.join(format!("files/background/{id}.out"))).unwrap(),
        "begun\ndone\n"
    );
    let listed = call(&next, "list", json!({}));
    assert_eq!(
        listed["structuredContent"]["commands"][0]["state"],
        "exited"
    );
    let read = call(&next, "read", json!({"id": id, "from": 6}));
    assert_eq!(read_text(&read), "done\n");
    assert_eq!(read["structuredContent"]["next"], 11);
    let swept = std::process::Command::new(PROGRAM)
        .arg("sweep")
        .arg("--state")
        .arg(dir.join("state"))
        .status()
        .unwrap();
    assert!(swept.success());
    assert_eq!(way_in.taken().len(), 1);
}

/// Another conversation of the same agent neither lists, reads nor
/// stops a background command it didn't start.
#[cfg(target_os = "linux")]
#[test]
fn a_command_is_only_its_conversations() {
    let dir = place("conversations");
    let way_in = WayIn::open(&dir);
    let server = run(&dir, &way_in.path, Vec::new());
    let id = start(&server, "echo mine; sleep 30");
    let listed = call_in(&server, "slack:7", "list", json!({}));
    assert_eq!(
        listed["structuredContent"]["commands"],
        json!([]),
        "{listed}"
    );
    for name in ["read", "stop"] {
        let refused = call_in(&server, "slack:7", name, json!({"id": id}));
        assert_eq!(refused["isError"], true, "{name}: {refused}");
    }
    let refused = call_in(&server, "", "list", json!({}));
    assert_eq!(refused["isError"], true, "{refused}");
    let listed = call(&server, "list", json!({}));
    assert_eq!(
        listed["structuredContent"]["commands"][0]["id"],
        id.as_str()
    );
    let stopped = call(&server, "stop", json!({"id": id}));
    assert_eq!(
        stopped["structuredContent"]["state"], "stopped",
        "{stopped}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_stop_leaves_nothing_it_started() {
    let dir = place("stop");
    let way_in = WayIn::open(&dir);
    let server = run(&dir, &way_in.path, Vec::new());
    let id = start(
        &server,
        "sleep 3101 & sleep 3101 & (sleep 3101 &) ; echo up; wait",
    );
    link_test_kit::eventually(
        "three of sleep 3101 running",
        Duration::from_secs(10),
        || running_with("sleep 3101").len() == 3,
    );
    let stopped = call(&server, "stop", json!({"id": id}));
    assert_eq!(
        stopped["structuredContent"]["state"], "stopped",
        "{stopped}"
    );
    link_test_kit::eventually("every sleep 3101 to end", Duration::from_secs(5), || {
        running_with("sleep 3101").is_empty()
    });
    let told = way_in.wait_for(1);
    assert_eq!(told.len(), 1);
    assert!(told[0].text.contains("stopped"), "{}", told[0].text);
    assert!(told[0].text.contains("up"), "{}", told[0].text);
}

#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_supervisor_killed_leaves_its_command_lost_and_told_by_a_sweep() {
    let dir = place("killed");
    let way_in = WayIn::open(&dir);
    let server = run(&dir, &way_in.path, Vec::new());
    let id = start(&server, "sleep 3102 & sleep 3102; wait");
    link_test_kit::eventually("two of sleep 3102 running", Duration::from_secs(10), || {
        running_with("sleep 3102").len() == 2
    });
    // Killed without a word: the machine's OOM killer, a kill -9.
    let supervisor = supervisor_of(&id);
    // SAFETY: a signal to the supervisor this test started.
    unsafe { libc::kill(supervisor, libc::SIGKILL) };
    link_test_kit::eventually("its supervisor to end", Duration::from_secs(5), || {
        !alive(supervisor)
    });
    // Still running, unowned, until a sweep finds it: the next turn's.
    let listed = call(&server, "list", json!({}));
    assert_eq!(listed["structuredContent"]["commands"][0]["state"], "lost");
    link_test_kit::eventually("every sleep 3102 to end", Duration::from_secs(5), || {
        running_with("sleep 3102").is_empty()
    });
    assert!(!record(&server, &id).told);
    let swept = std::process::Command::new(PROGRAM)
        .arg("sweep")
        .arg("--state")
        .arg(dir.join("state"))
        .status()
        .unwrap();
    assert!(swept.success());
    let told = way_in.wait_for(1);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].text.contains("lost"), "{}", told[0].text);
}

/// A command runs only once its record names its group: what it finds
/// as it starts is its own process group there, so a supervisor killed
/// at any moment leaves nothing a sweep can't find.
#[cfg(target_os = "linux")]
#[test]
fn a_command_starts_only_once_its_record_names_its_group() {
    let dir = place("gated");
    let way_in = WayIn::open(&dir);
    let server = run(&dir, &way_in.path, Vec::new());
    let records = dir.join("state").display().to_string();
    let id = start(
        &server,
        &format!("grep -qs '\"group\":'$$, '{records}'/*.json && echo named || echo unnamed"),
    );
    link_test_kit::eventually("it to exit", Duration::from_secs(10), || {
        record(&server, &id).state == background::State::Exited
    });
    let ended = record(&server, &id);
    assert_eq!(ended.exit, Some(0));
    assert_eq!(
        std::fs::read_to_string(dir.join(format!("files/background/{id}.out"))).unwrap(),
        "named\n"
    );
}

/// The agent's service stopping (SIGTERM to all of it): the command
/// ends, lost, and isn't told until the agent is back.
#[cfg(target_os = "linux")]
#[test]
fn the_agent_stopping_ends_it_lost_and_it_is_told_when_back() {
    let dir = place("term");
    let way_in = WayIn::open(&dir);
    let server = run(&dir, &way_in.path, Vec::new());
    let id = start(&server, "sleep 3103; echo never");
    link_test_kit::eventually("one of sleep 3103 running", Duration::from_secs(10), || {
        running_with("sleep 3103").len() == 1
    });
    let supervisor = supervisor_of(&id);
    // SAFETY: a signal to the supervisor this test started.
    unsafe { libc::kill(supervisor, libc::SIGTERM) };
    link_test_kit::eventually("its supervisor to end", Duration::from_secs(5), || {
        !alive(supervisor)
    });
    assert!(running_with("sleep 3103").is_empty());
    let ended = record(&server, &id);
    assert_eq!(ended.state, background::State::Lost);
    assert!(!ended.told);
    // Its supervisor, which tells, is gone.
    assert!(way_in.taken().is_empty());
    // Back: its bridge sweeps as it starts.
    background::sweep(&dir.join("state"));
    let told = way_in.wait_for(1);
    assert_eq!(told.len(), 1);
    assert!(
        told[0].text.contains("lost: the agent stopped"),
        "{}",
        told[0].text
    );
}

/// A way in that isn't there when it ends: told once it is.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn an_ending_waits_for_the_bridge() {
    let dir = place("later");
    let socket = dir.join("way-in.sock");
    let server = run(&dir, &socket, Vec::new());
    let id = start(&server, "echo quick");
    link_test_kit::eventually("it to exit", Duration::from_secs(10), || {
        record(&server, &id).state == background::State::Exited
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(!record(&server, &id).told);
    let taken = Arc::new(Mutex::new(Vec::new()));
    WayIn::listen(&socket, taken.clone());
    let way_in = WayIn {
        path: socket,
        taken,
    };
    let swept = background::sweep(&dir.join("state"));
    assert!(swept.untold.is_empty(), "{swept:?}");
    let told = way_in.wait_for(1);
    assert_eq!(told.len(), 1);
    assert!(told[0].text.contains("quick"));
    assert!(record(&server, &id).told);
    background::sweep(&dir.join("state"));
    assert_eq!(way_in.taken().len(), 1);
}

#[test]
fn the_caps_hold() {
    let dir = place("caps");
    let way_in = WayIn::open(&dir);
    let mut server = run(&dir, &way_in.path, Vec::new());
    // At most two at once: a third is refused until one ends.
    let first = start(&server, "sleep 30");
    let _second = start(&server, "sleep 30");
    let third = call(&server, "command", background_of("true"));
    assert_eq!(third["isError"], true, "{third}");
    assert!(
        third["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("at once")
    );
    call(&server, "stop", json!({"id": first}));
    let _third = start(&server, "true");
    // Past the timeout its call gave, under the most, stopped and told so.
    let background = server.background.as_mut().unwrap();
    background.at_once = 8;
    let slow = start_with(
        &server,
        json!({"command": "echo slow; sleep 30", "run_in_background": true, "timeout": 1000}),
    );
    assert_eq!(record(&server, &slow).timeout_ms, 1000);
    link_test_kit::eventually("the slow one to time out", Duration::from_secs(10), || {
        record(&server, &slow).state == background::State::TimedOut
    });
    // Its output kept to its most: the end, and what was dropped counted.
    let background = server.background.as_mut().unwrap();
    background.output = 4096;
    let long = start(&server, "seq 1 20000");
    link_test_kit::eventually("the long one to exit", Duration::from_secs(10), || {
        record(&server, &long).state == background::State::Exited
    });
    let kept = record(&server, &long);
    assert!(kept.dropped > 0);
    let total = kept.bytes();
    assert_eq!(
        total,
        (1..=20000)
            .map(|n: u32| n.to_string().len() as u64 + 1)
            .sum::<u64>()
    );
    let end = call(&server, "read", json!({"id": long}));
    assert!(read_text(&end).ends_with("19999\n20000\n"));
    let start_of = call(&server, "read", json!({"id": long, "from": 0}));
    assert_eq!(start_of["structuredContent"]["from"], kept.dropped);
    // Too far down a chain: refused.
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "command", "arguments": background_of("true"),
            "_meta": {"link.local/thread": "telegram:42", "link.local/hops": 8}}});
    let deep = answer(&server, &message.to_string()).unwrap()["result"].clone();
    assert_eq!(deep["isError"], true, "{deep}");
    // No conversation said: refused, nobody to tell.
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "command", "arguments": background_of("true")}});
    let nowhere = answer(&server, &message.to_string()).unwrap()["result"].clone();
    assert_eq!(nowhere["isError"], true, "{nowhere}");
    // A start that fails isn't counted.
    let background = server.background.as_mut().unwrap();
    let program = std::mem::replace(&mut background.program, "/nonexistent/lnk-run-tool".into());
    let failed = call(&server, "command", background_of("true"));
    assert_eq!(failed["isError"], true, "{failed}");
    let background = server.background.as_mut().unwrap();
    background.program = program;
    // Past the most started in a day: refused, each ending being a turn.
    // Five started so far today.
    background.per_day = 5;
    let today = call(&server, "command", background_of("true"));
    assert_eq!(today["isError"], true, "{today}");
    assert!(
        today["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("in the last day"),
        "{today}"
    );
    // Without a place for them, none are offered, nor its argument.
    let plain = Run {
        background: None,
        conversations: None,
        ..run(&dir, &way_in.path, Vec::new())
    };
    let listed = answer(&plain, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert!(tools[0]["inputSchema"]["properties"]["run_in_background"].is_null());
    let refused = call(&plain, "command", background_of("true"));
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(call(&plain, "list", json!({}))["isError"].is_null());
}

#[test]
fn what_ended_is_removed_past_its_keep_and_all_when_forgotten() {
    let dir = place("keep");
    let way_in = WayIn::open(&dir);
    let mut server = run(&dir, &way_in.path, Vec::new());
    let quick = start(&server, "echo quick");
    way_in.wait_for(1);
    let state = dir.join("state");
    // Not past its keep: still there.
    assert!(background::sweep_quietly(&state).removed.is_empty());
    let mut kept = record(&server, &quick);
    kept.keep_secs = 0;
    background::save(&state, &kept).unwrap();
    assert_eq!(
        background::sweep_quietly(&state).removed,
        std::slice::from_ref(&quick)
    );
    assert!(!dir.join(format!("files/background/{quick}.out")).exists());
    // Forgotten: the running stopped, everything gone.
    server.background.as_mut().unwrap().at_once = 4;
    let _long = start(&server, "sleep 3104");
    #[cfg(target_os = "linux")]
    link_test_kit::eventually("one of sleep 3104 running", Duration::from_secs(10), || {
        running_with("sleep 3104").len() == 1
    });
    let said = background::forget(&state);
    assert_eq!(said.len(), 1, "{said:?}");
    #[cfg(target_os = "linux")]
    assert!(running_with("sleep 3104").is_empty());
    assert!(!state.exists());
    assert!(!dir.join("files/background").exists());
}

/// `lnk-sandbox`, from `link-core` beside this folder, built there.
#[cfg(target_os = "linux")]
fn lnk_sandbox() -> PathBuf {
    let core = std::env::var_os("LNK_CORE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    link_test_kit::programs_in(&core.join("../modules/sandbox")).join("lnk-sandbox")
}

/// Under `lnk sandbox run`: its output written, the files folder read
/// and only `work/` written, and nothing left once it's stopped or its
/// supervisor is killed.
#[cfg(target_os = "linux")]
#[test]
fn in_its_sandbox_nothing_outlives_it() {
    let program = lnk_sandbox();
    // Probed in a home of its own, never the shared temporary folder's.
    let probe = link_test_kit::TempDir::new("sandbox-probe");
    if !link_test_kit::gate::sandbox(&program, &probe) {
        return;
    }
    let dir = place("sandboxed");
    let way_in = WayIn::open(&dir);
    let files = dir.join("files");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let sandbox = vec![
        "/usr/bin/env".into(),
        format!("HOME={}", home.display()),
        program.display().to_string(),
        "run".into(),
        "--files.read".into(),
        files.display().to_string(),
        "--files.write".into(),
        files.join("work").display().to_string(),
        "--commands.toolchains".into(),
        "shell".into(),
    ];
    let server = run(&dir, &way_in.path, sandbox);
    let id = start(
        &server,
        "echo made > work/made; echo nope > chats/forged 2>&1; echo ran",
    );
    let told = way_in.wait_for(1);
    assert_eq!(told.len(), 1);
    assert!(files.join("work/made").is_file());
    assert!(!files.join("chats/forged").exists());
    assert!(told[0].text.contains("ran"), "{}", told[0].text);
    assert!(record(&server, &id).told);
    // Stopped: nothing left.
    let stopped = start(&server, "sleep 3105 & sleep 3105 & wait");
    link_test_kit::eventually("two of sleep 3105 running", Duration::from_secs(10), || {
        running_with("sleep 3105").len() == 2
    });
    call(&server, "stop", json!({"id": stopped}));
    link_test_kit::eventually("every sleep 3105 to end", Duration::from_secs(5), || {
        running_with("sleep 3105").is_empty()
    });
    // Its supervisor killed: the sandbox goes with it, at once.
    let killed = start(&server, "sleep 3106 & sleep 3106 & wait");
    link_test_kit::eventually("two of sleep 3106 running", Duration::from_secs(10), || {
        running_with("sleep 3106").len() == 2
    });
    let supervisor = supervisor_of(&killed);
    // SAFETY: a signal to the supervisor this test started.
    unsafe { libc::kill(supervisor, libc::SIGKILL) };
    link_test_kit::eventually("every sleep 3106 to end", Duration::from_secs(5), || {
        running_with("sleep 3106").is_empty()
    });
}

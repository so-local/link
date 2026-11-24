//! The address driver through the program, as a machine that runs an
//! agent only while it answers sets it up: the tool (`serve`) tells the
//! machine's address (the test's Unix socket, a stand-in for the
//! machine) when the jobs are next due after each call that changes
//! them, and a wake (`run --driver address`, once) fires what's due
//! through the bridge's way in, tells the machine the next time, and
//! stops, leaving nothing running. Each post is proved as the agent:
//! another's is refused.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use link_connection_contract::AnswerTo;
use link_scheduler_tool::{Job, Kind, Store, now};
use link_test_kit::TempDir;
use serde_json::{Value, json};

/// The agent the scheduler is for, and another bound on the machine.
const AGENT: &str = "aaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbb";

const PROGRAM: &str = env!("CARGO_BIN_EXE_lnk-scheduler-tool");

/// The machine's address at `socket`: each post taken only when proved
/// with `AGENT`'s key (its binding in the keyring in `keys`), kept, and
/// answered `{}`; any other answered `{"error"}`.
fn machine(socket: &Path, keys: &Path) -> Arc<Mutex<Vec<Value>>> {
    let binding = link_sandbox_binding::Keyring::at(keys)
        .bound(AGENT)
        .expect("the agent is bound");
    let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
    let posts = Arc::new(Mutex::new(Vec::new()));
    let kept = posts.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let answer = match link_sandbox_binding::take::<Value>(&stream, &binding) {
                Ok((who, action)) => {
                    assert_eq!(who.agent, AGENT);
                    kept.lock().unwrap().push(action);
                    json!({})
                }
                Err(err) => json!({"error": format!("{err:#}")}),
            };
            let _ = (&stream).write_all(format!("{answer}\n").as_bytes());
        }
    });
    posts
}

/// The agent's bridge's way in at `socket`: each firing proved as
/// `AGENT`, kept, and answered.
fn way_in(socket: &Path, keys: &Path) -> Arc<Mutex<Vec<Value>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    link_agent_contract::testing::stand_in_bridge(socket, keys, move |who, firing: Value| {
        if who.agent != AGENT {
            return json!({"error": "not this agent's"});
        }
        kept.lock().unwrap().push(firing);
        json!({"answer": "Good morning to you."})
    });
    seen
}

/// A test's folder: the agent and another bound in `keys/`.
struct Machine {
    dir: TempDir,
    state: PathBuf,
    keys: PathBuf,
    address: PathBuf,
    posts: Arc<Mutex<Vec<Value>>>,
}

fn set_up(name: &str) -> Machine {
    let dir = TempDir::new(name);
    let (state, keys, address) = (
        dir.join("state"),
        dir.join("keys"),
        dir.join("machine.sock"),
    );
    link_store_contract::testing::bound(&keys, AGENT);
    link_store_contract::testing::bound(&keys, OTHER);
    let posts = machine(&address, &keys);
    Machine {
        dir,
        state,
        keys,
        address,
        posts,
    }
}

impl Machine {
    /// The caps and the address driver's flags, proved as `agent`.
    fn flags(&self, agent: &str) -> Vec<String> {
        [
            "--state",
            self.state.to_str().unwrap(),
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
            "--driver",
            "address",
            "--socket",
            self.address.to_str().unwrap(),
            "--as",
            agent,
            "--keyring",
            self.keys.to_str().unwrap(),
        ]
        .map(String::from)
        .to_vec()
    }

    /// `serve` as `agent`, handed `calls` (each a `tools/call`'s params)
    /// after `initialize`: each call's result.
    fn serve(&self, agent: &str, calls: &[Value]) -> Vec<Value> {
        let mut child = link_test_kit::env::command(PROGRAM, &self.dir)
            .arg("serve")
            .args(["--floor", "owner"])
            .args(self.flags(agent))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = String::from(
            "{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"initialize\",\"params\":{}}\n",
        );
        for (n, params) in calls.iter().enumerate() {
            let call =
                json!({"jsonrpc": "2.0", "id": n + 1, "method": "tools/call", "params": params});
            input.push_str(&format!("{call}\n"));
        }
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = ended(child);
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .skip(1)
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["result"].clone())
            .collect()
    }

    /// One wake (`run --driver address`) as `agent`, firing through the
    /// way in at `way_in`: its output, once it returned.
    fn wake(&self, agent: &str, way_in: &Path) -> Output {
        let child = link_test_kit::env::command(PROGRAM, &self.dir)
            .arg("run")
            .args(self.flags(agent))
            .args(["--way-in", way_in.to_str().unwrap(), "--timeout", "30s"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        ended(child)
    }

    fn posts(&self) -> Vec<Value> {
        self.posts.lock().unwrap().clone()
    }
}

/// `child`'s output once it returned, which it must within 30 seconds.
fn ended(child: std::process::Child) -> Output {
    let pid = child.id();
    let (done, waited) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(child.wait_with_output());
    });
    match waited.recv_timeout(Duration::from_secs(30)) {
        Ok(output) => output.unwrap(),
        Err(_) => {
            // SAFETY: a signal to the test's own child.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            panic!("lnk-scheduler-tool didn't return");
        }
    }
}

/// The processes whose command line names `dir`: none left by a wake.
#[cfg(target_os = "linux")]
fn running_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.parse::<u32>().is_ok())
        })
        .filter_map(|entry| {
            let line = std::fs::read(entry.path().join("cmdline")).ok()?;
            command_names_dir(&line, dir).then(|| String::from_utf8_lossy(&line).replace('\0', " "))
        })
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn running_in(_dir: &Path) -> Vec<String> {
    Vec::new()
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

fn call(name: &str, arguments: Value) -> Value {
    json!({"name": name, "arguments": arguments, "_meta": {"link.local/standing": "owner", "link.local/thread": "telegram:42"}})
}

/// Each call that changes the jobs posts when they're next due, proved
/// as the agent: a schedule made, its time; another made later, still
/// the soonest; one cancelled, the other's; the last cancelled, none. A
/// `list` posts nothing.
#[test]
fn the_tool_posts_when_the_jobs_are_next_due() {
    let machine = set_up("address-tool");
    let results = machine.serve(
        AGENT,
        &[
            call(
                "schedule",
                json!({"prompt": "Stretch.", "cron": "0 0 1 1 *", "recurring": false}),
            ),
            call(
                "schedule",
                json!({"prompt": "Lunch.", "cron": "0 0 1 1 *", "recurring": false}),
            ),
            call("list", json!({})),
        ],
    );
    assert!(
        results.iter().all(|r| r.get("isError").is_none()),
        "{results:?}"
    );
    let ids: Vec<String> = results[..2]
        .iter()
        .map(|r| r["structuredContent"]["id"].as_str().unwrap().to_string())
        .collect();
    let store = Store::new(&machine.state).unwrap();
    let soonest = store.get(&ids[0]).unwrap().due;
    assert!(soonest >= now() + 3590, "{soonest}");
    assert_eq!(
        machine.posts(),
        [json!({"next": soonest}), json!({"next": soonest})]
    );
    let later = store.get(&ids[1]).unwrap().due;
    machine.serve(AGENT, &[call("cancel", json!({"id": ids[0]}))]);
    machine.serve(AGENT, &[call("cancel", json!({"id": ids[1]}))]);
    assert_eq!(
        machine.posts()[2..],
        [json!({"next": later}), json!({"next": null})]
    );
}

/// A wake fires what's due through the way in, posts when the rest are
/// next due (none: null), and returns, nothing of it left running.
#[test]
fn a_wake_fires_whats_due_posts_the_next_and_leaves_nothing() {
    let machine = set_up("address-wake");
    let socket = machine.dir.join("way-in.sock");
    let seen = way_in(&socket, &machine.keys);
    let store = Store::new(&machine.state).unwrap();
    let later = now() + 3600;
    store.put(&job("aaaaaaaaaaq1", now() - 1)).unwrap();
    store.put(&job("aaaaaaaaaaq2", later)).unwrap();
    let woke = machine.wake(AGENT, &socket);
    assert!(woke.status.success(), "{woke:?}");
    let fired = seen.lock().unwrap().clone();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0]["from"], "schedule:aaaaaaaaaaq1");
    assert_eq!(machine.posts(), [json!({"next": later})]);
    assert_eq!(store.jobs().len(), 1);
    assert!(store.firing().is_empty());
    assert!(
        running_in(&machine.dir).is_empty(),
        "{:?}",
        running_in(&machine.dir)
    );
    // Its last job fired: none left, null.
    store.cancel("aaaaaaaaaaq2").unwrap();
    store.put(&job("aaaaaaaaaaq3", now() - 1)).unwrap();
    let woke = machine.wake(AGENT, &socket);
    assert!(woke.status.success(), "{woke:?}");
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert_eq!(machine.posts()[1..], [json!({"next": null})]);
    assert!(store.jobs().is_empty() && store.firing().is_empty());
    assert!(running_in(&machine.dir).is_empty());
}

/// A message to a teammate on no way in of the machine's goes to the
/// machine, as this agent's, to carry: the way in isn't asked, the wake
/// doesn't wait for an answer, and none is put back, since the
/// teammate's comes as a message of its own.
#[test]
fn a_message_to_a_teammate_elsewhere_goes_to_the_machine() {
    let machine = set_up("address-teammate");
    let socket = machine.dir.join("way-in.sock");
    let seen = way_in(&socket, &machine.keys);
    let store = Store::new(&machine.state).unwrap();
    let teammate = "cccccccccccc";
    store
        .put(&Job {
            kind: Kind::Send,
            thread: link_scheduler_tool::teammates_thread(AGENT),
            text: "Draft the summary?".into(),
            answer: AnswerTo::Thread("telegram:42".into()),
            to: Some(link_scheduler_tool::Peers {
                from: AGENT.into(),
                to: teammate.into(),
            }),
            ..job("aaaaaaaaaar1", now() - 1)
        })
        .unwrap();
    let woke = machine.wake(AGENT, &socket);
    assert!(woke.status.success(), "{woke:?}");
    assert!(seen.lock().unwrap().is_empty(), "the way in was asked");
    let posts = machine.posts();
    assert_eq!(posts.len(), 2, "{posts:?}");
    let sent = &posts[0]["firing"];
    assert_eq!(sent["to"], teammate);
    assert_eq!(sent["firing"]["from"], format!("agent:{AGENT}"));
    assert_eq!(sent["firing"]["text"], "Draft the summary?");
    assert_eq!(sent["firing"]["thread"], format!("agent-{AGENT}"));
    assert_eq!(posts[1], json!({"next": null}));
    assert!(store.jobs().is_empty() && store.firing().is_empty());
}

/// The scheduler proves its posts with its own agent's key only: run as
/// another agent, its post is refused, and it says so; the job is kept.
#[test]
fn a_post_proved_as_another_agent_is_refused() {
    let machine = set_up("address-other");
    let results = machine.serve(
        OTHER,
        &[call(
            "schedule",
            json!({"prompt": "Stretch.", "cron": "0 0 1 1 *", "recurring": false}),
        )],
    );
    // Kept, so a success that warns, never an error inviting it again.
    assert_ne!(results[0]["isError"], true, "{results:?}");
    let said = results[0]["structuredContent"]["warning"].as_str().unwrap();
    assert!(said.contains("wasn't told"), "{said}");
    assert!(said.contains("isn't bound here"), "{said}");
    assert!(machine.posts().is_empty());
    let store = Store::new(&machine.state).unwrap();
    assert_eq!(store.jobs().len(), 1);
    // A wake as the other: refused too, and it fails.
    let socket = machine.dir.join("way-in.sock");
    way_in(&socket, &machine.keys);
    let woke = machine.wake(OTHER, &socket);
    assert!(!woke.status.success());
    assert!(
        String::from_utf8_lossy(&woke.stderr).contains("isn't bound here"),
        "{woke:?}"
    );
    assert!(machine.posts().is_empty());
}

/// The driver and its address go together.
#[test]
fn the_address_driver_needs_its_address() {
    let machine = set_up("address-flags");
    let run = |flags: &[String]| {
        let output = link_test_kit::env::command(PROGRAM, &machine.dir)
            .arg("run")
            .args(flags)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    let mut flags = machine.flags(AGENT);
    let at = flags.iter().position(|f| f == "--socket").unwrap();
    flags.drain(at..at + 2);
    assert!(run(&flags).contains("--socket"));
}

#[cfg(target_os = "linux")]
fn command_names_dir(line: &[u8], dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    line.split(|b| *b == 0)
        .any(|arg| Path::new(std::ffi::OsStr::from_bytes(arg)).starts_with(dir))
}

#[cfg(target_os = "linux")]
#[test]
fn process_checks_keep_sibling_fixture_prefixes_separate() {
    let dir = Path::new("/fixture/t1");
    assert!(command_names_dir(
        b"scheduler\0--state\0/fixture/t1/state\0",
        dir
    ));
    assert!(command_names_dir(b"scheduler\0/fixture/t1\0", dir));
    assert!(!command_names_dir(
        b"scheduler\0--state\0/fixture/t10/state\0",
        dir
    ));
    assert!(!command_names_dir(
        b"scheduler\0--state\0/fixture/t1-other/state\0",
        dir
    ));
}

//! The jobs kept through the store bridge (`--store-bridge`), as on a
//! machine that runs the agent only while it answers: the bridge is
//! `link_store_contract::store::serve_bridge` over `lnk-sqlite-store` (from `link-core`
//! beside this folder, built there), its file standing in for the store
//! off the machine. The jobs fire, move on and are cancelled as in the
//! folder, nothing of them in `--state`; the next machine the agent is
//! bound to reads them, a firing cut off by its sleep included; each
//! call is proved as the agent, so another's are refused; and through
//! the program, the tool and a wake on the address driver keep them
//! there.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use link_connection_contract::{AnswerTo, Fired, Firing};
use link_sandbox_binding::Identity;
use link_sandbox_binding::Keyring;
use link_scheduler_tool::store::Bridge;
use link_scheduler_tool::{
    Caller, Caps, Driver, InProcess, Job, JobStore, Kind, Place, Store, look, now, wake,
};
use link_store_contract::store::Driver as StoreDriver;
use link_test_kit::TempDir;
use serde_json::{Value, json};

/// The agent the scheduler is for, and another bound on the machine.
const AGENT: &str = "aaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbb";

const PROGRAM: &str = env!("CARGO_BIN_EXE_lnk-scheduler-tool");

const CAPS: Caps = Caps {
    per_day: 100,
    subagents: 5,
    hops: 8,
    late_runs: true,
    timeout: None,
    interval: None,
};

/// `lnk-sqlite-store`, from `link-core` beside this folder, built there.
fn lnk_sqlite_store() -> PathBuf {
    let core = std::env::var_os("LNK_CORE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    link_test_kit::programs_in(&core).join("lnk-sqlite-store")
}

/// A machine the agent is bound to: its keyring, its store bridge (over
/// `lnk-sqlite-store` keeping its file in `kept`, the store off every machine)
/// and its state folder.
struct Machine {
    keys: PathBuf,
    socket: PathBuf,
    state: PathBuf,
}

impl Machine {
    /// Machine `name` in `dir`: `AGENT` bound there with `key`, and
    /// `OTHER` too.
    fn new(dir: &Path, name: &str, kept: &Path, key: &str) -> Self {
        let keys = dir.join(name).join("keys");
        let keyring = Keyring::at(&keys);
        for agent in [AGENT, OTHER] {
            let identity = Identity {
                agent: agent.into(),
                service: String::new(),
                owner: "me@box".into(),
                teams: Vec::new(),
            };
            keyring
                .bind(&identity, &link_secretive::Secret::from(key.to_string()))
                .unwrap();
        }
        let socket = dir.join(name).join("store.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let driver = StoreDriver::at("sqlite", lnk_sqlite_store(), kept);
        let served = Keyring::at(&keys);
        std::thread::spawn(move || {
            link_store_contract::store::serve_bridge(
                listener,
                Arc::new(served),
                Arc::new(move |_: &Identity| Ok(driver.clone())),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )
        });
        Self {
            keys,
            socket,
            state: dir.join(name).join("state"),
        }
    }

    /// The jobs at its store bridge, called as `agent`.
    fn bridge(&self, agent: &str) -> Bridge {
        Bridge::new(
            &self.socket,
            Caller {
                agent: agent.into(),
                keyring: Keyring::at(&self.keys),
            },
        )
    }

    /// The store: its jobs at the bridge, as `agent`, the rest in its
    /// state folder.
    fn store(&self, agent: &str) -> Store {
        Store::with(&self.state, Arc::new(self.bridge(agent))).unwrap()
    }
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

fn recording() -> (Arc<dyn Driver>, Arc<Mutex<Vec<Firing>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    let driver: Arc<dyn Driver> = Arc::new(InProcess(move |firing: &Firing| {
        kept.lock().unwrap().push(firing.clone());
        Ok(Fired {
            answer: "ok".into(),
        })
    }));
    (driver, seen)
}

/// Through the store bridge, the jobs fire, move on, are cancelled and
/// come back after a restart as in the folder; what's due is the
/// store's answer; nothing of them is in the state folder.
#[test]
fn jobs_at_the_store_bridge_fire_as_in_the_folder() {
    let dir = TempDir::new("sched-bridge");
    let kept = dir.join("kept");
    let machine = Machine::new(&dir, "one", &kept, link_store_contract::testing::TEST_KEY);
    let store = machine.store(AGENT);
    assert!(store.jobs().is_empty());
    assert_eq!(store.next().unwrap(), None);
    let (driver, seen) = recording();
    let every = Job {
        cron: Some("* * * * *".into()),
        ..job("aaaaaaaaaar1", 600)
    };
    store.put(&every).unwrap();
    store.put(&job("aaaaaaaaaar2", 600)).unwrap();
    let task = Job {
        kind: Kind::Task,
        ..job("aaaaaaaaaar3", 5000)
    };
    store.put(&task).unwrap();
    assert_eq!((store.waiting(), store.subagents()), (3, 1));
    assert_eq!(store.next().unwrap(), Some(600));
    let changed = store.changed();
    assert!(changed.is_some());
    let looked = look(&store, &driver, &CAPS, 600, true);
    assert_eq!(looked.fired, ["aaaaaaaaaar1", "aaaaaaaaaar2"]);
    assert_eq!(looked.next, Some(5000));
    assert_ne!(store.changed(), changed);
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert_eq!(store.get("aaaaaaaaaar1").unwrap().due, 660);
    assert!(store.get("aaaaaaaaaar2").is_none());
    assert_eq!(store.sent_on(600), 2);
    assert!(store.cancel("aaaaaaaaaar1").unwrap());
    assert!(!store.cancel("aaaaaaaaaar1").unwrap());
    assert_eq!(store.next().unwrap(), Some(5000));
    // Cut off as it fired: back after a restart, counted as a try.
    let jobs = machine.bridge(AGENT);
    assert!(jobs.shift("aaaaaaaaaar3", Place::Waiting, Place::Firing));
    assert!(!jobs.shift("aaaaaaaaaar3", Place::Waiting, Place::Firing));
    assert_eq!(store.firing().len(), 1);
    assert_eq!(store.subagents(), 1);
    assert_eq!(store.next().unwrap(), None);
    store.recover();
    assert_eq!(store.get("aaaaaaaaaar3").unwrap().tries, 1);
    assert!(store.firing().is_empty());
    // Nothing of the jobs in the state folder; all in the store's file.
    for sub in ["jobs", "firing", "schedule"] {
        assert!(!machine.state.join(sub).exists(), "{sub}");
    }
    assert!(!machine.state.join("day.json").exists());
    assert!(kept.join("events.sqlite").exists());
}

/// A sleeping agent's jobs outlive its machine: the next machine it's
/// bound to, with a key of its own and a state folder of its own, reads
/// them, its day's count too, and moves back the one the last was firing
/// when the agent slept; the last machine's key proves nothing any more.
#[test]
fn the_next_machine_reads_a_sleeping_agents_jobs() {
    let dir = TempDir::new("sched-bridge-next");
    let kept = dir.join("kept");
    let first = Machine::new(&dir, "first", &kept, link_store_contract::testing::TEST_KEY);
    let store = first.store(AGENT);
    let (driver, _) = recording();
    store.put(&job("aaaaaaaaaan1", 100)).unwrap();
    store.put(&job("aaaaaaaaaan2", 900)).unwrap();
    store.put(&job("aaaaaaaaaan3", 950)).unwrap();
    assert_eq!(look(&store, &driver, &CAPS, 100, true).fired.len(), 1);
    assert_eq!(store.sent_on(100), 1);
    // Asleep in the middle of a firing: the job left being fired, and the
    // agent unbound.
    assert!(
        first
            .bridge(AGENT)
            .shift("aaaaaaaaaan3", Place::Waiting, Place::Firing)
    );
    Keyring::at(&first.keys).unbind(AGENT, &link_secret_contract::keeper::RouteRevoker);
    assert!(store.jobs().is_empty());
    assert!(store.next().is_err());
    // Bound next on another machine: its jobs and count are there; the
    // one cut off waits again, counted as a try, and both fire.
    let next = Machine::new(&dir, "next", &kept, &"c".repeat(64));
    let store = next.store(AGENT);
    assert_eq!(store.next().unwrap(), Some(900));
    assert_eq!(store.sent_on(120), 1);
    let (driver, seen) = recording();
    let looked = wake(&store, &driver, &CAPS, 1000).unwrap();
    assert_eq!(looked.fired, ["aaaaaaaaaan2", "aaaaaaaaaan3"]);
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert!(store.jobs().is_empty() && store.firing().is_empty());
    // The one cut off fired again as a try, which spends none of the
    // day's: a message counts once, where it was first fired (here it
    // was moved to firing by hand, so the day's count holds the two
    // first tries).
    assert_eq!(store.sent_on(1000), 2);
    assert_eq!(store.next().unwrap(), None);
}

/// Each call is proved as the agent the store bridge answers for: one
/// made as another agent is that agent's own jobs, none of this one's;
/// one whose agent isn't bound is refused, and a put is an error.
#[test]
fn each_agents_jobs_are_its_own() {
    let dir = TempDir::new("sched-bridge-own");
    let kept = dir.join("kept");
    let machine = Machine::new(&dir, "one", &kept, link_store_contract::testing::TEST_KEY);
    let store = machine.store(AGENT);
    store.put(&job("aaaaaaaaaao1", 100)).unwrap();
    let other = machine.store(OTHER);
    assert!(other.jobs().is_empty());
    assert_eq!(other.next().unwrap(), None);
    assert!(!other.cancel("aaaaaaaaaao1").unwrap());
    assert_eq!(store.jobs().len(), 1);
    // Unbound: refused, read as none, a put an error.
    Keyring::at(&machine.keys).unbind(OTHER, &link_secret_contract::keeper::RouteRevoker);
    let err = other.put(&job("aaaaaaaaaao2", 100)).unwrap_err();
    assert!(format!("{err:#}").contains("isn't bound"), "{err:#}");
    assert!(other.next().is_err());
    assert_eq!(store.jobs().len(), 1);
}

/// Through the program: `serve --store-bridge` keeps the jobs the tool
/// makes at the store bridge and posts when they're next due; a wake
/// (`run --driver address --store-bridge`) fires them from there; the
/// state folder holds only the machine's locks. A bridge without `--as`
/// is refused.
#[test]
fn the_tool_and_a_wake_keep_the_jobs_at_the_store_bridge() {
    let dir = TempDir::new("sched-bridge-program");
    let kept = dir.join("kept");
    let machine = Machine::new(&dir, "one", &kept, link_store_contract::testing::TEST_KEY);
    let address = dir.join("machine.sock");
    let posts = posts_at(&address, &machine.keys);
    let flags: Vec<String> = [
        "--state",
        machine.state.to_str().unwrap(),
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
        address.to_str().unwrap(),
        "--as",
        AGENT,
        "--keyring",
        machine.keys.to_str().unwrap(),
        "--store-bridge",
        machine.socket.to_str().unwrap(),
    ]
    .map(String::from)
    .to_vec();
    // The tool: a schedule kept at the bridge, its time posted.
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "schedule",
            "arguments": {"prompt": "Stretch.", "cron": "0 0 1 1 *", "recurring": false},
            "_meta": {"link.local/standing": "owner", "link.local/thread": "telegram:42"}
        }
    });
    let mut child = link_test_kit::env::command(PROGRAM, &dir)
        .arg("serve")
        .args(["--floor", "owner"])
        .args(&flags)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"initialize\",\"params\":{{}}}}\n{call}\n"
            )
            .as_bytes(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let store = machine.store(AGENT);
    let jobs = store.jobs();
    assert_eq!(jobs.len(), 1, "{output:?}");
    assert_eq!(
        posts.lock().unwrap().clone(),
        [json!({"next": jobs[0].due})]
    );
    // Due now: a wake fires it from the bridge and posts none left.
    store
        .put(&Job {
            due: now() - 1,
            ..jobs[0].clone()
        })
        .unwrap();
    let way_in = dir.join("way-in.sock");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let fired = seen.clone();
    link_agent_contract::testing::stand_in_bridge(
        &way_in,
        &machine.keys,
        move |who, firing: Value| {
            assert_eq!(who.agent, AGENT);
            fired.lock().unwrap().push(firing);
            json!({"answer": "Done."})
        },
    );
    let woke = link_test_kit::env::command(PROGRAM, &dir)
        .arg("run")
        .args(&flags)
        .args(["--way-in", way_in.to_str().unwrap(), "--timeout", "30s"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(woke.status.success(), "{woke:?}");
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(store.jobs().is_empty() && store.firing().is_empty());
    assert_eq!(posts.lock().unwrap()[1..], [json!({"next": null})]);
    // The state folder: the machine's locks, no job.
    let mut held: Vec<String> = std::fs::read_dir(&machine.state)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    held.sort();
    assert_eq!(
        held,
        [
            "definitions.lock",
            "jobs-control.lock",
            "post.lock",
            "runner.lock"
        ]
    );
    // Refused: jobs in an environment, and a bridge called as no one.
    let refused = |flags: &[String]| {
        let output = link_test_kit::env::command(PROGRAM, &dir)
            .arg("run")
            .args(flags)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    let service: Vec<String> = flags
        .chunks(2)
        .filter(|pair| pair[0] != "--driver" && pair[0] != "--socket")
        .flatten()
        .cloned()
        .collect();
    let without_as: Vec<String> = service
        .chunks(2)
        .filter(|pair| pair[0] != "--as")
        .flatten()
        .cloned()
        .collect();
    assert!(refused(&without_as).contains("--as"));
}

/// The machine's address at `socket`: each post proved as `AGENT` kept
/// and answered `{}`.
fn posts_at(socket: &Path, keys: &Path) -> Arc<Mutex<Vec<Value>>> {
    let posts = Arc::new(Mutex::new(Vec::new()));
    let kept = posts.clone();
    link_agent_contract::testing::stand_in_bridge(socket, keys, move |who, action: Value| {
        if who.agent != AGENT {
            return json!({"error": "not this agent's"});
        }
        kept.lock().unwrap().push(action);
        json!({})
    });
    posts
}

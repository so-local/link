//! The real runner projects portable definitions and authenticates each firing.
use link_connection_contract::Firing;
use link_sandbox_binding::{Identity, Keyring, Limits};
use link_test_kit::{ChildGroup, TempDir, env, wait};
use serde_json::json;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

const AGENT: &str = "aaaaaaaaaaaa";
const PROGRAM: &str = env!("CARGO_BIN_EXE_lnk-scheduler-tool");

struct Bridge {
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
    seen: Arc<Mutex<Vec<Firing>>>,
}

impl Bridge {
    fn new(socket: &Path, keys: &Path) -> Option<Self> {
        let listener = match std::os::unix::net::UnixListener::bind(socket) {
            Ok(listener) => listener,
            Err(err) => {
                link_test_kit::gate::lacks("Unix sockets", err, None);
                return None;
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (stopping, observing) = (stop.clone(), seen.clone());
        let keys = Keyring::at(keys);
        let thread = std::thread::spawn(move || {
            let limits = Limits {
                name: "portable schedule fixture",
                calls: 1,
                per_agent: 1,
                per_owner: 1,
                action_max: link_sandbox_binding::CALL_MAX,
                action_wait: Duration::from_secs(30),
                answer_wait: Duration::from_secs(30),
            };
            link_sandbox_binding::serve(
                listener,
                Arc::new(keys),
                &limits,
                &stopping,
                move |who, firing: Firing| {
                    assert_eq!(who.agent, AGENT);
                    observing.lock().unwrap().push(firing);
                    Ok(json!({"answer":"delivered"}))
                },
            )
        });
        Some(Self {
            socket: socket.into(),
            stop,
            thread: Some(thread),
            seen,
        })
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = std::os::unix::net::UnixStream::connect(&self.socket);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn wake(root: &Path, definitions: &Path, socket: &Path) -> (bool, String) {
    let mut command = env::command(PROGRAM, root);
    command
        .args(["run", "--driver", "cron", "--state"])
        .arg(root.join("state"))
        .arg("--definitions")
        .arg(definitions)
        .arg("--way-in")
        .arg(socket)
        .args(["--as", AGENT, "--keyring"])
        .arg(root.join("keys"))
        .args([
            "--time-zone",
            "UTC",
            "--per-day",
            "100",
            "--subagents",
            "5",
            "--hops",
            "8",
            "--late",
            "run",
            "--timeout",
            "30s",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let mut child = ChildGroup::spawn(&mut command).unwrap();
    let status = wait::eventually("portable runner exits", Duration::from_secs(30), || {
        child.try_status().unwrap()
    });
    let mut error = String::new();
    child
        .take_stderr()
        .unwrap()
        .read_to_string(&mut error)
        .unwrap();
    (status.success(), error)
}

#[test]
fn published_native_schedule_fires_through_the_authenticated_bridge_once_across_restarts() {
    let root = TempDir::short("sp");
    let keys = root.join("keys");
    Keyring::at(&keys)
        .bind(
            &Identity {
                agent: AGENT.into(),
                service: String::new(),
                owner: "owner@fixture".into(),
                teams: Vec::new(),
            },
            &link_secretive::Secret::from("fixture-secret".repeat(4)),
        )
        .unwrap();
    let socket = root.join("way.sock");
    let Some(bridge) = Bridge::new(&socket, &keys) else {
        return;
    };
    let at = link_scheduler_tool::now();
    let file = root.join("portable.json");
    let mut definitions = json!({"schedules": [{"id":"openclaw:inbox","name":"Morning inbox",
        "when":{"kind":"at","at":at},"message":"Summarise client mail",
        "to":{"kind":"thread","thread":"telegram:-42.7"},"enabled":true,"revision":1,
        "origin":{"adapter":"openclaw","job":"inbox"}}], "schema": 1});
    std::fs::write(&file, serde_json::to_vec(&definitions).unwrap()).unwrap();
    for _ in 0..2 {
        let (ok, error) = wake(&root, &file, &socket);
        assert!(ok, "{error}");
    }
    let first = bridge.seen.lock().unwrap().clone();
    assert_eq!(first.len(), 1);
    assert!(first[0].thread.starts_with("schedule-"));
    assert_eq!(
        first[0].answer,
        link_connection_contract::AnswerTo::OwnerThread("telegram:-42.7".into())
    );
    assert_eq!(first[0].text, "Summarise client mail");
    assert!(first[0].from.starts_with("schedule:"));
    assert_eq!(
        first[0].occurrence.as_ref().unwrap().schedule.0,
        "openclaw:inbox"
    );
    assert_eq!(first[0].occurrence.as_ref().unwrap().scheduled_at, at);
    definitions["schedules"][0]["revision"] = 2.into();
    std::fs::write(&file, serde_json::to_vec(&definitions).unwrap()).unwrap();
    let (ok, error) = wake(&root, &file, &socket);
    assert!(ok, "{error}");
    let seen = bridge.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_ne!(seen[0].occurrence, seen[1].occurrence);
}

#[test]
fn malformed_published_definitions_never_reach_the_bridge() {
    let root = TempDir::short("sp-bad");
    let file = root.join("portable.json");
    std::fs::write(&file, b"{broken").unwrap();
    let Some(bridge) = Bridge::new(&root.join("way.sock"), &root.join("keys")) else {
        return;
    };
    let (ok, _) = wake(&root, &file, &bridge.socket);
    assert!(!ok);
    assert!(bridge.seen.lock().unwrap().is_empty());
}

//! An idle agent holding nothing: where this machine's one bridge carries
//! it ([`crate::bridges`]), an agent its spec lets stop when idle (`[run]
//! idle_after`) holds no process, memory or port of its own between
//! messages. The bridge listens on the agent's endpoint port for it, its
//! front door ([`Door`]): each connection is passed into the agent's
//! sandbox through its proxy's folder (`in.sock`), as its proxy would,
//! and an agent asleep is started first. With no byte through the door,
//! no answer still coming through it (however long the answer pauses:
//! [`exchange`]), no turn running and no background command for
//! `idle_after`, the agent's job is stopped: its service, or, in a
//! terminal, the start's child.
//!
//! On Linux, for an agent sandboxed behind its proxy, its proxy serves
//! its endpoint into the sandbox through its folder, so the bridge serves
//! it in the proxy's place, awake or asleep, and sees every byte. Any
//! other (a Mac, where the harness listens on the loopback itself, or an
//! agent with no proxy) is handed its port as it wakes ([`Sleep::handoff`]):
//! the bridge listens only while it's asleep, lets go of the port at the
//! first connection, starts it, and passes that connection on once it
//! listens; a connection's connection meanwhile is refused and tried again
//! (`link_connection_contract`). Awake, it's idle with no connection to its
//! port open, no turn running and no background command.

use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::bridge::Carrying;

mod exchange;
use exchange::Exchange;
mod jobs;
mod listening;
mod reaching;
pub use jobs::run_in_terminal;
use jobs::{start_job, start_shared, stop_job};
pub use reaching::forget;
use reaching::{answers, busy, connected, pipe_tcp, proxy_runs, reach_inside, set_mark};

/// How an agent that stops when idle is woken and put to sleep: what its
/// record with the bridge says ([`crate::bridges::Record`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sleep {
    /// Its endpoint's port, which the bridge serves for it.
    pub port: u16,
    /// Its proxy's folder: `in.sock`, into its sandbox, and `out.sock`,
    /// there while its proxy runs.
    pub net: PathBuf,
    /// How long it may be idle, in milliseconds (`[run] idle_after`).
    pub after_ms: u64,
    /// Its background commands' records (`run/`): one whose lock is held
    /// is running.
    pub run: PathBuf,
    /// Its conversations: one holding `turn.json` has a turn running.
    pub conversations: PathBuf,
    /// How its job is stopped and started.
    pub job: Job,
    /// Whether its harness listens on its port itself: the bridge then
    /// holds the port only while it's asleep, and hands it over as it
    /// wakes.
    #[serde(default)]
    pub handoff: bool,
    /// The harness its owner's agents share, which it's served by, if it
    /// is: started again when it wakes and none runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<Shared>,
}

/// Where an agent's job runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Job {
    /// Its service: its launchd label and systemd unit.
    Service { label: String, unit: String },
    /// Its start in a terminal, told through this socket.
    Terminal(PathBuf),
}

/// The harness an owner's agents share ([`Sleep::shared`]): its folder,
/// its service, and the command it runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shared {
    pub dir: PathBuf,
    pub unit: String,
    pub argv: Vec<String>,
}

/// The mark of an agent asleep: `~/.lnk/safe/agents/bridges/asleep/<agent id>`,
/// so the harness its owner's agents share keeps serving it.
fn mark_of(key: &str) -> PathBuf {
    crate::bridges::dir().join("asleep").join(key)
}

/// Whether the agent keyed `key` is asleep, its job stopped for being
/// idle, to start at its next message.
pub fn asleep(key: &str) -> bool {
    mark_of(key).exists()
}

/// An agent's front door while the bridge carries it.
pub struct Door {
    state: Arc<State>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

struct State {
    key: String,
    sleep: Sleep,
    carrying: Arc<Carrying>,
    /// Its background commands' sweep, run as it wakes.
    sweeper: Option<Vec<String>>,
    /// Held while it's woken or put to sleep: whether it's asleep.
    asleep: Mutex<bool>,
    /// Connections open through the door now, by a number of its own:
    /// ended as it stops, an idle one kept open by its client among them.
    open: Mutex<std::collections::BTreeMap<usize, Open>>,
    /// Numbers them.
    opened: AtomicUsize,
    /// When a byte last went through the door, or it last woke.
    last: Mutex<Instant>,
    stop: AtomicBool,
}

/// A connection open through the door: to end as the agent stops, and
/// what it carries, whether an answer is still coming on it.
struct Open {
    client: TcpStream,
    exchange: Arc<Mutex<Exchange>>,
}

/// Whether an answer is still coming on any of `open`.
fn answering(open: &std::collections::BTreeMap<usize, Open>) -> bool {
    open.values().any(|open| {
        open.exchange
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .answering()
    })
}

/// How often an agent is looked at for being idle.
const LOOK: Duration = Duration::from_secs(1);

// How long a woken agent may take to answer: the connection contract's,
// which a bridge's challenge waits as long for.
use link_connection_contract::constants::WAKING;

impl Door {
    /// Opens agent `key`'s front door: its port listened on, and a look
    /// each second whether it's been idle long enough to stop. Whether
    /// it's asleep now is read from its proxy: none running, it is.
    pub fn open(
        key: &str,
        sleep: Sleep,
        carrying: Arc<Carrying>,
        sweeper: Option<Vec<String>>,
    ) -> Self {
        Self::open_on(key, sleep, None, carrying, sweeper)
    }

    /// [`Door::open`], on `bound` when given: its port's listener, bound
    /// already (by a test, to a port none can take in between). An agent
    /// handed its port binds it only while it's asleep, and takes none.
    fn open_on(
        key: &str,
        sleep: Sleep,
        bound: Option<TcpListener>,
        carrying: Arc<Carrying>,
        sweeper: Option<Vec<String>>,
    ) -> Self {
        // One handed its port is taken for awake, and stopped once idle.
        let awake = sleep.handoff || proxy_runs(&sleep.net);
        set_mark(key, !awake);
        let state = Arc::new(State {
            key: key.to_string(),
            sleep,
            carrying,
            sweeper,
            asleep: Mutex::new(!awake),
            open: Mutex::new(Default::default()),
            opened: AtomicUsize::new(0),
            last: Mutex::new(Instant::now()),
            stop: AtomicBool::new(false),
        });
        let mut threads = Vec::new();
        let listening = state.clone();
        threads.push(std::thread::spawn(move || listening.listen(bound)));
        let watching = state.clone();
        threads.push(std::thread::spawn(move || watching.watch()));
        // What it finished that nobody took, once it answers.
        if awake {
            let asking = state.clone();
            std::thread::spawn(move || asking.recover_soon());
        }
        Self { state, threads }
    }

    /// Closes it: its port let go, at once, for the agent's next start,
    /// and no more looks. A connection already in goes on.
    pub fn close(self) {
        self.state.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.state.sleep.port));
        for thread in self.threads {
            let _ = thread.join();
        }
    }
}

//! Link carrying the connections for a harness that speaks none: each
//! connected connection whose module carries messages runs as
//! `lnk-<connection> bridge`, outside the sandbox, beside the agent while it
//! runs. It sends the owner's messages to the harness's endpoint and the
//! answers back, so the harness never holds the connection's token.
//!
//! What `lnk agent start` decides (the endpoint, its key, the connections)
//! is kept in [`Plan`], with the agent's settings, where no harness
//! reaches; the background service (`lnk agent bridge`) or a foreground
//! start reads it and keeps a bridge running per connection.

use link_format::WithinExt;
use std::{
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

use crate::settings::{self, Settings};
mod asking;
mod bridging;
mod carrying;
mod holding;
mod outbox;
mod storage;
mod team_files;
mod way_in;
pub use asking::{forget_owner_reader, owner_reader};
pub use bridging::{carried, forget, keep_out, link, links, module_state, request, run, state};
use bridging::{keep, make_state, section};
#[cfg(test)]
use carrying::listen_storage;
pub use carrying::{carry, pause_mark, paused, quiet_mark, unpause};
use carrying::{real_folders, sleep_unless};
use holding::give_held_until;
pub use holding::{
    HELD, deliver_held, for_generation, generation, generation_file, give_held, held_file,
    keep_held, set_generation,
};
pub use storage::StoragePlan;
use team_files::serve_team_files;
pub use team_files::{FILES_SOCKET, TeamFilesPlan};
use way_in::{RETRY_FIRST, RETRY_MOST};

/// Send through the configured connection's recipient checks, including
/// connections the harness carries itself. Credentials stay in this process.
pub fn send_owner(agent: &Settings, thread: &str, text: &str) -> anyhow::Result<()> {
    let (connection, _) = thread
        .split_once(':')
        .within("a connection thread is required")?;
    outgoing(agent, connection)?.send_owner(thread, text)
}

/// Send to the owner's own chat on a selected service.
pub fn send_connection(agent: &Settings, connection: &str, text: &str) -> anyhow::Result<()> {
    let connection = connection_name(connection);
    outgoing(agent, connection)?.send_connection(connection, text)
}

fn connection_name(connection: &str) -> &str {
    if connection == crate::chat::TERMINAL {
        crate::chat::CONNECTION
    } else {
        connection
    }
}

fn outgoing(agent: &Settings, connection: &str) -> anyhow::Result<WayIn> {
    let plan = Plan::load(&agent.name)?;
    let mut way_in = WayIn::new(agent, &plan);
    if connection != crate::chat::CONNECTION {
        let request =
            request(agent, &plan, connection).within("the connection is not connected")?;
        let program = link_core::find_module(connection).within("the connection is unavailable")?;
        way_in
            .connections
            .insert(connection.into(), (program, request));
    }
    Ok(way_in)
}

/// A scheduled turn explicitly sent its content through the agent tool.
/// Its final acknowledgement must not also be posted to that destination.
pub fn sent_in_schedule(
    agent: &Settings,
    thread: &str,
    from: &str,
    connection: &str,
) -> anyhow::Result<()> {
    let plan = Plan::load(&agent.name)?;
    WayIn::new(agent, &plan).sent_in_schedule(thread, from, connection)
}

link_files::specification!(Plan, include_str!("bridge/plan.schema.toml"));

/// How an agent's connections are carried.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// The harness's chat completions endpoint.
    pub endpoint: String,
    /// What its endpoint takes (`Authorization: Bearer`), made at each
    /// start.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub key: link_secretive::Secret,
    /// Whether its endpoint proves it's the harness before it's sent the
    /// key, as its adapter's `contract` says: then the bridges send the
    /// key only to a connection that proved it.
    pub proof: bool,
    /// The connections carried (`telegram`, `slack`, ...).
    pub connections: Vec<String>,
    /// Whether its endpoint reads who a message is from, as its
    /// adapter's `contract` says: then the bridge takes messages from
    /// others at its way in, and runs the agent's scheduler.
    pub from: bool,
    /// Whether its endpoint hands over the answers of turns it finished
    /// after a crash ([`link_connection_contract::acp::RECOVER`]), as its
    /// adapter's `contract` says: then the bridge asks for them and sends
    /// each where its turn's answer was to go.
    pub recover: bool,
    /// Whether its endpoint keeps a conversation's tools, as its
    /// adapter's `contract` says: then a message from others may narrow
    /// its conversation's tools (a subagent's). No: one that would is
    /// refused.
    pub tools: bool,
    /// Whether its endpoint takes a message's images and documents, as
    /// its adapter's `contract` says: then each connection downloads what a
    /// message holds and sends it.
    pub parts: bool,
    /// The storage bridge it runs, for a harness that keeps Link's
    /// collections (Link Harness): each read and write of them, through
    /// it, answered by the driver of the place its machine keeps that
    /// collection at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<StoragePlan>,
    /// The files bridge it runs, for an agent on a team whose folder an
    /// account hosting agents keeps: each call on that folder handed to
    /// the folder's service with the agent's own token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<TeamFilesPlan>,
}

/// The store bridge's socket, in the harness's own folder.
pub const STORE_SOCKET: &str = "link-store.sock";

/// The agent's notes, the one file its memory's `remember` writes.
pub const NOTES: &str = link_harness_contract::harness::layout::NOTES;

/// Where `owner`'s agent keyed `key` has its memory reach its storage
/// bridge: a socket of its own, alone in a folder (made private) of the
/// runtime folder its owner's memory service is told and checks a
/// binding against ([`link_files::layout::MemoryBridges`]), which
/// memory's sandbox reads and every agent's sandbox hides. The folder,
/// not the socket, is what memory's sandbox is given: a bridge started
/// again makes a new socket there.
pub fn memory_storage(owner: &str, key: &str) -> PathBuf {
    link_files::layout::MemoryBridges::of(owner).socket(key)
}

impl Plan {
    /// Where agent `agent`'s plan is kept.
    pub fn path(agent: &str) -> PathBuf {
        settings::config_of(agent).join("bridge.json")
    }

    pub fn load(agent: &str) -> anyhow::Result<Self> {
        let path = Self::path(agent);
        let bytes = std::fs::read(&path).within(format!(
            "reading {} (lnk agent start writes it)",
            path.display()
        ))?;
        link_files::saved::parse_json(&bytes).within(format!("reading {}", path.display()))
    }

    /// Saves it readable only by the user: it holds the endpoint's key.
    pub fn save(&self, agent: &str) -> anyhow::Result<()> {
        let path = Self::path(agent);
        link_files::write_private(&path, &link_files::saved::print_json(self)?)
            .within(format!("writing {}", path.display()))
    }

    /// Forgets agent `agent`'s plan: nothing to carry.
    pub fn remove(agent: &str) {
        let _ = std::fs::remove_file(Self::path(agent));
    }
}

/// What carries one agent's connections and messages from others, while it
/// does: its threads, and the connections' bridges they keep running. One
/// process can carry many agents' at once, each its own ([`carry`]), and
/// stop one without the others ([`Carrying::stop`]).
pub struct Carrying {
    stop: Arc<AtomicBool>,
    /// Set when its connections and its way in stop taking messages: as it
    /// stops, or once the agent is paused ([`pause_mark`]).
    quiet: Arc<AtomicBool>,
    /// Each connection's bridge running now, to stop.
    connections: Vec<Arc<Mutex<Option<Running>>>>,
    /// The scheduler's runner it keeps, if it does, to end.
    runner: Option<crate::kept::Kept>,
    /// The way in it listens at, if it does.
    way_in: Option<PathBuf>,
    /// The store bridge's socket, if it runs one.
    store: Option<PathBuf>,
    /// What asks the harness for the answers nobody took, when it hands
    /// them over.
    recovery: Option<WayIn>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

/// A connection's bridge, running: its stdin, held open while it runs, so
/// it ends once that closes.
struct Running {
    stdin: std::process::ChildStdin,
}

/// What the way in takes at once and waits for
/// ([`link_sandbox_binding::serve`]): each message is held until the
/// agent's answer went, so many at once, a few of them one sender's.
pub const WAY_IN: link_sandbox_binding::Limits = link_sandbox_binding::Limits {
    name: "the way in",
    calls: 64,
    per_agent: 16,
    per_owner: 64,
    action_max: 16 << 20,
    action_wait: Duration::from_secs(10),
    answer_wait: Duration::from_secs(30),
};

/// The bridge's way in: messages for the agent from someone who isn't
/// its owner ([`link_connection_contract::Firing`]), each a bridge's call
/// ([`link_sandbox_binding`]) on a Unix socket only the user reaches
/// ([`link_connection_contract::way_in`]), answered with one line once the
/// answer went where it goes: a connection's chat, another conversation, or
/// nowhere. Each is proved as the agent's whose scheduler or tool hands
/// it ([`link_connection_contract::Terms`]).
pub struct WayIn {
    interaction_agent: String,
    interactions: Option<link_interactions::Store>,
    /// Whose calls it takes, and their keys: this machine's keyring.
    keys: Arc<dyn link_sandbox_binding::Keys>,
    /// Where each conversation's answer goes, while it's owed
    /// ([`Owed`]).
    owed: PathBuf,
    /// What reaches the harness's endpoint.
    endpoint: link_connection_contract::Bridge,
    /// The terms it takes a message on: the agent, whether the harness
    /// keeps a conversation's tools ([`Plan::tools`]), and the tools a
    /// stranger's message may use (`[tools] strangers`), none when the
    /// agent's spec doesn't resolve.
    terms: link_connection_contract::Terms,
    /// Each connection carried, to send an answer nobody waits for.
    connections: BTreeMap<String, (PathBuf, link_connection_contract::Bridge)>,
    /// Where the teams' files are: a message from another agent is taken
    /// only from a member of a team this one is on, as they say now.
    teams: PathBuf,
    /// Where an answer in a `link` thread nobody waits for is held, for
    /// the next chat on it ([`crate::chat`]).
    held: crate::chat::Held,
    /// How many messages each teammate sent this agent today, a file
    /// each ([`Received`]), and how many it takes from one a day: its
    /// own scheduler's `per_day`, the cap on what it sends; none when the
    /// spec doesn't resolve, so nothing from a teammate is taken.
    received: PathBuf,
    per_day: Option<u32>,
    /// How many agents' messages a conversation takes since a person
    /// last spoke in it ([`link_interactions::chains`]): its scheduler's
    /// `hops`; none when the spec doesn't resolve, so none is taken.
    hops: Option<u32>,
    /// Where those chains are counted: none when the machine's
    /// interactions' folder can't be found, so none is taken.
    chains: Option<link_interactions::chains::Chains>,
    /// One count at a time, so two at once never both take the last.
    counting: Arc<std::sync::Mutex<()>>,
}

/// A teammate's messages taken on one day (UTC): [`WayIn`]'s cap on what
/// an agent is sent, as its scheduler caps what it sends.
type Received = link_connection_contract::Received;

/// What became of agent `agent`'s held messages, as each was given.
#[derive(Debug, Default)]
pub struct Given {
    /// Given to it, and its answer sent.
    pub given: usize,
    /// Still kept: it couldn't be reached, or its answer couldn't go.
    pub waiting: usize,
    /// Never to be given here (a connection Link doesn't carry for it, a
    /// sender it isn't paired with), and why: for the caller to show
    /// whole, kept only when it asked.
    pub refused: Vec<(link_agent_place::Held, String)>,
}

/// Where an answer nobody waits for goes, kept with the bridge's settings
/// (`~/.lnk/safe/agents/<agent>/owed/`) from the firing that asked it
/// until it went.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Owed {
    from: String,
    hops: u32,
    answer: link_connection_contract::AnswerTo,
    /// The standing of the firing that asked it, which its answer keeps.
    standing: link_connection_contract::access::Standing,
    #[serde(default)]
    sent: bool,
}

/// For a test: owed nowhere, as a stranger.
#[cfg(test)]
impl Default for Owed {
    fn default() -> Self {
        Owed {
            from: String::new(),
            hops: 0,
            answer: link_connection_contract::AnswerTo::default(),
            standing: link_connection_contract::access::Standing::Stranger,
            sent: false,
        }
    }
}

/// A message the harness has taken, its turn running: its answer is
/// still to be read and sent where `owed` says.
pub(crate) struct Taken {
    thread: String,
    owed: Owed,
    accepted: link_connection_contract::Accepted,
}

#[cfg(test)]
mod tests;

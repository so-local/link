//! The contract between the harness module (`lnk-agent`) and a harness
//! adapter (`lnk-harness`, or any `lnk-<name>` from outside Link).
//!
//! An adapter is a program answering these commands:
//!
//! ```text
//! lnk-<name> contract     prints [`Contract`] as JSON: the version of this contract it
//!                         speaks ([`PROTOCOL`]), and what it answers about itself. An
//!                         adapter without it speaks version 0, and answers no to each
//!                         question in it
//! lnk-<name> install      install the harness, or leave it; no questions asked.
//!                         Installing, it says so first ("Installing <Name> into
//!                         <dir> (first time only)...") and shows its progress
//! lnk-<name> configure    [`Settings`] as JSON on stdin -> the harness's own config.
//!                         Silent unless it fails (`run_quiet`): Link says
//!                         "Setting up <name>..." for every harness
//! lnk-<name> command      prints [`Launch`] as JSON: how to run it in the foreground
//! lnk-<name> health       exits 0 when the running harness answers
//! lnk-<name> internet-hosts prints a JSON list of the hosts its `install` and `configure`
//!                         reach (`["registry.npmjs.org", "*.githubusercontent.com"]`, as
//!                         [`crate::net::check_host`] takes them): behind their proxy,
//!                         they reach no other. Without it, any host on the internet
//! lnk-<name> traces       prints a JSON list of [`traces::Trace`]: each place the harness
//!                         writes when it isn't in its home, and what kind of thing is
//!                         there (`{"kind": "home", "path": "~/.<name>"}`), for finding
//!                         and migrating an install, `lnk agent harness show` and
//!                         `lnk agent delete`
//! lnk-<name> state        prints [`State`] as JSON: what in its home is the agent (its
//!                         settings, memory, conversations, skills) rather than what its
//!                         installer put there, for `lnk agent move`
//! lnk-<name> connections     prints a JSON list of the connections (`lnk-<connection>` modules) its harness
//!                         speaks (`["telegram", "slack", "discord"]`), for `lnk agent connect`
//!                         and `lnk agent harness set`; `configure` leaves out any other
//! lnk-<name> link <connection> [--again]
//!                         for a linked connection (`lnk-<connection> describe`), whose account the harness
//!                         links itself (Signal: from a QR code on the terminal): links it,
//!                         in its home, once configured; exits 0 at once when it's linked
//!                         already (`--again`: links it anew), and 3 when it isn't and
//!                         there's no terminal to link it on
//! lnk-<name> uninstall --parts --json
//!                         [`crate::uninstall::Kept`], run outside any home: what its
//!                         harness keeps in each agent's home ([`homes`]), its
//!                         credentials for `lnk secret list`
//! lnk-<name> needs        [`Settings`] as JSON on stdin (no keys or tokens) -> prints
//!                         [`Needs`] as JSON: what its harness needs beyond Link's
//!                         defaults (permissions, no sandbox at all, or a risk of its
//!                         own), each with why, which `lnk agent` asks the user for
//!                         before it runs. Without it (an error exit, or no answer),
//!                         nothing more
//! lnk-<name> endpoint     for a harness that speaks no connection itself: prints, as a JSON
//!                         string, the address of its ACP on this machine's loopback
//!                         (`"http://127.0.0.1:18800/acp"`), at one of its
//!                         `Launch.ports`. Link then carries the connections whose
//!                         modules carry messages (`describe`'s `bridged`) for it: each
//!                         message a prompt alone (never the thread's history), on a
//!                         WebSocket opened with `Authorization: Bearer
//!                         <Settings.endpoint_key>` and the agent in `Link-Agent`
//!                         ([`crate::connection::AGENT_HEADER`]), its session the thread.
//!                         An endpoint whose `contract` says `proof` first proves it's the
//!                         harness, on the same connection: `GET` [`PROOF_PATH`] with a
//!                         random [`CHALLENGE_HEADER`], answered with [`PROOF_HEADER`],
//!                         [`endpoint_proof`] of it with the key; one that can't gets neither
//!                         the key nor the message
//! ```
//!
//! **Every harness lives in its home**: one folder Link gives it, in its
//! agent's (`~/.lnk/data/agents/<agent>/harnesses/<name>`). `lnk-agent` runs `install`, `configure`,
//! `command` and `health` with `HOME` set to that home and, unless the user
//! turned the sandbox off, inside a sandbox that lets them write only the
//! home (with `network`, behind a proxy of their own), so an adapter finds and puts everything under
//! [`crate::home_dir`] and the harness can't spread across the machine.
//!
//! **Ports come from `LNK_PORT`.** A machine runs several agents, of
//! several users, so `lnk-agent` gives every command of an adapter the
//! agent's base port in `LNK_PORT` ([`base_port`]), every agent's own,
//! `main`'s too. An adapter puts its ports at and after it (within
//! [`PORT_BLOCK`]) and says so in `Launch.ports`; its own default is only
//! for running it outside Link, and one that ignores `LNK_PORT` is
//! refused.
//!
//! `lnk-agent` owns everything that stays the same when the harness
//! changes: the settings, the home, the background service, the sandbox,
//! switching.

use std::{collections::BTreeMap, path::PathBuf};

use link_secretive::{Duplicate, Secret};
use serde::{Deserialize, Serialize};

pub mod authority;
mod binding;
pub mod continuation;
pub mod features;
pub mod identity;
pub mod namespaces;
pub mod progress;
pub mod schedules;
pub mod switching;
pub mod traces;
pub mod transcript;

pub use binding::{BIND_MAX, Bind, BindAgent, Bound, bind};
mod needs;
mod settings;
pub use needs::{MAX_RISK_NAME, MAX_WHY, Need, Needs, Risk, Why, is_risk_name};
pub use settings::{
    Discord, Environment, FIRST_CONNECTIONS, Model, NOT_CONNECTIONS, Runtime, Settings, Signal,
    Slack, TeamPlace, Teammate, Telegram, default_model,
};

/// The harness protocol, as `harness/protocol.toml` declares it: the
/// version of this contract `lnk-agent` and the adapters built with this
/// crate speak, which [`Contract::version`] says, and the oldest an agent
/// still starts. A change to the contract advances it. An adapter that
/// doesn't answer `needs` needs nothing more, one that names no risk is
/// asked about none, one whose `contract` leaves out `proof` isn't asked
/// for one, one that leaves out `agent` answers every request as its own
/// agent's, and one that leaves out `blank` is never started blank.
pub static PROTOCOL: std::sync::LazyLock<link_version::Protocol> = std::sync::LazyLock::new(|| {
    link_version::Protocol::shipped(include_str!("harness/protocol.toml"))
});

// The tools' addresses Link hands a harness in its environment.

/// The variable holding the agent's own folder (`~/Link/Agents/<name>/agent`,
/// [`crate::folder`]) in its harness's environment, beside `LNK_FILES`,
/// its Home: where a harness that keeps Link's collections (Link Harness)
/// finds each one kept at `file`.
pub const AGENT_FOLDER_ENV: &str = "LNK_AGENT_FOLDER";

/// The variable holding the address of the agent's `run` tool (Link
/// run, its commands each in a sandbox of its own), beside
/// [`crate::memory::MEMORY_ENV`], for a harness that keeps to the agent's folder's
/// layout (`Contract::folders`).
pub const RUN_ENV: &str = "LNK_RUN";

/// The variable holding the address of the agent's `file` tool (Link
/// file, its files read and written as its sandbox file says), beside
/// [`RUN_ENV`].
pub const FILE_ENV: &str = "LNK_FILE";

/// The variable holding the address of the agent's `code` tool (Link
/// code, its project's language servers, each in a sandbox), beside
/// [`RUN_ENV`].
pub const CODE_ENV: &str = "LNK_CODE";

/// The namespaces of Link's own tools an agent hands its harness, each
/// at `LNK_TOOL_<NAME>`: no other tool is given one of these names. What
/// each one's actions reach is [`namespaces`]'.
pub const NAMESPACES: [&str; 6] = ["task", "cron", "tool", "file", "agent", "question"];

/// Those of [`NAMESPACES`] whose session a harness keeps between turns:
/// each is Link's run, file, code and scheduler, which keep nothing of a
/// conversation in a session (each call says its conversation).
pub const KEPT_NAMESPACES: [&str; 3] = ["task", "cron", "file"];

/// What `lnk-<name> contract` prints.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Contract {
    /// The version of this contract it speaks ([`PROTOCOL`]'s current
    /// when built with this crate), 0 for an adapter without `contract`.
    pub version: u32,
    /// Whether its harness reaches a hosted model at
    /// [`Model::base_url`] when that's set: then Link's proxy can hold the
    /// model's key, and the harness only a placeholder (keys on the wire,
    /// unless the agent says otherwise). No: the harness holds the key.
    #[serde(default)]
    pub proxy: bool,
    /// Whether its `endpoint` answers Link's challenge before it's handed
    /// the key ([`PROOF_PATH`], [`endpoint_proof`]): then Link's connections
    /// send the key and the message only to a connection that proved it's
    /// the harness. No: they send them to whatever answers on its port.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub proof: bool,
    /// Whether its `endpoint` reads which agent a request is for
    /// ([`crate::connection::AGENT_HEADER`]): it answers a request naming an
    /// agent it runs as that agent's, one naming none as its own agent's
    /// (`LNK_AGENT`), and refuses one naming any other, before the model
    /// sees it. No: it answers every request as its own agent's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent: bool,
    /// How many ports its harness takes at and after its agent's base
    /// port ([`PORT_ENV`]): Link gives the agent that many and one more.
    /// None: [`PORT_BLOCK`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ports: Option<u16>,
    /// Whether it takes the agent spec's `[instructions]`: an agent spec
    /// that sets them is refused for a harness that doesn't, rather than
    /// ignored.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub instructions: bool,
    /// Whether its `endpoint` reads who a message is from
    /// ([`crate::connection::acp::Said::from`]) and passes its hops on: then it
    /// may be sent messages from others (a schedule, a subagent), and is
    /// given the scheduler. On ACP, `_meta.link`'s `from`, `fromName`
    /// and `hops` ([`crate::connection::acp::Said`]). No: it's never
    /// sent one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from: bool,
    /// Whether its `endpoint` answers [`crate::connection::acp::RECOVER`]:
    /// the answers of turns it finished after a crash, which nobody
    /// waited for: each once, or, asked in two phases, kept until the
    /// bridge says it went. Then the agent's bridge asks for them and
    /// sends each where its turn's answer was to go. On ACP, the request
    /// [`crate::connection::acp::RECOVER`]. No: they stay in the
    /// conversation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recover: bool,
    /// Whether its `endpoint` keeps a conversation's tools
    /// ([`crate::connection::acp::Said::tools`]): a message naming tools narrows
    /// its conversation to those, for good, and a call to its tools says
    /// which the conversation has (`_meta` `link.local/tools`). Then a
    /// subagent may be given fewer tools than its parent. On ACP,
    /// `_meta.link`'s `tools`. No: a subagent
    /// has all its parent's, and is never sent a list.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tools: bool,
    /// Whether its `endpoint` takes a message's images and documents
    /// ([`crate::connection::Part`]), as OpenAI's content parts, or on
    /// ACP as image blocks and embedded resources: then each
    /// connection downloads what a message holds and sends it. No: a message
    /// holding only them is answered that the agent reads text only, and
    /// none is ever sent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parts: bool,
    /// Whether it keeps to the agent's folder's layout ([`Folders`],
    /// [`crate::folder`]): it writes only `chats/` of the agent's
    /// own folder itself, so its sandbox reads Home and that folder, and
    /// writes only its conversations and the folders the agent's sandbox
    /// file lets it (`[files] write`). No: it writes the whole of Home,
    /// never reaches the agent's own folder, and `[files]` doesn't hold
    /// for it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub folders: bool,
    /// Whether it answers `lnk-<name> instructions`: it prints the system
    /// prompt it sends its model, as it would send it now (`--json`: with
    /// its sections), for `lnk agent show --instructions`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prompt: bool,
    /// Whether one process of it may serve several agents of one owner
    /// (`serve --agents <file>`, [`SharedAgents`]): each on its own
    /// port, as the agent its port is, with its own home, files and tools.
    /// Link runs it so only where the machine says
    /// (`agents.toml`'s `shared_harness`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
    /// Whether it answers `lnk-<name> conversation <thread>`, printing a
    /// conversation as a person reads it wherever it's kept, and
    /// `lnk-<name> conversations move`, moving each conversation kept
    /// another way than the agent's spec says (`[storage] conversations`)
    /// to that way: for `lnk agent chat show` and `lnk agent
    /// conversations move`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub conversations: bool,
    /// Whether `conversations list --json` and `conversation <thread>
    /// --json` return the remote API's typed link threads and messages.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub conversation_history: bool,
    /// Whether `conversations fork <thread> --before <seq>` copies the
    /// events before an owner's message into a fresh link thread and
    /// returns its typed history, preserving tools and the source log.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub conversation_fork: bool,
    /// Whether it can start blank, for a machine that runs an agent only
    /// while it answers: `<command> --control <control>`, its `command`'s
    /// argv with those two added, loads nothing of any agent's and
    /// answers on no port; it listens on the Unix socket `<control>` (its
    /// mode 0600) until one [`Bind`] line arrives, answers it with
    /// [`Bound`], and then serves that agent alone, on its usual port, as
    /// a start for it would. A second `Bind` is refused, and nothing
    /// unbinds it: the process ends when the agent sleeps.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blank: bool,
    /// What its endpoint's agent state exports and imports over ASP
    /// ([`link_agent_state_protocol::AgentState`]), as its `initialize` advertises: what
    /// `lnk agent harness set` carries in and out of it, named before a switch
    /// ([`Contract::left_behind`]). None, from an adapter before it.
    #[serde(
        default,
        skip_serializing_if = "link_agent_state_protocol::AgentState::is_empty"
    )]
    pub state: link_agent_state_protocol::AgentState,
    /// Whether `state export` reads portable state without a running endpoint.
    /// Native schemas and read consistency remain adapter-owned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub offline_state: bool,
    /// Non-text session content it can use after import. Session support
    /// alone guarantees text only; omitted kinds remain in snapshots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_content: Vec<switching::SessionContent>,
    /// Whether it reads the agent's notes (`memory/notes.md`) at each
    /// turn, where a switch writes what the agent remembered on other
    /// harnesses.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub notes: bool,
    /// Whether it keeps memory files of its own (`MEMORY.md`), which no
    /// other harness reads: a switch from it leaves them behind.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub native_memory: bool,
    /// Whether it carries who the agent is both ways ([`identity`]): it
    /// writes [`Settings::identity`] into its own files, reads the skills
    /// Link copies to [`identity::skills_relative()`], and answers `lnk-<name>
    /// state export` with what it holds of its own in ASP's portable
    /// forms ([`link_agent_state_protocol::State`]): its instructions, skills and memory.
    /// No: its persona and skills are its own, and the spec's stay behind.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub identity: bool,
    /// Whether it answers `lnk-<name> forget <home>`: handed secrets'
    /// values on stdin (a JSON list of strings), it takes each out of the
    /// files home `<home>` keeps them in of its own, other than its
    /// `secrets` traces, which Link edits itself. Link runs it, bringing an
    /// install in, on its copy of an install for each secret Link takes. No: such a
    /// copy keeps them until the harness's own configure takes them out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forget: bool,
    /// The features it honors, each how ([`features::Honors`]). None,
    /// from an adapter before the list: what its flags imply
    /// ([`Contract::honors`]).
    #[serde(
        default,
        deserialize_with = "features::read",
        skip_serializing_if = "Option::is_none"
    )]
    pub features: Option<features::Honors>,
}

/// What a harness serving several agents from one process serves
/// (`serve --agents <file>`, the file's JSON, naming its schema): one
/// owner's agents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedAgents {
    pub agents: Vec<SharedAgent>,
}

link_files::specification!(
    SharedAgents,
    include_str!("harness/shared_agents.schema.toml")
);

/// One agent of a [`SharedAgents`]: who it is, its harness's home, its
/// Home (`files`) and its own folder (`agent`), the port
/// its endpoint answers on, and the environment it would have had on its
/// own (`LNK_MEMORY`, `LNK_TOOL_<NAME>`, ...), which its tools come from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SharedAgent {
    pub name: String,
    pub id: String,
    pub owner: String,
    pub home: PathBuf,
    pub files: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<PathBuf>,
    pub port: u16,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}

impl Contract {
    /// This crate's version, with `proxy` as the adapter says, and no
    /// `proof` or `agent`.
    pub fn new(proxy: bool) -> Self {
        Self {
            version: PROTOCOL.current,
            proxy,
            ..Self::default()
        }
    }
}

/// `~/.lnk/data/agents/<agent>/harnesses`: one home per harness of agent
/// `agent`, each named by its harness, where its adapter runs it.
pub fn homes_of(agent: &str) -> std::path::PathBuf {
    link_files::layout::harness_homes_at(&link_files::installation_dir(), agent)
}

/// Each agent on this machine with a home for harness `harness`, by
/// name, and that home ([`homes_of`]): where an adapter finds what its
/// harness keeps, for `uninstall --parts`, run outside any home.
pub fn homes(harness: &str) -> Vec<(String, std::path::PathBuf)> {
    homes_in(&link_files::layout::Hidden::AgentData.path(), harness)
}

/// [`homes`] in the agents' folder `agents`. A link is never followed,
/// for an agent's folder, its harnesses' folder or the home itself: what's
/// listed is in the agents' folder, never where a link leads.
fn homes_in(agents: &std::path::Path, harness: &str) -> Vec<(String, std::path::PathBuf)> {
    let Ok(entries) = std::fs::read_dir(agents) else {
        return Vec::new();
    };
    let folder = |path: &std::path::Path| {
        std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
    };
    let mut homes: Vec<(String, std::path::PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|agent| {
            let dir = agents.join(&agent);
            let boxes = dir.join(link_files::layout::harness_homes_folder());
            let home = boxes.join(harness);
            let folders = [&dir, &boxes, &home].into_iter().all(|p| folder(p));
            folders.then_some((agent, home))
        })
        .collect();
    homes.sort();
    homes
}

/// The tools an adapter added to its harness's MCP servers, by name, as
/// its record (a file in its home) keeps them, naming its schema.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddedTools {
    tools: Vec<String>,
}

link_files::specification!(AddedTools, include_str!("harness/added_tools.schema.toml"));

/// The tools an adapter added to its harness's MCP servers, as `record`
/// lists them ([`tools_gone`]): none when it can't be read.
pub fn tools_added(record: &std::path::Path) -> Vec<String> {
    std::fs::read(record)
        .ok()
        .and_then(|bytes| link_files::saved::parse_json::<AddedTools>(&bytes).ok())
        .unwrap_or_default()
        .tools
}

/// Of the tools an adapter added to its harness's MCP servers before
/// (listed in `record`, a file in its home), those not in `tools` now,
/// to remove; `record` then lists `tools`. A server the user added
/// under another name is never touched.
pub fn tools_gone(
    record: &std::path::Path,
    tools: &BTreeMap<String, String>,
) -> anyhow::Result<Vec<String>> {
    let before = tools_added(record);
    let now = AddedTools {
        tools: tools.keys().cloned().collect(),
    };
    link_files::write_private(record, &link_files::saved::print_json(&now)?)?;
    Ok(before
        .into_iter()
        .filter(|name| !tools.contains_key(name))
        .collect())
}

/// What an adapter without native connections answers `link
/// <connection>`: a usage error, since it links none itself.
pub fn unsupported_link(connection: &str) -> link_format::Report {
    link_format::Report::error(link_format::sentence!(
        "This harness does not link {} itself.",
        link_format::Foreign::new(connection)
    ))
    .exit(link_format::Exit::Usage)
}

/// How to run a harness in the foreground. `lnk-agent` runs it, in its
/// sandbox, as a background service.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Launch {
    pub argv: Vec<String>,
    /// Added to a clean environment (PATH, HOME and the rest are set by
    /// `lnk-agent`). Never secrets: the service definition that holds
    /// them is readable by other local users. Secrets go in the harness's
    /// own files (see [`merge_env_file`]).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Folders it must write besides its home, which (like the files
    /// folder, and `/tmp/<name>*`) is always writable.
    #[serde(default)]
    pub write: Vec<PathBuf>,
    /// Folders outside its home it only reads.
    #[serde(default)]
    pub read: Vec<PathBuf>,
    /// Its own localhost ports (its gateway, its API): the only localhost
    /// ports it may connect to, besides a local model's.
    #[serde(default)]
    pub ports: Vec<u16>,
    /// Of `ports`, those Link reaches from outside the sandbox: its
    /// gateway or endpoint, its health check. Behind its proxy only these
    /// are served on this machine's loopback, where every local user can
    /// reach them; the others (a connection's daemon beside it, which asks
    /// for no credential) stay inside. None: all of `ports`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serve: Option<Vec<u16>>,
    /// Background services the harness installs for itself (launchd
    /// labels, systemd user units). They'd run outside the sandbox and
    /// fight over the bot token, so `lnk-agent` stops and removes them.
    #[serde(default)]
    pub services: Vec<String>,
    /// How long, in seconds, it may take to answer `health` after it
    /// starts, before `lnk agent start` gives up on it. None:
    /// [`WAIT_SECS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_secs: Option<u64>,
}

/// How long a harness may take to answer after it starts, unless its
/// [`Launch::wait_secs`] says otherwise.
pub const WAIT_SECS: u64 = 60;

impl Launch {
    /// The ports served outside the sandbox: [`Launch::serve`], or all.
    pub fn served(&self) -> Vec<u16> {
        self.serve.clone().unwrap_or_else(|| self.ports.clone())
    }

    /// How long it may take to answer after it starts.
    pub fn wait(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.wait_secs.unwrap_or(WAIT_SECS))
    }
}

/// Sets `KEY=value` lines in a `.env` file, keeping every other line, and
/// leaves it readable only by this user. A `None` value removes the key.
/// A value with a control character (a newline could add a line of its
/// own) is refused, and the file is left as it was.
pub fn merge_env_file(
    path: &std::path::Path,
    pairs: &[(&str, Option<&str>)],
) -> std::io::Result<()> {
    for (key, value) in pairs {
        if value.is_some_and(|v| v.chars().any(char::is_control)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("the value for {key} has a control character (a newline?)"),
            ));
        }
    }
    let old = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err),
    };
    let ours = |line: &str| {
        let key = line.trim_start().trim_start_matches("export ");
        pairs
            .iter()
            .any(|(k, _)| key.split('=').next().is_some_and(|name| name.trim() == *k))
    };
    let mut text: String = old
        .lines()
        .filter(|line| !ours(line))
        .map(|line| format!("{line}\n"))
        .collect();
    for (key, value) in pairs {
        if let Some(value) = value {
            // Quoted, so a key with `#` or spaces survives dotenv parsers.
            let value = value.replace('\\', "\\\\").replace('"', "\\\"");
            text += &format!("{key}=\"{value}\"\n");
        }
    }
    link_files::write_private(path, text.as_bytes())
}

/// [`crate::random_token`], where adapters found it first.
pub use link_files::random_token;

/// The variable holding an agent's base port, in every adapter command's
/// environment when the agent has one.
pub const PORT_ENV: &str = "LNK_PORT";

/// How many ports from the base port are an agent's when its harness
/// doesn't say ([`Contract::ports`]): its harness's ports are all in
/// `base..base + PORT_BLOCK`.
pub const PORT_BLOCK: u16 = 200;

/// The agent's base port (`LNK_PORT`), else the adapter's `default`.
pub fn base_port(default: u16) -> u16 {
    port_or(std::env::var(PORT_ENV).ok().as_deref(), default)
}

/// `value` as a base port, else `default`: one with its whole block of
/// unprivileged ports.
fn port_or(value: Option<&str>, default: u16) -> u16 {
    value
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| (1024..=u16::MAX - PORT_BLOCK).contains(port))
        .unwrap_or(default)
}

/// Whether `GET http://127.0.0.1:<port><path>` answers 200: a harness's
/// health endpoint.
pub fn http_ok(port: u16, path: &str) -> bool {
    use std::io::{Read, Write};
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_secs(2),
    ) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let request = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).is_ok() && head.starts_with(b"HTTP/1.") && &head[9..12] == b"200"
}

/// A harness's official installer script, pinned: fetched from its
/// project's repository at a fixed commit, and run only if its SHA-256 is
/// this one.
pub struct Installer {
    /// The harness's name, for messages.
    pub name: &'static str,
    /// Where the script is, at a fixed commit (never a moving URL).
    pub url: &'static str,
    /// The script's SHA-256, hex.
    pub sha256: &'static str,
    /// Other pinned files the script sources from its own directory.
    pub companions: &'static [InstallerFile],
}

/// Another pinned file an [`Installer`] needs beside its main script.
pub struct InstallerFile {
    /// The single path component the installer opens.
    pub name: &'static str,
    /// Where the file is, at the same fixed commit as the installer.
    pub url: &'static str,
    /// The file's SHA-256, hex.
    pub sha256: &'static str,
}

/// Downloads `installer` and its companions into a private directory in
/// the harness's home, checks them, and runs the main script with `bash`
/// and `args`. Nothing runs if any download isn't the pinned file.
pub fn run_installer(installer: &Installer, args: &[&std::ffi::OsStr]) -> anyhow::Result<()> {
    run_installer_with(installer, args, &[])
}

/// [`run_installer`], the script also given the variables `env`, each a
/// name and its value: an adapter's settings for the tools its installer
/// runs, such as where they install what they download.
pub fn run_installer_with(
    installer: &Installer,
    args: &[&std::ffi::OsStr],
    env: &[(&str, &std::ffi::OsStr)],
) -> anyhow::Result<()> {
    let home = link_files::home_dir();
    std::fs::create_dir_all(&home)?;
    let dir = home.join(format!("lnk-installer-{}", std::process::id()));
    let script = dir.join("install.sh");
    let _ = std::fs::remove_dir_all(&dir);
    let result = (|| {
        std::fs::create_dir(&dir)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        let bytes = link_runtime::block_on(link_artifact::Cache::shared()?.bytes(
            installer.url,
            installer.sha256,
            5 * 1024 * 1024,
        ))?;
        link_files::write_private(&script, &bytes)?;
        verify(installer.name, installer.sha256, &std::fs::read(&script)?)?;
        for companion in installer.companions {
            let path = companion_path(&dir, companion.name)?;
            let bytes = link_runtime::block_on(link_artifact::Cache::shared()?.bytes(
                companion.url,
                companion.sha256,
                5 * 1024 * 1024,
            ))?;
            link_files::write_private(&path, &bytes)?;
            verify(companion.name, companion.sha256, &std::fs::read(path)?)?;
        }
        run(trusting_machine_cas(
            link_secretive::process::command("bash")
                .arg(&script)
                .args(args)
                .envs(env.iter().copied()),
            SYSTEM_CAS,
        ))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// A companion remains beside the installer: never an absolute path or
/// a path that can leave its private directory.
fn companion_path(dir: &std::path::Path, name: &str) -> anyhow::Result<std::path::PathBuf> {
    let mut components = std::path::Path::new(name).components();
    anyhow::ensure!(
        matches!(components.next(), Some(std::path::Component::Normal(_)))
            && components.next().is_none(),
        "the pinned installer companion name {name:?} isn't a file name"
    );
    Ok(dir.join(name))
}

/// Where this machine keeps the CAs it trusts, by distribution: a proxy
/// that re-signs TLS is trusted there, and never among the CAs Node and
/// uv bring with them.
pub const SYSTEM_CAS: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/cert.pem",
];

/// `command`, trusting this machine's CAs as well as its tools' own: Node's
/// (`NODE_EXTRA_CA_CERTS`) and uv's, Python's and curl's (`SSL_CERT_FILE`),
/// each the first of `cas` there is, unless `command` is given its own.
pub fn trusting_machine_cas<'a>(
    command: &'a mut std::process::Command,
    cas: &[&str],
) -> &'a mut std::process::Command {
    let Some(bundle) = cas.iter().find(|p| std::path::Path::new(p).is_file()) else {
        return command;
    };
    for variable in ["NODE_EXTRA_CA_CERTS", "SSL_CERT_FILE"] {
        let given = command
            .get_envs()
            .any(|(k, v)| k == variable && v.is_some());
        if !given {
            command.env(variable, bundle);
        }
    }
    command
}

/// Whether `bytes` are the pinned installer file.
fn verify(name: &str, expected: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let actual = link_digest::sha256(bytes);
    anyhow::ensure!(
        actual == expected,
        "the {} installer Link downloaded isn't the one it pins (SHA-256 {actual}, \
         expected {}); not running it. A network problem or a rate limit can cause this: \
         try again later",
        name,
        expected
    );
    Ok(())
}

/// Runs `command`, failing with its name if it doesn't succeed. Output
/// goes to the terminal.
pub fn run(command: &mut std::process::Command) -> anyhow::Result<()> {
    let status = command
        .status()
        .map_err(|err| anyhow::anyhow!("running {command:?}: {err}"))?;
    anyhow::ensure!(status.success(), "{command:?} failed ({status})");
    Ok(())
}

/// Runs `command` without showing its output, unless it fails: then the
/// output is printed and the error names the command.
pub fn run_quiet(command: &mut std::process::Command) -> anyhow::Result<()> {
    let output = command
        .output()
        .map_err(|err| anyhow::anyhow!("running {command:?}: {err}"))?;
    if !output.status.success() {
        use std::io::Write;
        let mut passed = link_format::streams::passed(link_format::streams::Stream::Err);
        let _ = passed.write_all(&output.stdout);
        let _ = passed.write_all(&output.stderr);
        anyhow::bail!("{command:?} failed ({})", output.status);
    }
    Ok(())
}

/// The value of `key` in a `.env` file, if set.
pub fn read_env_file(path: &std::path::Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().find_map(|line| {
        let (name, value) = line
            .trim_start()
            .trim_start_matches("export ")
            .split_once('=')?;
        (name.trim() == key).then(|| value.trim().trim_matches('"').to_string())
    })
}

/// This program's size and modification time, or nothing when unknown.
fn adapter_build() -> String {
    std::env::current_exe()
        .and_then(std::fs::metadata)
        .map(|m| {
            let modified = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            format!("{}:{modified}", m.len())
        })
        .unwrap_or_default()
}

/// What an adapter's last `configure` was given and left, kept as a hash
/// in its home, so a start that would change nothing skips the harness's
/// own CLI (a Node or Python start per setting). The hash covers the
/// settings as given on stdin, the adapter's and its harness's versions,
/// `LNK_PORT`, the adapter program as built, and the files `configure`
/// writes as it left them: a harness that edits its own config, a home
/// that moved without them, or a rebuilt adapter is configured again.
pub struct Configured {
    stamp: PathBuf,
    version: String,
    files: Vec<PathBuf>,
}

impl Configured {
    /// For an adapter at `version` (its own and its harness's), writing
    /// `files`: its stamp is `.lnk/data/harnesses/configured` in the home.
    pub fn new(version: &str, files: Vec<PathBuf>) -> Self {
        Self::at(
            link_files::state::path(&link_files::home_dir(), link_files::state::Area::Configured),
            version,
            files,
        )
    }

    /// With its stamp at `stamp`.
    pub fn at(stamp: PathBuf, version: &str, files: Vec<PathBuf>) -> Self {
        Self {
            stamp,
            version: version.to_string(),
            files,
        }
    }

    fn hash(&self, input: &[u8]) -> String {
        let mut all = Vec::new();
        let mut part = |bytes: &[u8]| {
            all.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            all.extend_from_slice(bytes);
        };
        part(self.version.as_bytes());
        // The adapter itself, as built: a dev build is one version however
        // often it changes.
        part(adapter_build().as_bytes());
        part(std::env::var(PORT_ENV).unwrap_or_default().as_bytes());
        part(input);
        for file in &self.files {
            part(file.as_os_str().as_encoded_bytes());
            match std::fs::read(file) {
                Ok(bytes) => part(&bytes),
                Err(_) => part(b"\0missing"),
            }
        }
        link_digest::sha256(&all)
    }

    /// Whether `configure` with `input` would find everything as the last
    /// one left it.
    pub fn unchanged(&self, input: &[u8]) -> bool {
        std::fs::read_to_string(&self.stamp).is_ok_and(|s| s.trim() == self.hash(input))
    }

    /// Records a `configure` with `input` that finished.
    pub fn done(&self, input: &[u8]) -> std::io::Result<()> {
        link_files::write_private(&self.stamp, self.hash(input).as_bytes())
    }

    /// Forgets the last `configure`, before one starts (a failed one
    /// leaves nothing to skip) or after an install.
    pub fn forget(&self) {
        let _ = std::fs::remove_file(&self.stamp);
    }
}

pub use link_harness_base::*;

#[cfg(test)]
mod tests;

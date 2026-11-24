//! Everything the harness reads and writes, behind one seam: [`Store`],
//! keyed by agent, and by conversation for a conversation's. Its
//! settings and instructions, its conversations and where each one's
//! window starts: nothing else of an agent's is read or written but
//! through it.
//!
//! [`Files`] is its one implementation, the agent's folders on disk:
//! `config.json` in the harness's home (or, for a harness started blank
//! and bound to its agent, the settings it was bound with, held in
//! memory: [`Files::holding`]); the files the prompt is made of in the
//! agent's own folder (`instructions/instruction.md`); and a folder per
//! conversation there (`chats/`). A conversation's events are
//! kept one of three ways ([`Way`]), the one its agent's spec said when
//! it started (`[storage] conversations`), and read where they are:
//!
//! - **the log** (`log`): one file, `events.jsonl`, a line of JSON each,
//!   in order, only ever added to; and `conversation.md` beside it, a copy for people
//!   and memory, added to as the agent answers, derived from the events
//!   and rebuilt from them when it doesn't match.
//! - **compact** (`compact`): the same events in zstd frames,
//!   `events.zst`, and those since the last frame in `events.jsonl`
//!   ([`frames`]). No copy: `lnk agent chat show` prints one.
//! - **a store driver** (`sqlite`, any other name): the program
//!   `lnk-<name>`, run in the harness's sandbox, keeps them, and the
//!   conversation's lease ([`link_store_contract::store`]). Its folder holds an
//!   empty `store.<name>`, which says so.
//!
//! Where the prompt's window starts ([`crate::window::Mark`]) is kept in
//! each answer of the agent's, and read from the last few events. What
//! isn't an event is a file beside them, whatever the way:
//! `tools.json`, the tools a conversation was narrowed to (`Said::tools`),
//! a grant, as the settings are; a turn asked to end; an answer that
//! finished a crashed turn; but for a driver's, the turn's lease; and in
//! `parts/`, the images and documents its events name, each a file
//! (none where it's kept off this machine, with no folder here).
//! In the harness's home, beside the settings, `tool-lists.json` keeps
//! what each tool last listed ([`crate::tools::Lists`]), derived.
//!
//! Every folder is reached from the one before it, never through a link,
//! and a conversation's is held open ([`Held`]) while it's read and
//! written: a folder swapped for a link meanwhile (by a `run` call) is
//! refused, or changes nothing.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::{Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::bail;
use link_files::nofollow::{self, Dir, Kind, read_open};
use link_format::WithinExt;
use link_secretive::Duplicate;

/// What a failed read or write was doing, worded only if it failed.
pub(crate) struct When<F>(pub F);

impl<F: Fn() -> String> std::fmt::Display for When<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&(self.0)())
    }
}

mod chat;
mod copies;
mod deletion;
mod files;
mod firsts;
mod frames;
mod held;
mod instruction;
mod kept;
mod log;
mod moving;
mod places;
mod recent;
mod seen;
mod snapshot;
#[cfg(test)]
mod tests;

pub use copies::markdown;
use held::{Held, left_out, parted, write_in};
pub use instruction::InstructionRead;
use log::{ends_with, event_of, last_seq, log_last, read_log, recovered_in};
use seen::Logs;
pub use snapshot::Snapshot;

use crate::agent::{Agent, Agents, Thread};
use crate::config::Config;
use crate::event::Event;
use crate::tools::Lists;
use crate::window::Mark;
use link_harness_wire::skill::{self, Skill};
use link_store_contract::store::Appended;
use link_store_contract::store::copy::{self, Said};

/// An agent's settings, the files of its prompt and its conversations.
pub trait Store: Send + Sync {
    /// The trusted host holds the work's execution lease and has admitted an explicit owner scope.
    fn delete_work(
        &self,
        _agent: &Agent,
        _thread: &Thread,
        _scope: &link_interactions::control::DeleteScope,
    ) -> anyhow::Result<()> {
        anyhow::bail!("this store does not support scoped work deletion")
    }
    /// `agent`'s settings, as `configure` last kept them: an error when
    /// there are none.
    fn config(&self, agent: &Agent) -> anyhow::Result<Config>;

    /// Keeps `config` as `agent`'s settings. They may hold a key.
    fn set_config(&self, agent: &Agent, config: &Config) -> anyhow::Result<()>;

    /// A file of `agent`'s prompt, by its path in the agent's files
    /// (`instructions/instruction.md`, as its spec names it), at most
    /// [`INSTRUCTIONS_MAX`] bytes of it: none when it isn't there, or
    /// holds nothing.
    fn instruction(&self, agent: &Agent, path: &str) -> anyhow::Result<Option<String>> {
        Ok(self.instruction_read(agent, path)?.map(|read| read.text))
    }

    /// The same instruction with its observed source and returned-text receipt.
    fn instruction_read(
        &self,
        agent: &Agent,
        path: &str,
    ) -> anyhow::Result<Option<InstructionRead>>;

    /// `agent`'s skills, by name: each folder of `skills/` whose
    /// `SKILL.md` says its name and what it's for, as the standard does
    /// ([`link_harness_wire::skill`]), at most [`SKILLS_MAX`] of them. One that
    /// doesn't, or is a link, is left out.
    fn skills(&self, agent: &Agent) -> anyhow::Result<Vec<Skill>>;

    /// Every event of `agent`'s conversation `thread`, in order; none for
    /// a new one.
    fn read(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Vec<Event>> {
        self.read_from(agent, thread, 0)
    }

    /// The events numbered `first` and on, in order.
    fn read_from(&self, agent: &Agent, thread: &Thread, first: u64) -> anyhow::Result<Vec<Event>>;

    /// The first message of `thread` that `repeat` names: none when
    /// there's none. A message its source sends again is found as the one
    /// it repeats.
    fn first_of(
        &self,
        agent: &Agent,
        thread: &Thread,
        repeat: &Repeat,
    ) -> anyhow::Result<Option<Event>> {
        Ok(self
            .read(agent, thread)?
            .into_iter()
            .find(|event| repeat.names(event)))
    }

    /// A full conversation's retained events and the completeness the
    /// store can establish. Ordinary reads alone establish no completeness.
    fn snapshot(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Snapshot> {
        Ok(Snapshot::unknown(self.read(agent, thread)?))
    }

    /// Adds `event` as number `event.seq`, which must be the next: an
    /// error when that number is taken (another turn wrote it first).
    /// `before` is what the caller holds of the events before it, the
    /// last of them numbered `event.seq - 1`, or none: the copy for
    /// people follows from them, or from every event when they don't
    /// reach back far enough.
    fn append(
        &self,
        agent: &Agent,
        thread: &Thread,
        event: &Event,
        before: &[Event],
    ) -> anyhow::Result<()>;

    /// The last `n` events of `agent`'s conversation `thread`, in order:
    /// fewer when it holds fewer.
    fn last_events(&self, agent: &Agent, thread: &Thread, n: u64) -> anyhow::Result<Vec<Event>>;

    /// Where the window's walk resumes, as the agent's latest answer of
    /// the last [`MARK_EVENTS`] kept it: none when none did (a new
    /// conversation, or a long run of calls). It's derived from the
    /// events: lost, it costs only a read of every event.
    fn mark(&self, agent: &Agent, thread: &Thread) -> Option<Mark> {
        let events = self.last_events(agent, thread, MARK_EVENTS).ok()?;
        events.into_iter().rev().find_map(|e| e.window)
    }

    /// The conversations `agent` has, each kept any way: none for a
    /// store that doesn't list them.
    fn threads(&self, _agent: &Agent) -> anyhow::Result<Vec<Thread>> {
        Ok(Vec::new())
    }

    /// The tools `thread` may use, as a list narrowed it last: none when
    /// none did, and it has every tool of its agent's.
    fn tools(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Option<Vec<String>>>;

    /// Narrows `thread` to `tools`, of those it may use: a tool it may not
    /// use already stays out, so a list never widens it.
    fn narrow_tools(&self, agent: &Agent, thread: &Thread, tools: &[String]) -> anyhow::Result<()>;

    /// Takes `thread`'s turn for `holder`: its lease, which says to any
    /// process that a turn runs. [`Took::Held`] while another's lease is
    /// renewed; [`Took::Lapsed`] when another's stopped being renewed,
    /// as long ago as the agent's spec says a lease lasts (`[turns]
    /// lapse`): its turn crashed, and is this one's to finish.
    fn take_turn(&self, agent: &Agent, thread: &Thread, holder: &str) -> anyhow::Result<Took>;

    /// Renews `holder`'s lease: an error when it isn't `holder`'s any
    /// more (it lapsed, and another took it).
    fn renew_turn(&self, agent: &Agent, thread: &Thread, holder: &str) -> anyhow::Result<()>;

    /// Lets `holder`'s lease go, and any interruption asked of it.
    fn end_turn(&self, agent: &Agent, thread: &Thread, holder: &str) -> anyhow::Result<()>;

    /// Whether a turn runs on `thread`: a lease renewed lately.
    fn turn_running(&self, agent: &Agent, thread: &Thread) -> bool;

    /// Asks the turn running on `thread` to end now.
    fn interrupt_turn(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<()>;

    /// Whether the turn running on `thread` was asked to end.
    fn interrupted(&self, agent: &Agent, thread: &Thread) -> bool;

    /// `agent`'s conversations whose turn crashed: a lease that lapsed.
    fn lapsed_turns(&self, agent: &Agent) -> Vec<Thread>;

    /// Keeps that `thread`'s answer numbered `seq` finished a crashed
    /// turn, and nobody waited for it: the agent's bridge takes it
    /// ([`Self::take_recovered`]) and sends it to the chat.
    fn set_recovered(&self, agent: &Agent, thread: &Thread, seq: u64) -> anyhow::Result<()>;

    /// Each of `agent`'s answers that finished a crashed turn and wasn't
    /// taken yet, by its conversation and number; taken now, so each is
    /// handed over once.
    fn take_recovered(&self, agent: &Agent) -> Vec<(Thread, u64)>;

    /// [`Self::take_recovered`]'s answers, left in place until each is
    /// let go ([`Self::drop_recovered`]): handed over until it's sent.
    fn recovered(&self, agent: &Agent) -> Vec<(Thread, u64)>;

    /// Lets go of `thread`'s answer numbered `seq` that finished a
    /// crashed turn: it was sent. Nothing when another is kept since.
    fn drop_recovered(&self, agent: &Agent, thread: &Thread, seq: u64) -> anyhow::Result<()>;

    /// Each tool's list of calls as `agent`'s harness last listed it,
    /// derived: none when there are none, or they can't be read, and
    /// then each tool is only what it lists. None by default.
    fn tool_lists(&self, agent: &Agent) -> Option<Lists> {
        let _ = agent;
        None
    }

    /// Keeps `lists` for the next process to read: losing them costs
    /// only that a tool down when it starts is left out of its turns.
    /// Not kept by default.
    fn set_tool_lists(&self, agent: &Agent, lists: &Lists) -> anyhow::Result<()> {
        let _ = (agent, lists);
        Ok(())
    }

    /// Keeps `bytes`, an image or document of a message of `agent`'s
    /// conversation `thread`, as its `file` (`parts/<name>`), before the
    /// event that names it is written: an error where the conversation
    /// has no folder to keep it in. Kept nowhere by default.
    fn keep_part(
        &self,
        agent: &Agent,
        thread: &Thread,
        file: &str,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let _ = (agent, bytes);
        bail!("{} keeps no images or documents: {file}", thread.name())
    }

    /// The bytes of `file`, a part [`Self::keep_part`] kept: an error
    /// when it isn't there, or isn't a part's name.
    fn part(&self, agent: &Agent, thread: &Thread, file: &str) -> anyhow::Result<Vec<u8>> {
        let _ = agent;
        bail!("{} keeps no images or documents: {file}", thread.name())
    }
    /// Writes `bytes` as `name` in `agent`'s [`EXPORTS`], over one there
    /// before: the file's path in the agent's folder.
    fn export(&self, agent: &Agent, name: &str, bytes: &[u8]) -> anyhow::Result<String> {
        let _ = (agent, bytes);
        bail!("this store keeps no exports: {name}")
    }
}

/// The folder of the agent's own that `/chat:export` writes to.
pub const EXPORTS: &str = "exports";

/// The folder of a conversation's parts, in its folder.
pub const PARTS: &str = "parts";

/// The name of `file` in [`PARTS`], when it's a part's: `parts/` and a
/// name of letters, digits, `-`, `_` and `.`, never starting with a dot,
/// so it can't reach outside it.
pub fn part_name(file: &str) -> anyhow::Result<&str> {
    let name = file
        .strip_prefix(PARTS)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or_default();
    let plain = !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    anyhow::ensure!(plain, "{file:?} isn't a part's file");
    Ok(name)
}

/// What taking a turn's lease came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Took {
    /// Taken: no turn ran.
    Took,
    /// Taken from a turn whose lease lapsed: it crashed.
    Lapsed,
    /// Another's, renewed lately.
    Held,
}

/// A turn's lease, in its conversation's folder ([`TURN`]): who runs
/// it, and when it started and was last renewed, in seconds since 1970.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lease {
    pub holder: String,
    /// Its thread's name, for a process finishing it after a crash.
    pub thread: String,
    pub started: u64,
    pub renewed: u64,
}

/// How a conversation's events are kept.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Way {
    /// [`LOG`], beside a copy for people ([`MARKDOWN`]).
    Log,
    /// Compressed, in frames ([`frames`]).
    Compact,
    /// By a store driver, `lnk-<name>`.
    Driver(String),
}

impl Way {
    /// The way `[storage] conversations` names: `log`, `compact`, or a
    /// driver's name.
    pub fn named(name: &str) -> Self {
        match name {
            link_agent_spec::LOG => Self::Log,
            link_agent_spec::COMPACT => Self::Compact,
            driver => Self::Driver(driver.to_string()),
        }
    }

    /// Its name, as `[storage] conversations` says it.
    pub fn name(&self) -> &str {
        match self {
            Self::Log => link_agent_spec::LOG,
            Self::Compact => link_agent_spec::COMPACT,
            Self::Driver(name) => name,
        }
    }
}

/// The start of a driver's marker, in a conversation's folder:
/// What names a message its source may send again: its delivery, or in a
/// task's conversation, who gave the task.
#[derive(Clone, Debug, PartialEq)]
pub enum Repeat {
    Delivery(link_interactions::TransportEventId),
    From(String),
}

impl Repeat {
    /// Whether `event` is a message this names.
    pub fn names(&self, event: &Event) -> bool {
        event.role == crate::event::USER
            && match self {
                Self::Delivery(id) => event.delivery.as_ref() == Some(id),
                Self::From(from) => event.from.as_deref() == Some(from.as_str()),
            }
    }

    /// The keys a message is found by: one for its delivery, one for who
    /// said it.
    fn keys(event: &Event) -> Vec<String> {
        if event.role != crate::event::USER {
            return Vec::new();
        }
        let delivery = event.delivery.clone().map(Self::Delivery);
        let from = event.from.clone().map(Self::From);
        delivery
            .into_iter()
            .chain(from)
            .map(|repeat| repeat.key())
            .collect()
    }

    fn key(&self) -> String {
        match self {
            Self::Delivery(id) => {
                format!("delivery {}", serde_json::to_string(id).unwrap_or_default())
            }
            Self::From(from) => format!("from {from}"),
        }
    }
}

/// `store.<name>`, empty.
pub const DRIVER_MARK: &str = "store.";

/// How the conversation in `folder` is kept, as its files say: a
/// driver's marker, else compact's frames, else the log; none when nothing is kept there yet.
fn kept_in(folder: &Held) -> anyhow::Result<Option<Way>> {
    let names = folder
        .names()
        .within(format!("reading {}", folder.path.display()))?;
    let mut ways = Vec::new();
    for name in &names {
        if let Some(driver) = name.strip_prefix(DRIVER_MARK)
            && link_store_contract::store::check_name(driver).is_ok()
        {
            ways.push(Way::Driver(driver.to_string()));
        } else if name == link_store_contract::store::compact::FRAMES {
            ways.push(Way::Compact);
        } else if name == LOG {
            ways.push(Way::Log);
        }
    }
    // A driver's first: its marker is all it leaves.
    ways.sort();
    Ok(ways.pop())
}

/// The store driver that keeps the conversation in `folder`, if one does.
fn driver_of(folder: &Held) -> Option<String> {
    match kept_in(folder).ok()?? {
        Way::Driver(name) => Some(name),
        _ => None,
    }
}

/// A turn's lease, in the conversation's folder.
pub const TURN: &str = "turn.json";

/// Asked of a turn that's to end now, beside its lease.
const INTERRUPT: &str = "turn.interrupt";

/// Held by the one taking a lapsed lease over, beside it.
const TAKE: &str = "turn.take";

/// The number of an answer that finished a crashed turn, beside the
/// lease, until the agent's bridge takes it.
const RECOVERED: &str = "turn.recovered";

/// Now, in seconds since 1970.
fn secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A conversation's events, in its folder: a line of JSON each, in
/// order, only ever added to ([`link_store_contract::store::folder`], the folder
/// a move carries too).
pub const LOG: &str = link_store_contract::store::folder::EVENTS;

/// The largest number of an event.
const SEQ_MAX: u64 = 99_999_999;

/// The markdown copy's name, in the conversation's folder.
pub const MARKDOWN: &str = link_store_contract::store::folder::MARKDOWN;

/// How many of the last events [`Store::mark`] reads for the window's
/// [`Mark`].
pub const MARK_EVENTS: u64 = 32;

/// The most an event's line holds: a message's text is at most 2 MiB
/// (the endpoint's limit; its parts are files of their own), and an
/// answer less, written as JSON.
pub const EVENT_MAX: u64 = 16 << 20;

/// The most a small file of the conversation's holds (its lease).
const MARK_MAX: u64 = 4 << 10;

/// The tools a conversation was narrowed to, in its folder.
pub const TOOLS: &str = link_store_contract::store::folder::TOOLS;

/// The most `tools.json` holds.
const TOOLS_FILE_MAX: u64 = link_store_contract::store::folder::TOOLS_FILE_MAX;

/// The most `config.json` holds.
const CONFIG_MAX: u64 = 1 << 20;

/// The longest instructions read, in bytes.
pub const INSTRUCTIONS_MAX: u64 = 64 << 10;

/// The most `tool-lists.json` holds, in bytes.
const TOOL_LISTS_MAX: u64 = 8 << 20;

/// The most skills read: past it, the rest are left out.
pub const SKILLS_MAX: usize = 1024;

/// Which file, and which write of it: each write puts a new file in
/// place ([`link_files::write_private`]).
type Stamp = (u64, u64, u64, Option<std::time::SystemTime>);

/// Each agent's things as files in its harness's home and its files
/// folder.
pub struct Files {
    agents: Agents,
    /// Each agent's settings as last read, with the file they came from,
    /// so a turn, a proof and a key check read the file only once it
    /// changes. Derived: a process without it reads the file.
    configs: Mutex<BTreeMap<Agent, (Stamp, Config)>>,
    /// Each skill's front matter as last read, by its `SKILL.md`, with
    /// the file it came from: none when it said no name and purpose.
    /// Derived, as `configs` is.
    skills: Mutex<BTreeMap<PathBuf, (Stamp, Option<Skill>)>>,
    /// Whether a driver is called through the agent's store bridge, when
    /// Link runs one: yes but for a step run while the agent may be
    /// stopped ([`Files::without_bridge`]).
    bridged: bool,
    /// The settings of each agent that has no `config.json`, held in
    /// memory: a harness started blank, bound to its agent with them
    /// ([`Files::holding`]). Never written to a file.
    held: Mutex<BTreeMap<Agent, Config>>,
    /// The marks of conversations kept off this machine, which are never
    /// on its disk.
    marks: Mutex<Marks>,
    /// What this process last read of each conversation's log, so a read
    /// parses only what was added since ([`seen`]). Derived.
    logs: Logs,
    /// Where each log's first message of each delivery and sender is
    /// ([`firsts`]). Derived.
    firsts: firsts::Firsts,
    /// What this process last read of each conversation kept compact or
    /// by a driver, so a read asks for only what was added ([`recent`]).
    /// Derived.
    recent: recent::Recent,
}

/// A conversation's marks for this process alone, held in memory where
/// its conversations are kept off this machine: a turn asked to end, and
/// an answer of a crashed turn not yet sent. Losing them costs only that
/// interruption, or that answer's copy to the chat; the answer itself is
/// in the conversation.
#[derive(Default)]
struct Marks {
    interrupted: std::collections::BTreeSet<(Agent, String)>,
    recovered: BTreeMap<(Agent, Thread), u64>,
}

impl Files {
    pub fn new(agents: Agents) -> Self {
        Self {
            agents,
            configs: Mutex::default(),
            skills: Mutex::default(),
            bridged: true,
            held: Mutex::default(),
            marks: Mutex::default(),
            logs: Logs::default(),
            firsts: firsts::Firsts::default(),
            recent: recent::Recent::default(),
        }
    }

    /// How many bytes of its conversations' logs it has parsed into
    /// events, all told: what reading them has cost. A read parses only
    /// what was added since this store last read the log.
    pub fn parsed(&self) -> u64 {
        self.logs.total()
    }

    /// How many events of its conversations kept compact or by a driver
    /// it has read, all told ([`recent`]): a read takes only those added
    /// since this store last read the conversation.
    pub fn fetched(&self) -> u64 {
        self.recent.fetched()
    }

    /// One that holds `config` as `agent`'s settings, in memory: its
    /// `config.json` is never read or written, and a change of them
    /// (`configure`) changes only what it holds. For a harness started
    /// blank, bound to `agent` with them.
    pub fn holding(self, agent: &Agent, config: Config) -> Self {
        self.held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(agent.clone(), config);
        self
    }

    /// One that runs every driver itself, never through a store bridge:
    /// for a step of Link's run while the agent may be stopped (reading a
    /// conversation, moving them), whose bridge is then gone.
    pub fn without_bridge(mut self) -> Self {
        self.bridged = false;
        self
    }

    /// The skill `file` says, its `SKILL.md` at `path`: read only once
    /// it changes.
    fn skill(&self, path: PathBuf, file: std::fs::File) -> Option<Skill> {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata().ok()?;
        let stamp = (meta.dev(), meta.ino(), meta.len(), meta.modified().ok());
        let mut skills = self.skills.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((seen, skill)) = skills.get(&path)
            && *seen == stamp
        {
            return skill.clone();
        }
        // Its front matter is at its head.
        let mut text = Vec::new();
        file.take(INSTRUCTIONS_MAX).read_to_end(&mut text).ok()?;
        let skill = skill::parse(&String::from_utf8_lossy(&text)).ok();
        skills.insert(path, (stamp, skill.clone()));
        skill
    }

    /// `agent`'s own folder, held open: never through a link, which Link
    /// never follows there. None when it isn't there, unless `make`.
    fn own(&self, agent: &Agent, make: bool) -> anyhow::Result<Option<Held>> {
        Ok(Held::unlinked(&self.agents.paths(agent)?.own, make)?)
    }

    /// `agent`'s `chats/`, held open, as [`Files::own`] is.
    fn conversations(&self, agent: &Agent, make: bool) -> anyhow::Result<Option<Held>> {
        let Some(own) = self.own(agent, make)? else {
            return Ok(None);
        };
        Ok(own.folder(
            link_store_contract::storage::Collection::Conversations.directory(),
            make,
        )?)
    }

    /// `thread`'s folder, held open: the agent's own folder, its
    /// `chats/` and the conversation's each opened from the one
    /// before, never through a link (refused), so nothing is read or
    /// written elsewhere however they change meanwhile. None when one
    /// isn't there, unless `make`: then each is made, the conversation's
    /// its own from the start (700), never another user's to read.
    fn folder(&self, agent: &Agent, thread: &Thread, make: bool) -> anyhow::Result<Option<Held>> {
        let Some(conversations) = self.conversations(agent, make)? else {
            return Ok(None);
        };
        let Some(folder) = conversations.folder(&thread.folder(), make)? else {
            return Ok(None);
        };
        if make {
            folder.private()?;
        }
        Ok(Some(folder))
    }
}

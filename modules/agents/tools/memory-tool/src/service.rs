//! Link Memory for many agents from one process (`lnk-memory-tool
//! serve-many <socket> <agents>`, `link_memory_tool_contract`'s `serve-many`): every
//! agent of one owner on a machine, or every agent a machine binds, each
//! session answered from the agent Link opened it for and no other.
//!
//! A session is a connection to the Unix socket. Its first line names
//! its agent ([`Hello`]), written by Link outside every sandbox; the
//! rest is MCP, as over stdio. The agent is never read from what comes
//! after: a session is one agent's from its first line to its end, and
//! answered from that agent's index and notes alone, opened from what
//! `<agents>` says of it ([`served`]) at its first session.
//!
//! A [`Hello`] may hand it the agent's bridges ([`Bound`]): then the
//! agent is served as its bridges reach it, by its owner and ID
//! ([`Table::bind`]). Its conversations kept elsewhere are read through
//! the storage bridge, its notes written there (`remember`), and its
//! index is a cache of the copy kept with its `memory` collection where
//! that's elsewhere, or beside its Home where Home is kept in a bucket
//! (through the files bridge): fetched when there's no cache, written
//! back at most once a minute while it changes, and at a
//! [`Control::Flush`]. A write back refused (another wrote the copy
//! meanwhile) drops the cache and fetches the copy again, then indexes
//! what it lacks. At [`Control::End`] its key is dropped. An agent
//! `<agents>` doesn't name keeps its index in `--cache`
//! (`<cache>/<owner>/<agent>/`), and is read only through its bridges.
//!
//! A session is taken only from a process of the user it runs as, or of
//! the one user `--peer` names (a machine's service binding its agents)
//! ([`taken_from`]): its socket is in Link's settings, which no sandbox
//! reaches, and those processes are all that's left.
//!
//! It ends once no session has been open for [`IDLE`], its socket
//! removed first, so an idle owner runs nothing of it, and a session
//! arriving as it ends finds no socket and starts another; with
//! `--stay`, never.

use link_format::{Name, sentence};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use link_memory_tool_contract::declared::Agents;
use link_memory_tool_contract::{Bound, Control, Hello, TeamSession, Welcome};
use link_sandbox_binding::Binding;
use link_sandbox_binding::Identity;
use link_secretive::Duplicate;
use link_store_contract::files::{self, Version};
use link_store_contract::storage::Collection;

use crate::notes::{self, Notes};
use crate::semantic::Source;
use crate::serve::{Answers, Pool, Served, Writer, Writes};
use crate::{Common, Index, mcp, tree};

mod bridges;
mod one;
mod users;

pub use one::{One, serve_one};

pub use users::{group_id, same_user, user_id};
use users::{peer_user, shared_group, taken_from};

/// How long it runs with no session open: long enough that a harness
/// opening a session as another ends finds it, short enough that an idle
/// owner's machine holds nothing of it for long.
pub const IDLE: Duration = Duration::from_secs(5);

/// How often a bound agent's changed index is written back to its copy.
pub const WRITE_EVERY: Duration = Duration::from_secs(60);

/// How long a [`Control::Flush`] embeds what's waiting before it writes
/// the index back: within the 30 s the machine waits for it.
const FLUSH_EMBED: Duration = Duration::from_secs(20);

/// The longest first line taken, a [`Hello`] or a [`Control`].
const HELLO_MAX: u64 = 8192;

/// How long a session may take to say its agent.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// An agent's index served, opened at its first session: its writer
/// started, its sessions reading through readers of their own.
type Opened = Arc<Served>;

/// An agent's place in the table: its index once opened. Each agent's
/// is opened under its own lock, so a slow open holds up only sessions
/// of its agent, and two sessions arriving at once open it once.
type Slot = Arc<Mutex<Option<Opened>>>;

/// What a `serve` command line says.
struct Args {
    /// The agent's Home, and its own folder, its conversations and
    /// skills there (none named: the folder holds both).
    folder: link_agent_contract::folder::Folder,
    index: Option<PathBuf>,
    model: Option<String>,
    url: Option<String>,
    tokens: usize,
    notes: Option<PathBuf>,
    notes_most: u64,
    /// Whether the agent's harness is sent others' messages
    /// ([`mcp::Options::from`]).
    from: bool,
    /// What its memory may spend.
    opinions: crate::opinions::Opinions,
}

impl Args {
    /// The folder its notes are in: the agent's own.
    fn notes_root(&self) -> &Path {
        &self.folder.agent
    }
}

fn args(serve: &crate::cli::MemoryToolServeArgs) -> Result<Args> {
    let opinions = crate::opinions::Opinions::served(serve)?;
    Ok(Args {
        folder: match &serve.agent_folder {
            Some(agent) => link_agent_contract::folder::Folder {
                home: serve.folder.clone(),
                agent: agent.clone(),
            },
            None => link_agent_contract::folder::Folder::flat(&serve.folder),
        },
        index: serve.index.clone(),
        model: serve.embed.clone(),
        url: serve.embed_url.clone(),
        tokens: serve.tokens.map_or(mcp::tokens_max(), usize::from),
        notes: serve.notes.clone(),
        notes_most: serve.notes_most.unwrap_or(notes::MOST),
        from: serve.from_others,
        opinions,
    })
}

/// What a `serve` command line says: the folder's index, opened, and
/// how it serves. `env` is the environment it runs with, which names
/// its agent; its model's address is the line's `--embed-url`.
pub fn served(
    serve: &crate::cli::MemoryToolServeArgs,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(Index, mcp::Options)> {
    let parsed = args(serve)?;
    let notes = parsed
        .notes
        .as_ref()
        .map(|file| Notes::new(parsed.notes_root(), file, parsed.notes_most))
        .transpose()?;
    let mut index = Index::open_with(&parsed.folder, parsed.index.as_deref())?;
    index.spend(parsed.opinions.clone())?;
    // Its agent, as Link Harness names it to a store driver.
    let id = env(link_agent_contract::identity::AGENT_ID_ENV)
        .zip(env(link_agent_contract::identity::OWNER_ENV));
    index.for_agent(match id {
        Some((id, owner)) => link_store_contract::store::Agent { id, owner },
        None => link_store_contract::store::Agent {
            id: env("LNK_AGENT").unwrap_or_else(|| "main".into()),
            owner: String::new(),
        },
    });
    index.driven_by(env(link_memory_tool_contract::DRIVER_ENV).filter(|d| !d.is_empty()));
    embed(&mut index, &parsed)?;
    Ok((
        index,
        mcp::Options {
            tokens: parsed.tokens,
            notes,
            from: parsed.from,
        },
    ))
}

/// Its index searching by meaning, as `parsed` says.
fn embed(index: &mut Index, parsed: &Args) -> Result<()> {
    match (&parsed.model, &parsed.url) {
        // Empty: served for it outside its sandbox, but no runtime here
        // has the model, so it searches by words.
        (Some(model), Some(url)) if url.is_empty() => index.embed(Source::Missing(model.clone())),
        (Some(model), Some(url)) => index.embed(Source::Url(url.clone(), model.clone())),
        (Some(model), None) => index.embed(Source::Serve(model.clone())),
        // Its declaration refuses an address with no model (`checks.rs`).
        (None, _) => Ok(()),
    }
}

/// How the service runs, besides the agents it names.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// Where the index of a bound agent it doesn't name is kept, a folder
    /// each (`<cache>/<owner>/<agent>/`); none, it binds only those it
    /// names.
    pub cache: Option<PathBuf>,
    /// The embedding server, and the model asked of it, for a bound agent
    /// it doesn't name; none, by words alone.
    pub embed: Option<(String, String)>,
    /// How long it runs with no session open; none, for good.
    pub idle: Option<Duration>,
    /// Another user whose sessions it takes: the service binding a
    /// machine's agents, or the user who started it where the sandbox runs
    /// it as an identity of its own (an ordinary box's worker).
    pub peer: Option<libc::uid_t>,
    /// The group its socket is shared with (mode 660), `peer`'s, where
    /// `peer` reaches the socket only through it; its folder must let no
    /// one else in. None, the socket is its owner's alone.
    pub group: Option<libc::gid_t>,
    /// The files bridge a bound agent's `Bound` may name: this machine's
    /// own, and no other socket.
    pub files_bridge: Option<PathBuf>,
    /// The storage bridge a bound agent's `Bound` may name, when one serves
    /// every agent here (a machine that binds its agents).
    pub storage_bridge: Option<PathBuf>,
    /// The folder each agent's own storage bridge socket is in, by its ID
    /// (`<bridges>/<agent>/storage.sock`), where each agent has one.
    pub bridges: Option<PathBuf>,
    /// How often a bound agent's changed index is written back.
    pub write_every: Duration,
}

/// What a bound agent's writer holds, or its session of a team's
/// memory's: its index, its bridges, and its copy.
struct Held {
    index: Index,
    bound: Bound,
    /// The team whose memory it is; none, the agent's own.
    team: Option<TeamSession>,
    /// Where its index's copy is kept; none, its index is kept here.
    copies: Option<Arc<dyn files::Store>>,
    /// The copy's version the cache is of; none: there was no copy.
    copy: Option<Version>,
    /// Its index file: the copy's version noted beside it, and its
    /// snapshot written there.
    file: PathBuf,
    /// When it was last written back, and the index's mark then.
    written: (Instant, (i64, u64)),
    /// The index its sessions read, told anew when it's replaced (its
    /// copy fetched again).
    readers: Arc<RwLock<Arc<Common>>>,
}

impl Writes for Held {
    fn work(&mut self, pool: &Pool) -> bool {
        self.index.work(pool)
    }
}

/// A bound agent, or its session of a team's memory, served: how it's
/// bound and served, which never changes while it is, its writer, and
/// the index its sessions read.
struct BoundAgent {
    bound: Bound,
    team: Option<TeamSession>,
    options: mcp::Options,
    readers: Arc<RwLock<Arc<Common>>>,
    /// Unbound: a session still open answers nothing more.
    ended: AtomicBool,
    writer: Writer<Held>,
}

impl BoundAgent {
    /// `held`, served as `options` say.
    fn start(mut held: Held, options: mcp::Options, pool: Arc<Pool>) -> Self {
        held.index.watch();
        let journal = held.index.common().journal().clone();
        let agent = Self {
            bound: held.bound.duplicate(),
            team: held.team.clone(),
            options,
            readers: held.readers.clone(),
            ended: AtomicBool::new(false),
            writer: Writer::start(held, pool),
        };
        journal.wake_with(agent.writer.waker());
        agent
    }
}

impl Answers for BoundAgent {
    fn common(&self) -> Arc<Common> {
        self.readers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn options(&self) -> &mcp::Options {
        &self.options
    }

    fn ended(&self) -> Option<&'static str> {
        self.ended
            .load(Ordering::SeqCst)
            .then_some("its agent isn't bound here any more")
    }
}

type BoundSlot = Arc<BoundAgent>;

/// A bound session's key: the agent's owner and ID, and the ID of the
/// team whose memory it is (none, the agent's own).
type Key = (String, String, Option<String>);

fn key(owner: &str, agent: &str, team: Option<&str>) -> Key {
    (owner.into(), agent.into(), team.map(str::to_string))
}

/// The agents served: what `<agents>` says of each and each one's index
/// once a session opened it, and each agent bound, by owner and ID.
struct Table {
    agents: Agents,
    config: Config,
    opened: Mutex<HashMap<String, Slot>>,
    bound: Mutex<HashMap<Key, BoundSlot>>,
    /// Held while an agent (or its team's memory) is bound, by its key:
    /// two first sessions of one at once bind it once.
    binding: Mutex<HashMap<Key, BindLock>>,
    /// The turns its indexes' writers take at once.
    pool: Arc<Pool>,
}

/// The values of `map`, taken out from under its lock.
fn busy_list<K, V: Clone>(map: &Mutex<HashMap<K, V>>) -> Vec<V> {
    map.lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .cloned()
        .collect()
}

/// Held while one agent is bound ([`Table::binding`]).
type BindLock = Arc<Mutex<()>>;

/// A session's first line: a [`Control`] or a [`Hello`].
enum First {
    Control(Control),
    Hello(Hello),
}

impl First {
    /// `line` read as a control when it's one (an object of one key,
    /// `changed`, `flush` or `end`), else a hello: refused saying why, as
    /// each refuses a field it doesn't know.
    fn parse(line: &str) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(line)?;
        let control = value.as_object().is_some_and(|fields| {
            fields.len() == 1
                && ["changed", "flush", "end"]
                    .iter()
                    .any(|key| fields.contains_key(*key))
        });
        Ok(match control {
            true => First::Control(serde_json::from_value(value).context("a control")?),
            false => First::Hello(serde_json::from_value(value).context("a hello")?),
        })
    }
}

/// An error unless `agent` is an agent's ID as a cache folder may be
/// named: letters, digits, `-` and `_`, at most 64.
fn check_agent(agent: &str) -> Result<()> {
    ensure!(
        (1..=64).contains(&agent.len())
            && agent
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "{:?} isn't an agent's ID",
        link_format::clean(agent)
    );
    Ok(())
}

impl Table {
    /// Whether any index it holds has indexing left, is extracting
    /// documents' text, or is held by work under way: the service waits
    /// for it before it ends, so what it began is finished, not begun again.
    fn extracting(&self) -> bool {
        let opened: Vec<Slot> = busy_list(&self.opened);
        let bound: Vec<BoundSlot> = busy_list(&self.bound);
        opened.iter().any(|slot| match slot.try_lock() {
            Ok(held) => held
                .as_ref()
                .is_some_and(|served| served.writer().busy(Index::busy)),
            Err(_) => true,
        }) || bound
            .iter()
            .any(|slot| slot.writer.busy(|held| held.index.busy()))
    }

    /// Agent `id`'s index, opened at its first session: an agent the
    /// table doesn't name is refused.
    fn open(&self, id: &str) -> Result<Opened> {
        let args = self.serve_args(id)?;
        let slot = self
            .opened
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(id.to_string())
            .or_default()
            .clone();
        // The table's lock let go: other agents open meanwhile.
        let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(open) = &*slot {
            return Ok(open.clone());
        }
        let agent = &self.agents.0[id];
        let env = |key: &str| agent.env.get(key).cloned();
        let (index, options) =
            served(&args, &env).with_context(|| format!("serving agent {id:?}"))?;
        let open = Arc::new(Served::start_in(index, options, self.pool.clone()));
        *slot = Some(open.clone());
        Ok(open)
    }

    /// What `<agents>` says agent `id` is served with (its `serve`
    /// line, read back): an agent it doesn't name is refused.
    fn serve_args(&self, id: &str) -> Result<crate::cli::MemoryToolServeArgs> {
        let Some(agent) = self.agents.0.get(id) else {
            bail!("no agent {id:?} here");
        };
        let env = |key: &str| agent.env.get(key).cloned();
        crate::cli::serve_line(&agent.argv, &env)
            .with_context(|| format!("agent {id:?} isn't served by 'serve'"))
    }

    fn get(&self, owner: &str, agent: &str, team: Option<&str>) -> Option<BoundSlot> {
        self.bound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key(owner, agent, team))
            .cloned()
    }

    /// Every session bound of `owner`'s `agent`: its own first, then its
    /// teams' memories'.
    fn all_of(&self, owner: &str, agent: &str) -> Vec<BoundSlot> {
        let mut all: Vec<(Key, BoundSlot)> = self
            .bound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|((o, a, _), _)| o == owner && a == agent)
            .map(|(key, slot)| (key.clone(), slot.clone()))
            .collect();
        all.sort_by(|(a, _), (b, _)| a.cmp(b));
        all.into_iter().map(|(_, slot)| slot).collect()
    }

    /// One session unbound: its key and index dropped; its cache stays.
    fn end_one(&self, key: &Key) {
        let slot = self
            .bound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
        if let Some(slot) = slot {
            slot.ended.store(true, Ordering::SeqCst);
        }
    }

    /// `owner`'s `agent` unbound, its teams' memories too: their keys and
    /// indexes dropped; their caches stay.
    fn end(&self, owner: &str, agent: &str) {
        let keys: Vec<Key> = self
            .bound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .filter(|(o, a, _)| o == owner && a == agent)
            .cloned()
            .collect();
        for key in keys {
            self.end_one(&key);
        }
    }

    fn control(&self, control: Control) -> Result<()> {
        let checked = |owner: &str, agent: &str| -> Result<()> {
            check_agent(agent)?;
            link_agent_contract::identity::check_owner(owner)
        };
        match control {
            Control::Changed {
                owner,
                agent,
                path,
                team,
            } => {
                checked(&owner, &agent)?;
                // A change to what memory keeps: listed again soon, told
                // without waiting on its writer.
                if tree::wanted(&path, 0, 0).is_some()
                    && let Some(slot) = self.get(&owner, &agent, team.as_deref())
                {
                    slot.common().noted(&path);
                }
                Ok(())
            }
            Control::Flush { owner, agent } => {
                checked(&owner, &agent)?;
                let slots = self.all_of(&owner, &agent);
                ensure!(!slots.is_empty(), "agent {agent} isn't bound here");
                // Each written back, its teams' too: the first failure
                // said once all were tried.
                let mut failed = Ok(());
                for slot in slots {
                    let mut held = slot.writer.lock();
                    if let Err(err) = self.write_back(&agent, &mut held, true) {
                        link_format::daemon::warn!("{agent}: writing its index back: {err:#}");
                        if failed.is_ok() {
                            failed = Err(err);
                        }
                    }
                }
                failed
            }
            Control::End { owner, agent } => {
                checked(&owner, &agent)?;
                // Written back first, so nothing it indexed is lost.
                for slot in self.all_of(&owner, &agent) {
                    let mut held = slot.writer.lock();
                    if let Err(err) = self.write_back(&agent, &mut held, false) {
                        crate::warn(
                            sentence!(
                                "The index of {} isn't written back.",
                                Name::new("agent", &agent)
                            ),
                            &err,
                        );
                    }
                }
                self.end(&owner, &agent);
                Ok(())
            }
        }
    }

    /// Writes back what's due, for each agent bound whose writer isn't
    /// taking a turn: its writer lists what changed.
    fn tick(&self) {
        let slots: Vec<(Key, BoundSlot)> = self
            .bound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(k, s)| (k.clone(), s.clone()))
            .collect();
        for ((_, agent, _), slot) in slots {
            let Some(mut held) = slot.writer.try_lock() else {
                continue;
            };
            if held.written.0.elapsed() >= self.config.write_every
                && let Err(err) = self.write_back(&agent, &mut held, false)
            {
                crate::warn(
                    sentence!(
                        "The index of {} isn't written back.",
                        Name::new("agent", &agent)
                    ),
                    &err,
                );
            }
        }
    }
}

/// Serves every agent of `agents` on `socket` until none has had a
/// session open for [`IDLE`].
pub fn serve(socket: &Path, agents: Agents) -> Result<()> {
    serve_with(
        socket,
        agents,
        Config {
            idle: Some(IDLE),
            write_every: WRITE_EVERY,
            ..Config::default()
        },
    )
}

/// [`serve`], ending after `idle` with no session.
pub fn serve_for(socket: &Path, agents: Agents, idle: Duration) -> Result<()> {
    serve_with(
        socket,
        agents,
        Config {
            idle: Some(idle),
            write_every: WRITE_EVERY,
            ..Config::default()
        },
    )
}

/// [`serve`], as `config` says.
pub fn serve_with(socket: &Path, agents: Agents, config: Config) -> Result<()> {
    if let Some(cache) = &config.cache {
        link_files::create_private_dir(cache)?;
    }
    // Only this user's, or shared with its peer's group (0660): so from
    // the moment it's at its path.
    let given = match config.group {
        Some(group) => link_sandbox_bridge::socket::Given {
            group: Some(shared_group(socket, group)?),
            mode: 0o660,
            ..Default::default()
        },
        None => link_sandbox_bridge::socket::Given::default(),
    };
    let listener = link_sandbox_bridge::socket::listen_given(socket, given)?;
    listener.set_nonblocking(true)?;
    let idle = config.idle;
    let table = Arc::new(Table {
        agents,
        config,
        opened: Mutex::default(),
        bound: Mutex::default(),
        binding: Mutex::default(),
        pool: crate::serve::Pool::new(),
    });
    ticking(&table);
    // Sessions open now, and when the last one ended.
    let open = Arc::new(Mutex::new((0usize, Instant::now())));
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                {
                    let mut open = open.lock().unwrap_or_else(|e| e.into_inner());
                    let most = crate::limits::limits().sessions_max;
                    if open.0 >= most {
                        // Refused, said, and closed: never a thread more.
                        let refused = Welcome {
                            ok: false,
                            error: Some(format!(
                                "the service serves {most} sessions at once: try again"
                            )),
                        };
                        let _ = writeln!(stream, "{}", serde_json::to_string(&refused)?);
                        continue;
                    }
                    open.0 += 1;
                }
                let (table, open) = (table.clone(), open.clone());
                std::thread::spawn(move || {
                    if let Err(err) = session(&table, stream) {
                        crate::warn(sentence!("A session ended."), &err);
                    }
                    let mut open = open.lock().unwrap_or_else(|e| e.into_inner());
                    *open = (open.0 - 1, Instant::now());
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                let (sessions, since) = *open.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(idle) = idle
                    && sessions == 0
                    && since.elapsed() >= idle
                    && !table.extracting()
                {
                    // Every bound agent's index written back first.
                    let ended: std::collections::BTreeSet<(String, String)> = table
                        .bound
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .keys()
                        .map(|(owner, agent, _)| (owner.clone(), agent.clone()))
                        .collect();
                    for (owner, agent) in ended {
                        let _ = table.control(Control::End { owner, agent });
                    }
                    // Gone first: a session starting now starts another.
                    let _ = std::fs::remove_file(socket);
                    return Ok(());
                }
                waiting(&listener, Duration::from_secs(1));
            }
            Err(err) => return Err(err).context("taking a session"),
        }
    }
}

/// Each second while `table` is held, what's due written back
/// ([`Table::tick`]).
#[expect(clippy::disallowed_methods, reason = "the service's own period")]
fn ticking(table: &Arc<Table>) {
    let ticking = Arc::downgrade(table);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            match ticking.upgrade() {
                Some(table) => table.tick(),
                None => return,
            }
        }
    });
}

/// Waits until `listener` has a session to take, or `most`: taken as it
/// comes, not at the next of a timer's ticks.
fn waiting(listener: &UnixListener, most: Duration) {
    use std::os::fd::AsRawFd;
    let mut fd = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one pollfd of a listener this process holds open.
    unsafe { libc::poll(&mut fd, 1, most.as_millis() as libc::c_int) };
}

/// What a session is answered from: an agent `<agents>` names, or one
/// bound.
enum Answering {
    Named(Opened),
    Bound(BoundSlot),
}

impl Answering {
    fn answers(&self) -> &dyn Answers {
        match self {
            Answering::Named(served) => served.as_ref(),
            Answering::Bound(slot) => slot.as_ref(),
        }
    }
}

/// One session: a control, answered once done; or an agent's, its agent
/// from its first line, then its messages answered from that agent's
/// index alone.
fn session(table: &Table, mut stream: UnixStream) -> Result<()> {
    // SAFETY: getuid takes nothing and can't fail.
    let me = unsafe { libc::getuid() };
    if !taken_from(peer_user(&stream), me, table.config.peer) {
        // Its first line read before the answer, as every session's is: a
        // client still sending it would otherwise meet a closed socket and
        // never read why. Said, so its session ends refused rather than
        // reset and tried again.
        stream.set_read_timeout(Some(HELLO_TIMEOUT))?;
        let mut first = String::new();
        let _ = BufReader::new(&stream)
            .take(HELLO_MAX)
            .read_line(&mut first);
        let refused = "a session from another user than this service's isn't taken";
        let said = Welcome {
            ok: false,
            error: Some(refused.into()),
        };
        let _ = writeln!(stream, "{}", serde_json::to_string(&said)?);
        anyhow::bail!(refused);
    }
    stream.set_read_timeout(Some(HELLO_TIMEOUT))?;
    let mut output = stream.try_clone()?;
    let mut input = BufReader::new(stream);
    let mut line = String::new();
    (&mut input).take(HELLO_MAX).read_line(&mut line)?;
    let first = First::parse(&line).context("its first line isn't a hello or a control");
    fn welcome<T>(result: &Result<T>) -> Welcome {
        match result {
            Ok(_) => Welcome {
                ok: true,
                error: None,
            },
            Err(err) => Welcome {
                ok: false,
                error: Some(format!("{err:#}")),
            },
        }
    }
    let opened = match first {
        Ok(First::Control(control)) => {
            let done = table.control(control);
            writeln!(output, "{}", serde_json::to_string(&welcome(&done))?)?;
            return done;
        }
        Ok(First::Hello(hello)) => match (hello.bound, hello.team) {
            (Some(bound), team) => table.bind(&hello.agent, bound, team).map(Answering::Bound),
            (None, None) => table.open(&hello.agent).map(Answering::Named),
            (None, Some(_)) => Err(anyhow::anyhow!(
                "a team's memory is served only to an agent bound with its files bridge"
            )),
        },
        Err(err) => Err(err),
    };
    writeln!(output, "{}", serde_json::to_string(&welcome(&opened))?)?;
    output.flush()?;
    let opened = opened?;
    input.get_ref().set_read_timeout(None)?;
    crate::serve::session(opened.answers(), input, output)
}

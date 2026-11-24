//! What carries an agent's connections while it runs: started, paused,
//! quieted and stopped, with its storage bridge and scheduler's runner.

use super::*;
use link_format::WithinExt;

impl Carrying {
    /// Carrying nothing: an agent whose plan couldn't be read.
    pub fn none() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(true)),
            quiet: Arc::new(AtomicBool::new(true)),
            connections: Vec::new(),
            runner: None,
            way_in: None,
            store: None,
            recovery: None,
            threads: Vec::new(),
        }
    }

    /// Asks the harness now for the answers of turns nobody took (a
    /// crash's, or a caller's that went), and sends each where it goes:
    /// for an agent whose harness stops when idle, asked as it wakes and
    /// before it stops, never by a connection held to it.
    pub fn recover_now(&self) -> anyhow::Result<()> {
        let Some(way_in) = &self.recovery else {
            return Ok(());
        };
        link_runtime::block_on(way_in.recover())
    }

    /// Waits for every thread, which is for good unless [`Self::stop`]s.
    pub fn join(self) {
        for thread in self.threads {
            let _ = thread.join();
        }
    }

    /// Stops carrying: each connection's bridge ends (its stdin closed), the
    /// way in takes no more messages and goes, the scheduler's runner
    /// ends ([`crate::kept::Kept::stop`]: asked, then waited for), and
    /// the threads end. A message already taken is carried to its end.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        quiet(
            &self.quiet,
            &self.connections,
            self.way_in.iter().chain(&self.store),
        );
        if let Some(runner) = &self.runner {
            runner.stop();
        }
    }
}

/// Stops taking messages: each connection's bridge ends (its stdin
/// closed), and each socket in `sockets` takes no more and goes. A
/// message already taken is carried to its end.
fn quiet<'a>(
    flag: &AtomicBool,
    connections: &[Arc<Mutex<Option<Running>>>],
    sockets: impl Iterator<Item = &'a PathBuf>,
) {
    flag.store(true, Ordering::Relaxed);
    for connection in connections {
        if let Some(running) = connection.lock().ok().and_then(|mut c| c.take()) {
            drop(running.stdin);
        }
    }
    for path in sockets {
        // Woken to see it's stopped, then gone: a message to it from
        // now on finds nobody, and is tried again later.
        let _ = std::os::unix::net::UnixStream::connect(path);
        let _ = std::fs::remove_file(path);
    }
}

/// The mark that agent `agent` is paused (`lnk agent pause`, before a
/// move's switch): `~/.lnk/safe/agents/<agent>/paused`, with its
/// settings, where no harness reaches. While it's there, whatever
/// carries the agent's connections takes no message: its connections' bridges
/// stop reading, so their platforms keep what comes, and its way in
/// takes nothing.
pub fn pause_mark(agent: &str) -> PathBuf {
    settings::config_of(agent).join("paused")
}

/// The mark that what carries agent `agent`'s connections has stopped
/// taking messages, for its pause.
pub fn quiet_mark(agent: &str) -> PathBuf {
    settings::config_of(agent).join("paused.quiet")
}

/// Whether agent `agent` is paused.
pub fn paused(agent: &str) -> bool {
    pause_mark(agent).exists()
}

/// Agent `agent` is no longer paused: a start, or `lnk agent resume`.
pub fn unpause(agent: &str) {
    let _ = std::fs::remove_file(pause_mark(agent));
    let _ = std::fs::remove_file(quiet_mark(agent));
}

/// How often what carries an agent looks whether it's paused.
const PAUSE_LOOK: Duration = Duration::from_millis(200);

/// Starts carrying agent `agent`'s connections and its messages from others
/// as `plan` says, running the scheduler's `runner` beside them if given
/// (none when something else runs it), until [`Carrying::stop`] ends it. With `held`, it holds a connection
/// to the harness to ask for what nobody took each time it's back; without,
/// the caller asks ([`Carrying::recover_now`]), so an idle harness is never
/// woken by it.
#[expect(
    clippy::disallowed_methods,
    reason = "polls its sockets for quiet, which nothing announces"
)]
pub fn carry(
    agent: &Settings,
    plan: &Plan,
    runner: Option<link_scheduler_tool_contract::cli::Runner>,
    held: bool,
) -> Carrying {
    let stop = Arc::new(AtomicBool::new(false));
    let quiet_now = Arc::new(AtomicBool::new(false));
    // Paused already: nothing taken, until a start or `resume`.
    if paused(&agent.name) {
        let _ = link_files::write_private(&quiet_mark(&agent.name), b"");
        return Carrying {
            stop,
            quiet: Arc::new(AtomicBool::new(true)),
            ..Carrying::none()
        };
    }
    let mut threads = Vec::new();
    let mut way_in_at = None;
    let mut kept_runner = None;
    // Messages from others (a schedule, a subagent), and the runner that
    // sends them, for a harness that reads who a message is from.
    if plan.from {
        let way_in = WayIn::new(agent, plan);
        let path = link_connection_contract::way_in(&agent.name);
        way_in_at = Some(path.clone());
        let stopped = quiet_now.clone();
        threads.push(std::thread::spawn(move || {
            if let Err(err) = way_in.listen(&path, &stopped) {
                link_format::daemon::warn!("the way in for messages from others: {err:#}");
            }
        }));
        if let Some(runner) = runner {
            let argv = runner.argv();
            kept_runner = Some(crate::kept::Kept::start(
                move || {
                    let mut command = link_secretive::process::command(&argv[0]);
                    command.args(&argv[1..]);
                    command
                },
                RUNNER_RESTART,
                crate::kept::Said("the scheduler"),
            ));
        }
        // Its background commands lost while it was down, and the ends not
        // told yet: swept once, as it starts.
        if let Some(argv) = crate::background::sweeper(agent) {
            std::thread::spawn(move || {
                if let Err(err) = link_secretive::process::command(&argv[0])
                    .args(&argv[1..])
                    .stdin(Stdio::null())
                    .status()
                {
                    link_format::daemon::warn!("sweeping the background commands: {err:#}");
                }
            });
        }
    }
    // Its webhooks' posts, on 127.0.0.1 at its port for them, for a
    // harness that says who a message is from and keeps a conversation's
    // tools: a post is a stranger's, with none of them.
    if let (Some(port), false) = (agent.webhook_port, agent.webhooks.is_empty()) {
        match plan.from && plan.tools {
            true => {
                let pushed =
                    crate::pushed::PushedIn::new(agent, plan, crate::webhooks::hooks(&agent.name));
                let at = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
                match pushed.listen(at, quiet_now.clone()) {
                    Ok(thread) => threads.push(thread),
                    Err(err) => link_format::daemon::warn!("its webhooks: {err:#}"),
                }
            }
            false => {
                link_format::daemon::info!(
                    "its webhooks take no posts: its harness can't say who a message is from, or \
                     keep a conversation's tools"
                )
            }
        }
    }
    // The answers of turns its harness finished after a crash, which
    // nobody waited for: asked for as the bridge starts, and again each
    // time the harness comes back after it couldn't be reached.
    let recovers = plan.recover && (plan.from || !plan.connections.is_empty());
    if recovers && held {
        let way_in = WayIn::new(agent, plan);
        let stopped = stop.clone();
        threads.push(std::thread::spawn(move || {
            way_in.deliver_recovered(&stopped)
        }));
    }
    let recovery = recovers.then(|| WayIn::new(agent, plan));
    // Its questions, asked on the owner's chats while it's carried.
    {
        let (way_in, name, stopped) = (WayIn::new(agent, plan), agent.name.clone(), stop.clone());
        threads.push(std::thread::spawn(move || {
            way_in.ask_owner(&name, &stopped)
        }));
    }
    // Messages a cloud that hosts agents held for it while it was paused
    // there, not given yet: given as its harness answers, as any message
    // would wake it.
    if !plan.connections.is_empty() && held_file(&agent.name).exists() {
        let (agent, plan) = (agent.duplicate(), plan.duplicate());
        let stopped = stop.clone();
        threads.push(std::thread::spawn(move || {
            give_held_until(&agent, &plan, &stopped)
        }));
    }
    // Its store bridge: the driver run here, outside the sandbox, for
    // this agent's calls alone.
    let mut store_at = None;
    if let Some(storage) = &plan.storage {
        match serve_storage(agent, storage, stop.clone()) {
            Ok(thread) => {
                threads.push(thread);
                store_at = Some(storage.socket.clone());
            }
            Err(err) => link_format::daemon::warn!("the store bridge: {err:#}"),
        }
    }
    // Its files bridge, for its teams whose folders are kept elsewhere: the
    // agent's token made and kept here, never in its sandbox.
    if let Some(files) = &plan.files {
        match serve_team_files(agent, files, stop.clone()) {
            Ok(thread) => threads.push(thread),
            Err(err) => link_format::daemon::warn!("the files bridge: {err:#}"),
        }
    }
    let mut connections = Vec::new();
    for connection in &plan.connections {
        let Some(request) = request(agent, plan, connection) else {
            link_format::daemon::warn!("{connection} isn't connected: not carrying it");
            continue;
        };
        let Some(program) = link_core::find_module(connection) else {
            link_format::daemon::warn!(
                "the {connection} module (lnk-{connection}) isn't installed: not carrying {connection}"
            );
            continue;
        };
        if let Err(err) = make_state(&request) {
            link_format::daemon::warn!("not carrying {connection}: {err:#}");
            continue;
        }
        let connection = connection.clone();
        let (running, stopped) = (Arc::new(Mutex::new(None)), quiet_now.clone());
        connections.push(running.clone());
        threads.push(std::thread::spawn(move || {
            keep(&connection, &program, &request, &running, &stopped)
        }));
    }
    // A pause, seen within a moment: its connections and way in stop taking
    // messages, and it says so; the store bridge serves the turn in
    // flight until the agent stops.
    let (watching, name) = (stop.clone(), agent.name.clone());
    let (quieted, quieting) = (quiet_now.clone(), connections.clone());
    let sockets: Vec<PathBuf> = way_in_at.iter().cloned().collect();
    threads.push(std::thread::spawn(move || {
        while !watching.load(Ordering::Relaxed) && !quieted.load(Ordering::Relaxed) {
            if paused(&name) {
                quiet(&quieted, &quieting, sockets.iter());
                let _ = link_files::write_private(&quiet_mark(&name), b"");
                return;
            }
            std::thread::sleep(PAUSE_LOOK);
        }
    }));
    Carrying {
        stop,
        quiet: quiet_now,
        connections,
        runner: kept_runner,
        way_in: way_in_at,
        store: store_at,
        recovery,
        threads,
    }
}

/// An error unless `dir` and every folder from `files` (the agent's own
/// folder) down to it is a folder, not a link: the driver runs outside
/// the sandbox there, so it must never be led elsewhere.
pub(super) fn real_folders(dir: &std::path::Path, files: &std::path::Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        files.is_absolute(),
        "the store bridge has no agent's folder to check its conversations' folder in"
    );
    let below = dir.strip_prefix(files).within(format!(
        "{} isn't in {}",
        dir.display(),
        files.display()
    ))?;
    let real = |at: &std::path::Path| -> anyhow::Result<bool> {
        match std::fs::symlink_metadata(at) {
            Ok(meta) => {
                anyhow::ensure!(
                    meta.file_type().is_dir(),
                    "{} isn't a folder (a link?): the store bridge won't use it",
                    at.display()
                );
                Ok(true)
            }
            // Not made yet: the driver makes it, in a folder checked here.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err).within(format!("looking at {}", at.display())),
        }
    };
    let mut at = files.to_path_buf();
    if !real(&at)? {
        return Ok(());
    }
    for part in below.components() {
        at.push(part);
        if !real(&at)? {
            return Ok(());
        }
    }
    Ok(())
}

/// Runs agent `agent`'s storage bridge as `storage` says, until `stop`:
/// each call proved as this agent's, by its key in this machine's
/// keyring, answered by the driver of the place its collection is kept
/// at, run here; any other agent's refused.
fn serve_storage(
    agent: &Settings,
    storage: &StoragePlan,
    stop: Arc<AtomicBool>,
) -> anyhow::Result<std::thread::JoinHandle<()>> {
    let me = agent.identity();
    // Without the agent's own folder, the check before each call checks
    // nothing: such a plan isn't run.
    anyhow::ensure!(
        storage.own.is_absolute(),
        "the storage bridge's plan names no agent's folder: lnk agent start makes it again"
    );
    // In a folder the harness's sandbox writes: what's there first (a
    // stale socket, or a link the harness put there) is replaced,
    // never followed, so it changes nothing of yours.
    let (listener, memory) =
        listen_storage(&storage.socket, &memory_storage(&me.owner, &agent.key()))?;
    let driver_for = storage.driver_for(me.clone());
    let keys: Arc<dyn link_sandbox_binding::Keys> = Arc::new(link_sandbox_binding::Keyring::here());
    if let Some(memory) = memory {
        let memory_for = storage.for_memory().driver_for(me);
        let (keys, stop) = (keys.clone(), stop.clone());
        std::thread::spawn(move || {
            link_store_contract::store::serve_storage(memory, keys, memory_for, stop)
        });
    }
    Ok(std::thread::spawn(move || {
        link_store_contract::store::serve_storage(listener, keys, driver_for, stop)
    }))
}

/// The store bridge's socket at `store`, and memory's own way in, as
/// this agent, at `memory`: a folder only memory reads, and what memory
/// may write ([`StoragePlan::for_memory`]). Memory's failing (its path
/// too long for a socket, say) is said and leaves memory without it,
/// never the agent without its store bridge: a socket bound and then let
/// go stays at its path, refusing every call.
pub(super) fn listen_storage(
    store: &std::path::Path,
    memory: &std::path::Path,
) -> anyhow::Result<(
    std::os::unix::net::UnixListener,
    Option<std::os::unix::net::UnixListener>,
)> {
    let listener = link_sandbox_bridge::socket::listen_private(store)?;
    let memory = (|| {
        if let Some(dir) = memory.parent() {
            link_files::create_private_dir(dir).within(format!("creating {}", dir.display()))?;
        }
        link_sandbox_bridge::socket::listen_private(memory)
    })();
    match memory {
        Ok(memory) => Ok((listener, Some(memory))),
        Err(err) => {
            link_format::daemon::warn!(
                "memory's way to the store bridge: {err:#}: memory reaches nothing kept off the \
                 agent's folder"
            );
            Ok((listener, None))
        }
    }
}

/// Sleeps `wait`, a little at a time: whether it was stopped meanwhile.
#[expect(
    clippy::disallowed_methods,
    reason = "a pause that ends early when told to stop"
)]
pub(super) fn sleep_unless(wait: Duration, stop: &AtomicBool) -> bool {
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100).min(until - Instant::now()));
    }
    stop.load(Ordering::Relaxed)
}

/// When the scheduler's runner is started again after it ends: soon,
/// then less often while it keeps ending.
const RUNNER_RESTART: crate::kept::Restart = crate::kept::Restart {
    after: Duration::from_secs(2),
    at_most: Duration::from_secs(60),
};

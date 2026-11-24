//! The harness behind the hub: its ACP over stdio, JSON-RPC a line at a
//! time, as the protocol's SDK client. Started on first need and again
//! when it stops; a start runs on its own, so a caller that gives up
//! waiting (a health check) doesn't stop it.

use link_format::sentence;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    InitializeRequest, InitializeResponse, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome, SessionId,
    SessionNotification, SessionUpdate,
};
use agent_client_protocol::{self as sdk, ConnectionTo, Lines};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// How long the harness has to start and agree on ACP, and a call but a
/// prompt to answer.
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(2 * 60);

/// The longest a state export or import waits for running turns to end.
pub(crate) const TURN_WAIT: Duration = Duration::from_secs(2 * 60);

/// How long a health check waits for a harness starting: Link's own
/// check gives up after 5 s, and asks again.
pub(crate) const HEALTH_WAIT: Duration = Duration::from_secs(3);

/// How long after a failed start the next waits, doubling with each
/// failure in a row up to [`RETRY_MOST`]: a harness that can't start
/// isn't started again at every health check.
const RETRY_FIRST: Duration = Duration::from_secs(1);

/// The longest wait between failed starts.
const RETRY_MOST: Duration = Duration::from_secs(60);

/// When the hub stops starting a harness that keeps failing
/// (`restarts.toml`).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Restarts {
    pub(crate) most_in_a_row: u32,
    steady_seconds: u64,
}

pub(crate) static RESTARTS: std::sync::LazyLock<Restarts> = std::sync::LazyLock::new(|| {
    toml::from_str(include_str!("restarts.toml")).expect("the hub's restarts.toml")
});

/// How long the lines a harness wrote before its stdout ended have to
/// reach their callers, before the calls still waiting are ended.
const LAST_LINES: Duration = Duration::from_millis(200);

/// The harness's two ends, and what to stop when they're dropped.
pub struct Pipes {
    pub read: Box<dyn AsyncRead + Unpin + Send>,
    pub write: Box<dyn AsyncWrite + Unpin + Send>,
    pub child: Option<Child>,
}

/// The harness's process, ended and reaped before its owner ends: tokio's
/// deferred reaper may otherwise end with the runtime, leaving the
/// host's init to reap it.
#[cfg(unix)]
pub struct Child(pub link_secretive::process::OwnedChild);

#[cfg(not(unix))]
pub struct Child(pub tokio::process::Child);

/// Captured launcher resources (a native gateway, for example) outlive
/// their ACP child, including failed initialization and held connections.
struct Native {
    _child: Option<Child>,
    _start: Arc<crate::Start>,
}

#[cfg(not(unix))]
impl Drop for Child {
    #[expect(
        clippy::disallowed_methods,
        reason = "a killed child is reaped as soon as it ends, in a drop that can't await"
    )]
    fn drop(&mut self) {
        let _ = self.0.start_kill();
        loop {
            match self.0.try_wait() {
                Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                _ => break,
            }
        }
    }
}

/// Where a session's updates go: the bridge's connection whose prompt
/// runs on it, or the hub's own reading of it, and the session as that
/// connection names it, its thread.
#[derive(Clone)]
pub(crate) struct Route {
    pub to: To,
    pub session: SessionId,
}

#[derive(Clone)]
pub(crate) enum To {
    Bridge(ConnectionTo<sdk::Client>),
    Archived(
        ConnectionTo<sdk::Client>,
        Arc<crate::continuity::Archive>,
        String,
    ),
    /// A session replayed for an export.
    Kept(tokio::sync::mpsc::UnboundedSender<SessionUpdate>),
}

type Routes = Arc<Mutex<HashMap<String, Route>>>;

type Commands = Arc<Mutex<HashMap<String, tokio::sync::watch::Sender<Option<HashSet<String>>>>>>;

/// One running harness, and the sessions it has opened.
pub(crate) struct Up {
    pub cx: ConnectionTo<sdk::Agent>,
    pub initialized: InitializeResponse,
    pub opened: Mutex<HashSet<String>>,
    routes: Routes,
    /// The commands it offers, as it last said
    /// (`available_commands_update`).
    commands: Commands,
    closed: Arc<AtomicBool>,
    /// When it agreed on ACP.
    since: std::time::Instant,
    _native: Native,
}

impl Up {
    /// Sends `session`'s updates to `route` from now on.
    pub fn route(&self, session: &str, route: Route) {
        lock(&self.routes).insert(session.to_string(), route);
    }

    /// Restore the preceding route even when a replay fails or is cancelled.
    pub fn replay_route(&self, session: &str, route: Route) -> ReplayRoute<'_> {
        let previous = lock(&self.routes).insert(session.to_string(), route);
        ReplayRoute {
            up: self,
            session: session.to_string(),
            previous,
        }
    }

    /// Native restore invalidates the process's loaded-session cache.
    pub fn forget_sessions(&self) {
        lock(&self.opened).clear();
        lock(&self.commands).clear();
    }

    /// The commands it offers, as it last said.
    pub async fn commands(&self, session: &str, wait: Duration) -> anyhow::Result<HashSet<String>> {
        let mut offered = {
            let mut commands = lock(&self.commands);
            commands
                .entry(session.to_string())
                .or_insert_with(|| tokio::sync::watch::channel(None).0)
                .subscribe()
        };
        let known = tokio::time::timeout(wait, offered.wait_for(Option::is_some))
            .await
            .map_err(|_| {
                anyhow::anyhow!("the harness has not advertised commands for session {session}")
            })?
            .map_err(|_| anyhow::anyhow!("the harness ended before advertising commands"))?;
        Ok(known.as_ref().expect("waited for commands").clone())
    }

    #[cfg(test)]
    pub fn waiting_commands(&self, session: &str) -> bool {
        lock(&self.commands)
            .get(session)
            .is_some_and(|offered| offered.receiver_count() > 0)
    }

    pub fn opened(&self, session: &str) -> bool {
        lock(&self.opened).contains(session)
    }

    pub fn open(&self, session: &str) {
        lock(&self.opened).insert(session.to_string());
    }
}

pub(crate) struct ReplayRoute<'a> {
    up: &'a Up,
    session: String,
    previous: Option<Route>,
}

impl Drop for ReplayRoute<'_> {
    fn drop(&mut self) {
        let mut routes = lock(&self.up.routes);
        match self.previous.take() {
            Some(previous) => {
                routes.insert(self.session.clone(), previous);
            }
            None => {
                routes.remove(&self.session);
            }
        }
    }
}

/// The harness, as far as it got.
#[derive(Default)]
enum Link {
    #[default]
    Down,
    Starting(Started),
    Up(Arc<Up>),
}

/// Starts that failed in a row, or whose harness ended before it ran
/// steadily.
struct Failed {
    times: u32,
    at: std::time::Instant,
    why: String,
    /// Whether the last one started and then ended: the next starts at
    /// once, as a harness that stopped does, but counts toward the limit.
    ended: bool,
}

impl Failed {
    /// How long after the last the next start waits.
    fn retry(&self) -> Duration {
        if self.ended {
            return Duration::ZERO;
        }
        RETRY_FIRST
            .saturating_mul(1 << self.times.saturating_sub(1).min(16))
            .min(RETRY_MOST)
    }

    /// Whether the hub has stopped starting it.
    fn given_up(&self) -> bool {
        self.times >= RESTARTS.most_in_a_row
    }
}

/// A start's outcome, once there is one.
type Started = tokio::sync::watch::Receiver<Option<Result<Arc<Up>, String>>>;

pub(crate) struct Harness {
    start: Arc<crate::Start>,
    link: Mutex<Link>,
    /// The starts that failed in a row, the last one's end and why.
    failed: Mutex<Option<Failed>>,
    /// How long a start, and a call but a prompt, may take.
    pub calls: Duration,
    commands: Vec<agent_client_protocol::schema::v1::AvailableCommand>,
}

impl Harness {
    pub fn new(
        start: crate::Start,
        endpoints: &link_harness_contract::sandbox::namespace::Endpoints,
    ) -> Self {
        Harness {
            start: Arc::new(start),
            link: Mutex::default(),
            failed: Mutex::default(),
            calls: CALL_TIMEOUT,
            commands: link_agent_contract::namespace::client::commands(endpoints)
                .into_iter()
                .map(|(name, description)| {
                    agent_client_protocol::schema::v1::AvailableCommand::new(name, description)
                })
                .collect(),
        }
    }

    /// Observe the existing process without starting or reconnecting one.
    /// Cancellation of old work must never wake a replacement runtime.
    pub fn running(&self) -> Option<Arc<Up>> {
        let up = match &*lock(&self.link) {
            Link::Up(up) => Some(up.clone()),
            Link::Starting(started) => match &*started.borrow() {
                Some(Ok(up)) => Some(up.clone()),
                _ => None,
            },
            _ => None,
        };
        up.filter(|up| !up.closed.load(Ordering::SeqCst))
    }

    #[cfg(test)]
    pub fn with_calls(mut self, calls: Duration) -> Self {
        self.calls = calls;
        self
    }

    /// One more start failed, now, for `why`; `ended`, it started and
    /// then ended. The one that reaches the limit says so.
    fn failed_now(&self, why: String, ended: bool) {
        let mut failed = lock(&self.failed);
        let times = failed.as_ref().map_or(0, |f| f.times) + 1;
        let now = Failed {
            times,
            at: std::time::Instant::now(),
            why,
            ended,
        };
        if now.given_up() {
            link_format::aside(
                link_format::Report::warning(sentence!(
                    "Link stopped starting the harness: {}.",
                    link_format::Foreign::new(&now.why)
                ))
                .why(sentence!(
                    "It failed to start, or ended soon after starting, {} times in a row.",
                    times.to_string()
                ))
                .next(
                    link_format::cmd!("lnk agent start"),
                    "start the agent again once it's fixed",
                ),
            );
        }
        *failed = Some(now);
    }

    /// The running harness, started (its protocol agreed) if it isn't,
    /// waiting at most `wait` for a start under way. A start that fails
    /// or takes longer than `calls` stops its harness; a later call starts
    /// another once the failures' wait is over, refused until then.
    pub async fn up_within(&self, wait: Duration) -> anyhow::Result<Arc<Up>> {
        let mut started = {
            let mut link = lock(&self.link);
            if let Link::Starting(started) = &*link {
                let done = started.borrow().clone();
                match done {
                    Some(Ok(up)) => *link = Link::Up(up),
                    Some(Err(why)) => {
                        self.failed_now(why, false);
                        *link = Link::Down;
                    }
                    None => {}
                }
            }
            let steady = Duration::from_secs(RESTARTS.steady_seconds);
            if let Link::Up(up) = &*link {
                match (
                    up.closed.load(Ordering::SeqCst),
                    up.since.elapsed() >= steady,
                ) {
                    // Running long enough: it started well.
                    (_, true) => *lock(&self.failed) = None,
                    (true, false) => self.failed_now(
                        format!(
                            "the harness ended {} s after it started",
                            up.since.elapsed().as_secs()
                        ),
                        true,
                    ),
                    (false, false) => {}
                }
            }
            match &*link {
                Link::Up(up) if !up.closed.load(Ordering::SeqCst) => return Ok(up.clone()),
                Link::Starting(started) => started.clone(),
                _ => {
                    // Not up: what was there is gone, and counted.
                    *link = Link::Down;
                    if let Some(failed) = &*lock(&self.failed) {
                        if failed.given_up() {
                            anyhow::bail!(
                                "{} (Link stopped starting it after {} tries in a row; \
                                 start the agent again once it's fixed)",
                                failed.why,
                                failed.times
                            );
                        }
                        let left = failed.retry().saturating_sub(failed.at.elapsed());
                        if !left.is_zero() {
                            anyhow::bail!(
                                "{} (failed {} times in a row; it starts again in {} s)",
                                failed.why,
                                failed.times,
                                left.as_secs().max(1)
                            );
                        }
                    }
                    let mut pipes = match (self.start)() {
                        Ok(pipes) => pipes,
                        Err(err) => {
                            self.failed_now(format!("{err:#}"), false);
                            return Err(err.into());
                        }
                    };
                    let native = Native {
                        _child: pipes.child.take(),
                        _start: self.start.clone(),
                    };
                    let (done, started) = tokio::sync::watch::channel(None);
                    let calls = self.calls;
                    let commands = self.commands.clone();
                    tokio::spawn(async move {
                        let up =
                            tokio::time::timeout(calls, connect(pipes, native, commands)).await;
                        let up = match up {
                            Ok(Ok(up)) => Ok(up),
                            Ok(Err(err)) => Err(format!("{err:#}")),
                            Err(_) => Err(format!(
                                "the harness didn't agree on ACP within {} s",
                                calls.as_secs()
                            )),
                        };
                        let _ = done.send(Some(up));
                    });
                    *link = Link::Starting(started.clone());
                    started
                }
            }
        };
        let done = tokio::time::timeout(wait, started.wait_for(Option::is_some)).await;
        let up = match done {
            Ok(Ok(done)) => done.clone().expect("waited for it"),
            Ok(Err(_)) => Err("the harness's start ended without an answer".into()),
            Err(_) => Err(format!(
                "the harness is still starting after {} s",
                wait.as_secs_f32()
            )),
        };
        up.map_err(|err| anyhow::anyhow!(err))
    }
}

/// Connects to the harness at `pipes` and agrees on ACP. Its connection
/// runs in a task of its own, which ends when the harness's stdout does:
/// then it's closed, and the next message starts another.
#[expect(
    clippy::disallowed_methods,
    reason = "its last lines are given a moment to be dispatched: the SDK tells no one its queue is empty"
)]
async fn connect(
    pipes: Pipes,
    native: Native,
    namespace_commands: Vec<agent_client_protocol::schema::v1::AvailableCommand>,
) -> anyhow::Result<Arc<Up>> {
    let routes = Routes::default();
    let commands = Commands::default();
    let closed = Arc::new(AtomicBool::new(false));
    let (tell, told) = tokio::sync::oneshot::channel();
    let (ended_tx, mut ended_rx) = tokio::sync::watch::channel(false);
    let transport = lines::<sdk::Client>(pipes.read, pipes.write, Some(ended_tx));
    tokio::spawn({
        let routes = routes.clone();
        let commands = commands.clone();
        let closed = closed.clone();
        let closing = closed.clone();
        async move {
            let ended = sdk::Client
                .builder()
                .name("link-hub")
                .on_receive_notification(
                    async move |mut said: SessionNotification, _cx| {
                        if let SessionUpdate::AvailableCommandsUpdate(update) = &said.update {
                            let offered = update
                                .available_commands
                                .iter()
                                .map(|c| c.name.clone())
                                .collect();
                            lock(&commands)
                                .entry(said.session_id.0.to_string())
                                .or_insert_with(|| tokio::sync::watch::channel(None).0)
                                .send_replace(Some(offered));
                        }
                        if let SessionUpdate::AvailableCommandsUpdate(update) = &mut said.update {
                            for command in &namespace_commands {
                                if !update
                                    .available_commands
                                    .iter()
                                    .any(|native| native.name == command.name)
                                {
                                    update.available_commands.push(command.clone());
                                }
                            }
                        }
                        let route = lock(&routes).get(&*said.session_id.0).cloned();
                        match route.map(|route| (route.to, route.session)) {
                            Some((To::Archived(to, archive, thread), session)) => {
                                archive
                                    .append(&thread, vec![said.update.clone()], None)
                                    .map_err(|error| {
                                        sdk::Error::internal_error().data(error.to_string())
                                    })?;
                                said.session_id = session;
                                let _ = to.send_notification(said);
                            }
                            Some((To::Bridge(to), session)) => {
                                let mut said = said;
                                said.session_id = session;
                                // The bridge gone: the turn runs on, unheard.
                                let _ = to.send_notification(said);
                            }
                            Some((To::Kept(kept), _)) => {
                                let _ = kept.send(said.update);
                            }
                            None => {}
                        }
                        Ok(())
                    },
                    sdk::on_receive_notification!(),
                )
                .on_receive_request(
                    async move |asked: RequestPermissionRequest, responder, _cx| {
                        responder.respond(allowed(&asked))
                    },
                    sdk::on_receive_request!(),
                )
                .connect_with(transport, async move |cx: ConnectionTo<sdk::Agent>| {
                    let initialized = cx
                        .send_request(InitializeRequest::new(ProtocolVersion::V1))
                        .block_task()
                        .await?;
                    let _ = tell.send((cx, initialized));
                    // Until its stdout ends: then it's closed, and once
                    // the lines it wrote last are dispatched, every call
                    // still waiting on it ends too.
                    let _ = ended_rx.wait_for(|ended| *ended).await;
                    closing.store(true, Ordering::SeqCst);
                    tokio::time::sleep(LAST_LINES).await;
                    Err::<(), _>(sdk::Error::internal_error().data("the harness's output ended"))
                })
                .await;
            if let Err(err) = ended {
                crate::warned(sentence!("The harness's ACP ended."), &err);
            }
            closed.store(true, Ordering::SeqCst);
        }
    });
    let (cx, initialized) = told
        .await
        .map_err(|_| anyhow::anyhow!("the harness ended before it agreed on ACP"))?;
    Ok(Arc::new(Up {
        cx,
        initialized,
        opened: Mutex::default(),
        routes,
        commands,
        closed,
        since: std::time::Instant::now(),
        _native: native,
    }))
}

/// What a permission's asked is answered: the first option that allows,
/// since Link's sandbox, not the harness, is what confines it; none, the
/// turn goes on without.
fn allowed(asked: &RequestPermissionRequest) -> RequestPermissionResponse {
    let allows = asked.options.iter().find(|option| {
        matches!(
            option.kind,
            PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
        )
    });
    RequestPermissionResponse::new(match allows {
        Some(option) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            option.option_id.clone(),
        )),
        None => RequestPermissionOutcome::Cancelled,
    })
}

/// ACP's lines over a harness's stdout and stdin, `ended` told when its
/// stdout ends.
pub(crate) fn lines<R: sdk::Role>(
    read: Box<dyn AsyncRead + Unpin + Send>,
    write: Box<dyn AsyncWrite + Unpin + Send>,
    ended: Option<tokio::sync::watch::Sender<bool>>,
) -> impl sdk::ConnectTo<R> {
    let incoming = Box::pin(futures_util::stream::unfold(
        (BufReader::new(read).lines(), ended),
        async |(mut lines, ended)| match lines.next_line().await {
            Ok(Some(line)) => Some((Ok(line), (lines, ended))),
            Ok(None) => {
                if let Some(ended) = ended {
                    let _ = ended.send(true);
                }
                None
            }
            Err(err) => Some((Err(err), (lines, ended))),
        },
    ));
    let outgoing = Box::pin(futures_util::sink::unfold(
        write,
        async |mut write, line: String| {
            write.write_all(line.as_bytes()).await?;
            write.write_all(b"\n").await?;
            write.flush().await?;
            Ok::<_, std::io::Error>(write)
        },
    ));
    Lines::new(outgoing, incoming)
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

//! The hub: a harness that speaks ACP over stdio, served to Link's
//! bridges as ACP over a WebSocket at its endpoint's `/acp`
//! ([`link_connection_contract::acp::ACP_PATH`]), as Link Harness serves
//! it. It's a package, never a process of its own: each adapter's relay
//! is built on it, in the adapter's own `serve`.
//!
//! One harness runs behind it, started on the first message and again
//! when it stops. Each of Link's threads is a session the harness made,
//! which thread is which session kept in a file, so a thread goes on
//! where it was after a restart; a thread's turns run one at a time. A
//! prompt is told taken (`_link/taken`) as it's forwarded, and its
//! sender, when it isn't the owner, is written into its text, so a
//! harness that reads no `_meta` still knows who it's answering. Link's
//! tools and memory go with each session, as its `mcpServers`
//! ([`Tools`]). Agent state is carried over ASP, by the harness or its
//! adapter's [`Store`].

mod activity;
mod continuity;
mod crossing;
mod harness;
mod serving;
mod sessions;
mod state;
#[cfg(test)]
mod tests;
mod tools;

use link_format::{Address, Note, Prose, Report, Sentence, aside, sentence};
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use link_harness_contract::harness as contract;
use serde_json::json;

pub use harness::{Child, Pipes};
pub use state::Store;
pub use tools::Tools;

/// The hub's formats, which each harness serving through it owns.
pub const SAVED: [fn() -> link_files::upgrade::Saved; 2] = [sessions::saved, tools::saved];

/// Offline personal capture uses the same archive and source routing as a
/// running hub. Native files remain adapter-owned and unchanged.
pub fn export_personal(
    home: &std::path::Path,
    adapter: &str,
    sessions: PathBuf,
    mut state: link_harness_contract::asp::State,
) -> anyhow::Result<link_harness_contract::asp::State> {
    if let Some(incoming) = state.sessions.take() {
        let archive =
            continuity::Archive::at(home, contract::continuation::Context::archival(adapter)?)?;
        let bindings = sessions::Sessions::at(sessions);
        let mut seen = std::collections::BTreeSet::new();
        for mut session in incoming {
            let native = session.session_id.0.to_string();
            anyhow::ensure!(
                seen.insert(native.clone()),
                "native export repeats a session identity"
            );
            session.session_id = bindings
                .thread_of(&native)?
                .unwrap_or_else(|| contract::continuation::native_thread(adapter, &native))
                .into();
            archive.backfill(session)?;
        }
        state.sessions = Some(archive.sessions()?);
    }
    Ok(state)
}

/// Starts the harness: its ACP's two ends, and the process to stop when
/// they're dropped.
pub type Start = Box<dyn Fn() -> std::io::Result<Pipes> + Send + Sync>;

/// A [`Start`] running `command` with its stdin and stdout as its ACP,
/// ended with the hub.
pub fn spawning(command: impl Fn() -> std::process::Command + Send + Sync + 'static) -> Start {
    Box::new(move || {
        #[cfg(unix)]
        let (read, write, child, pid) = {
            let mut command = command();
            command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped());
            // Keep exclusive wait ownership until the complete ACP group
            // is stopped. A native launcher may retain its real client in
            // a child process, even after that launcher has exited.
            let mut child =
                link_secretive::process::OwnedChild::spawn(&mut command).map_err(|err| {
                    std::io::Error::new(err.kind(), format!("starting the harness: {err}"))
                })?;
            let process = child.child();
            let pid = process.id();
            let read = process
                .stdout
                .take()
                .ok_or_else(|| std::io::Error::other("the harness's stdout"))?;
            let write = process
                .stdin
                .take()
                .ok_or_else(|| std::io::Error::other("the harness's stdin"))?;
            (
                Box::new(tokio::process::ChildStdout::from_std(read)?)
                    as Box<dyn tokio::io::AsyncRead + Unpin + Send>,
                Box::new(tokio::process::ChildStdin::from_std(write)?)
                    as Box<dyn tokio::io::AsyncWrite + Unpin + Send>,
                Child(child),
                pid,
            )
        };
        #[cfg(not(unix))]
        let (read, write, child, pid) = {
            let mut child = tokio::process::Command::from(command())
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|err| {
                    std::io::Error::new(err.kind(), format!("starting the harness: {err}"))
                })?;
            let (read, write) = (child.stdout.take(), child.stdin.take());
            let (Some(read), Some(write)) = (read, write) else {
                return Err(std::io::Error::other("the harness's pipes"));
            };
            let pid = child.id().unwrap_or_default();
            (
                Box::new(read) as Box<dyn tokio::io::AsyncRead + Unpin + Send>,
                Box::new(write) as Box<dyn tokio::io::AsyncWrite + Unpin + Send>,
                Child(child),
                pid,
            )
        };
        aside(Note(sentence!(
            "Started the harness as process {}.",
            pid.to_string()
        )));
        Ok(Pipes {
            read,
            write,
            child: Some(child),
        })
    })
}

/// What a hub is made of.
pub struct Config {
    /// Starts the harness's ACP.
    pub start: Start,
    /// Where its sessions work: the agent's files.
    pub cwd: String,
    /// What a connection must carry (`Authorization: Bearer <key>`),
    /// and what it proves it holds; none, it takes any.
    pub key: Option<link_secretive::Secret>,
    /// Its agent's name: a connection naming another
    /// ([`link_connection_contract::AGENT_HEADER`]) is refused. None, it
    /// takes any.
    pub agent: Option<String>,
    /// Where each thread's session is kept.
    pub sessions: PathBuf,
    /// The harness's own store of agent state, for a harness whose ACP
    /// speaks none; none, it carries only what the harness speaks.
    pub state: Option<Arc<dyn Store>>,
    /// Link's tools and memory, given to the harness in each session.
    pub tools: Tools,
}

pub struct Hub {
    harness: harness::Harness,
    sessions: sessions::Sessions,
    key: Option<link_secretive::Secret>,
    agent: Option<String>,
    cwd: String,
    activity: activity::Activity,
    state: Option<Arc<dyn Store>>,
    tools: Tools,
    continuity: Option<Arc<continuity::Archive>>,
}

impl Hub {
    pub fn new(config: Config) -> Arc<Self> {
        Self::configured(config, false)
    }

    /// Serialize foreground native operations across all saved sessions.
    /// Native children and background work still require runtime confinement.
    pub fn single_session(config: Config) -> Arc<Self> {
        Self::configured(config, true)
    }

    fn configured(config: Config, single_session: bool) -> Arc<Self> {
        Arc::new(Hub {
            harness: harness::Harness::new(config.start, &config.tools.actions),
            sessions: sessions::Sessions::at(config.sessions),
            key: config.key,
            agent: config.agent,
            cwd: config.cwd,
            activity: activity::Activity::new(single_session),
            state: config.state,
            tools: config.tools,
            continuity: None,
        })
    }

    /// Personal adapters opt into full portable capture and bounded context
    /// continuation when their native runtime cannot import foreign sessions.
    pub fn with_continuity(self: Arc<Self>, home: &std::path::Path) -> anyhow::Result<Arc<Self>> {
        let mut hub =
            Arc::into_inner(self).expect("continuity is configured before sharing the hub");
        let context = contract::continuation::Context::load(home)?;
        hub.continuity = Some(Arc::new(continuity::Archive::at(home, context)?));
        Ok(Arc::new(hub))
    }

    /// Less time for the harness to start and answer (tests).
    #[cfg(test)]
    fn with_calls(self: Arc<Self>, calls: std::time::Duration) -> Arc<Self> {
        let mut hub = Arc::into_inner(self).expect("not shared yet");
        hub.harness = hub.harness.with_calls(calls);
        Arc::new(hub)
    }

    /// Whether the harness runs and answers, waiting a little for one
    /// starting.
    pub async fn healthy(&self) -> bool {
        self.sessions.check().is_ok()
            && self
                .continuity
                .as_ref()
                .is_none_or(|archive| archive.check().is_ok())
            && self.harness.up_within(harness::HEALTH_WAIT).await.is_ok()
    }

    /// Its endpoint: `/health`, the key's proof and `/acp`.
    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/health", get(health))
            .route(contract::PROOF_PATH, get(prove))
            .route(link_connection_contract::acp::ACP_PATH, get(acp))
            .with_state(self)
    }

    /// Serves its endpoint on `127.0.0.1:port` until the process is
    /// stopped (`SIGINT`, `SIGTERM`, `SIGHUP`), each answer sent at once
    /// (`TCP_NODELAY`), as the contract asks. Stopped, it stops the
    /// harness with it, which a signal's default end would leave running.
    pub fn serve(self: Arc<Self>, port: u16) -> anyhow::Result<()> {
        use std::future::IntoFuture;
        use tokio::signal::unix::{SignalKind, signal};
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
                .await
                .map_err(|err| anyhow::anyhow!("listening on 127.0.0.1:{port}: {err}"))?;
            aside(Note(sentence!(
                "The harness answers on {}.",
                Address::new(format!("127.0.0.1:{port}"))
            )));
            let listener = axum::serve::ListenerExt::tap_io(listener, |tcp| {
                let _ = tcp.set_nodelay(true);
            });
            let (mut interrupt, mut term, mut hangup) = (
                signal(SignalKind::interrupt())?,
                signal(SignalKind::terminate())?,
                signal(SignalKind::hangup())?,
            );
            // Returning drops every task, and with them the harness.
            tokio::select! {
                served = axum::serve(listener, self.router()).into_future() => served?,
                _ = interrupt.recv() => aside(Note(sentence!("Stopping the harness."))),
                _ = term.recv() => aside(Note(sentence!("Stopping the harness."))),
                _ = hangup.recv() => aside(Note(sentence!("Stopping the harness."))),
            }
            anyhow::Ok(())
        })?;
        runtime.shutdown_timeout(std::time::Duration::from_secs(2));
        Ok(())
    }

    /// Whether `headers` carry its key: never where it has none.
    fn keyed(&self, headers: &HeaderMap) -> bool {
        let Some(key) = &self.key else {
            return false;
        };
        let sent = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or_default();
        // `Secret`'s own comparison, in constant time.
        link_secretive::Secret::from(sent.to_string()) == *key
    }

    /// Refused, and why, when `headers` name an agent that isn't its own.
    fn its_agent(&self, headers: &HeaderMap) -> Result<(), (StatusCode, &'static str)> {
        let (Some(own), Some(named)) = (
            &self.agent,
            headers.get(link_connection_contract::AGENT_HEADER),
        ) else {
            return Ok(());
        };
        match named.to_str() {
            Ok(named) if named == own => Ok(()),
            Ok(_) => Err((StatusCode::NOT_FOUND, "no such agent here")),
            Err(_) => Err((StatusCode::BAD_REQUEST, "that Link-Agent isn't a name")),
        }
    }
}

/// Proves to Link's connection that this is the agent's endpoint, before it
/// sends the key: the challenge signed with the key
/// ([`contract::endpoint_proof`]), never the key itself.
async fn prove(State(hub): State<Arc<Hub>>, headers: HeaderMap) -> Response {
    let challenge = headers
        .get(contract::CHALLENGE_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|c| contract::is_challenge(c));
    let Some(challenge) = challenge else {
        return refuse(StatusCode::BAD_REQUEST, "no challenge");
    };
    let Some(key) = &hub.key else {
        return refuse(StatusCode::NOT_FOUND, "no key to prove");
    };
    let proof = contract::endpoint_proof(key, challenge);
    ([(contract::PROOF_HEADER, proof)], "").into_response()
}

async fn health(State(hub): State<Arc<Hub>>) -> Response {
    match hub.healthy().await {
        true => "ok".into_response(),
        false => refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "the harness isn't answering",
        ),
    }
}

/// An ACP connection, opened with the agent's headers and its key.
async fn acp(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    upgrade: axum::extract::ws::WebSocketUpgrade,
) -> Response {
    if let Err((status, why)) = hub.its_agent(&headers) {
        return refuse(status, why);
    }
    // With no key set, it serves nothing, as Link Harness does: an
    // endpoint any program of the machine could open.
    if hub.key.is_none() {
        return refuse(StatusCode::SERVICE_UNAVAILABLE, "no key is set");
    }
    if !hub.keyed(&headers) {
        return refuse(StatusCode::UNAUTHORIZED, "wrong or missing key");
    }
    upgrade
        .max_message_size(BODY_MAX)
        .on_upgrade(move |socket| serving::serve(hub, socket))
}

/// The most a message holds: its images and documents, in base64.
const BODY_MAX: usize = 48 << 20;

fn refuse(status: StatusCode, why: &str) -> Response {
    (status, Json(json!({"error": {"message": why}}))).into_response()
}

/// Says a fact about the run, `err`'s words as its why: the hub holds no
/// `Out`.
pub(crate) fn warned(headline: Sentence, err: &dyn std::fmt::Display) {
    aside(Report::warning(headline).why(Prose::from(Sentence::adopt(&err.to_string()))));
}

#[cfg(test)]
link_test_kit::program!(system);

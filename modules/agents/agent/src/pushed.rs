//! The bridge's pushed way in: posts from outside for one agent, each to
//! `/h/<hook id>` on a port of 127.0.0.1, and nowhere else. Each is
//! capped before it's read, checked by its hook's connection module
//! (`lnk-<connection> sift`, [`link_connection_contract::pushed`]), checked for
//! a replay, and handed to the agent by the bridge itself, through its way
//! in ([`crate::bridge::WayIn`]): as from whom the hook says, never what
//! the module says, so with a stranger's standing and a stranger's tools
//! (`[tools] strangers`). The module holds neither the endpoint nor its
//! secret.
//! It names no connection: what a hook is, and which module checks it, is
//! its [`Hook`]'s.

use link_format::WithinExt;
use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use link_connection_contract::pushed::{Sifting, UPDATE_MAX, Verdict};
use link_secretive::Duplicate;
use link_secretive::inbound::{LONGEST_KEEP, Refused};
use serde::{Deserialize, Serialize};

/// What the pushed way in takes at once, and waits for: as many posts at
/// once as the way in takes messages ([`crate::bridge::WAY_IN`]), each
/// body read within its wait, and each `sift` answered within its.
const LIMITS: link_sandbox_binding::Limits = crate::bridge::WAY_IN;

/// A hook: which connection's module checks its posts, what `sift` is handed
/// of it ([`Sifting::settings`]: none of it secret), the secret its posts
/// are signed with, and the one before it while it rolls; and how each
/// post it takes reaches the agent: from whom (`Said::from`, never the
/// owner), in which conversation.
#[derive(Duplicate, Debug)]
pub struct Hook {
    pub connection: String,
    pub settings: serde_json::Value,
    #[duplicate]
    pub secret: link_secretive::Secret,
    #[duplicate]
    pub previous: Option<link_secretive::Secret>,
    pub from: String,
    pub thread: String,
    /// How many questions it asks the owner a day, at most.
    pub questions_per_day: u32,
}

impl Hook {
    /// Its webhook's name: what its messages are from, after `webhook:`.
    pub fn name(&self) -> &str {
        self.from.strip_prefix("webhook:").unwrap_or(&self.from)
    }
}

/// The agent's hooks, by id, as they are when a post comes: read at each
/// post, so a secret rolled is taken with no restart.
pub type Hooks = Arc<dyn Fn(&str) -> Option<Hook> + Send + Sync>;

/// Where a connection's module is, by the connection's name.
pub type Find = Arc<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>;

/// How a post a hook took reaches the agent: a message from someone who
/// isn't its owner, answered nowhere; done once the harness has taken it,
/// never waiting for its turn to end.
pub type Deliver = Arc<
    dyn Fn(
            link_connection_contract::Firing,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>>
        + Send
        + Sync,
>;

/// Hands each post to `way_in`: delivered once its harness has taken it,
/// the turn running on and its answer (sent nowhere) read apart.
pub(crate) fn delivering(way_in: Arc<crate::bridge::WayIn>) -> Deliver {
    Arc::new(move |firing| {
        let way_in = way_in.clone();
        Box::pin(async move {
            let taken = way_in.take(firing).await?;
            tokio::spawn(async move {
                if let Err(err) = way_in.finish(taken).await {
                    link_format::daemon::warn!("a hook's post, taken: its turn: {err:#}");
                }
            });
            Ok(())
        })
    })
}

/// Whether `id` is the shape of a hook's id: hex, at least 128 bits of
/// it. Anything else is no hook's, looked up nowhere.
pub fn is_hook_id(id: &str) -> bool {
    (32..=128).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The pushed way in of one agent.
pub struct PushedIn {
    hooks: Hooks,
    /// How a post it took reaches the agent: the bridge's own way in,
    /// never a module.
    deliver: Deliver,
    /// Where each connection's module is.
    find: Find,
    seen: Arc<Seen>,
    /// Posts sifted or handed to the agent now, at most
    /// [`LIMITS`]`.calls`.
    calls: Arc<tokio::sync::Semaphore>,
    /// Where its webhooks' questions are asked of the owner; None: a
    /// question is a message like any other.
    questions: Option<questions::Questions>,
}

impl PushedIn {
    /// Agent `agent`'s, each post it takes handed to its way in as `plan`
    /// says ([`crate::bridge::WayIn`], whose checks a stranger's message
    /// passes), its hooks as `hooks` finds them, and what it took kept in
    /// its settings' folder.
    pub fn new(
        agent: &crate::settings::Settings,
        plan: &crate::bridge::Plan,
        hooks: Hooks,
    ) -> Self {
        Self::with(
            delivering(Arc::new(crate::bridge::WayIn::new(agent, plan))),
            hooks,
            Arc::new(link_core::find_module),
            Seen::at(crate::settings::config_of(&agent.name).join(SEEN_FILE)),
        )
        .asking(questions::Questions::of(agent))
    }

    /// The same, its webhooks' questions asked as `question list` says.
    pub fn asking(self, questions: Option<questions::Questions>) -> Self {
        Self { questions, ..self }
    }

    /// One handing what it takes to `deliver`, with `hooks`, each
    /// connection's module found by `find`, and what it took kept in `seen`.
    pub fn with(deliver: Deliver, hooks: Hooks, find: Find, seen: Seen) -> Self {
        Self {
            hooks,
            deliver,
            find,
            seen: Arc::new(seen),
            calls: Arc::new(tokio::sync::Semaphore::new(LIMITS.calls)),
            questions: None,
        }
    }

    /// Takes posts at `at`, which must be 127.0.0.1 (any other address,
    /// `0.0.0.0` among them, is refused: what reaches it from outside is a
    /// tunnel), until `stop` says. Bound before it returns the listener's
    /// thread, so a post right after is taken.
    pub fn listen(
        self,
        at: SocketAddr,
        stop: Arc<AtomicBool>,
    ) -> anyhow::Result<std::thread::JoinHandle<()>> {
        anyhow::ensure!(
            at.ip() == std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
            "the pushed way in listens on 127.0.0.1 alone, never {}",
            at.ip()
        );
        let listener = std::net::TcpListener::bind(at)
            .within(format!("listening on {at} for the agent's webhooks"))?;
        self.serve(listener, stop)
    }

    /// Takes posts on `listener`, bound already, which must be on
    /// 127.0.0.1, until `stop` says.
    #[expect(clippy::disallowed_methods, reason = "polls the stop flag")]
    pub fn serve(
        self,
        listener: std::net::TcpListener,
        stop: Arc<AtomicBool>,
    ) -> anyhow::Result<std::thread::JoinHandle<()>> {
        let at = listener.local_addr()?;
        anyhow::ensure!(
            at.ip() == std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
            "the pushed way in listens on 127.0.0.1 alone, never {}",
            at.ip()
        );
        listener.set_nonblocking(true)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let app = self.router();
        Ok(std::thread::spawn(move || {
            runtime.block_on(async move {
                let listener = match tokio::net::TcpListener::from_std(listener) {
                    Ok(listener) => listener,
                    Err(err) => {
                        return link_format::daemon::warn!("the pushed way in: {err:#}");
                    }
                };
                let stopped = async move {
                    while !stop.load(Ordering::Relaxed) {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                };
                if let Err(err) = axum::serve(listener, app)
                    .with_graceful_shutdown(stopped)
                    .await
                {
                    link_format::daemon::warn!("the pushed way in: {err:#}");
                }
            })
        }))
    }

    /// Its routes: `POST /h/<hook id>`, and `GET /h/<hook id>/questions/<id>`,
    /// a question's status; nothing else.
    pub fn router(self) -> Router {
        Router::new()
            .route("/h/{id}", post(posted))
            .route(
                "/h/{id}/questions/{question}",
                axum::routing::get(question_status),
            )
            .with_state(Arc::new(self))
    }
}

/// Question `question`'s status, for its webhook's poster: a `GET` signed
/// as its posts are.
async fn question_status(
    State(way_in): State<Arc<PushedIn>>,
    Path((id, question)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let (Some(questions), Some(hook)) = (
        &way_in.questions,
        Some(&id)
            .filter(|id| is_hook_id(id))
            .and_then(|id| (way_in.hooks)(id)),
    ) else {
        return status(404);
    };
    let headers: BTreeMap<String, String> = headers
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_string(), value.to_str().ok()?.into())))
        .collect();
    if let Err(code) = questions::signed(&hook, &headers) {
        return status(code);
    }
    match questions.status(&hook, &question) {
        Some(asked) => axum::Json(asked).into_response(),
        None => status(404),
    }
}

/// One post to hook `id`.
async fn posted(
    State(way_in): State<Arc<PushedIn>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    // Which hook, before anything is read of the post or a place is held
    // for it: a stranger's guess holds nothing.
    let Some(hook) = Some(&id)
        .filter(|id| is_hook_id(id))
        .and_then(|id| (way_in.hooks)(id))
    else {
        return status(404);
    };
    // Capped before anything else is read of it.
    let declared = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|len| len > UPDATE_MAX as u64) {
        return status(Refused::TooBig.status());
    }
    let Ok(permit) = way_in.calls.clone().try_acquire_owned() else {
        return status(503);
    };
    let body = match tokio::time::timeout(
        LIMITS.action_wait,
        axum::body::to_bytes(body, UPDATE_MAX),
    )
    .await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => return status(Refused::TooBig.status()),
        Err(_) => return status(408),
    };
    let Ok(body) = String::from_utf8(body.to_vec()) else {
        return status(Refused::Malformed.status());
    };
    let Some(program) = (way_in.find)(&hook.connection) else {
        link_format::daemon::info!(
            "a post to a hook of {}: its module (lnk-{}) isn't installed",
            link_format::clean(&hook.connection),
            link_format::clean(&hook.connection)
        );
        return status(Refused::Unconfigured.status());
    };
    // Its headers, their names in lowercase (as `HeaderMap` holds them).
    let headers: BTreeMap<String, String> = headers
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_string(), value.to_str().ok()?.into())))
        .collect();
    let verdict = match sift(&program, &hook, &headers, &body).await {
        Ok(verdict) => verdict,
        Err(err) => {
            link_format::daemon::warn!(
                "sifting a post with lnk-{}: {err:#}",
                link_format::clean(&hook.connection)
            );
            return status(503);
        }
    };
    match verdict {
        Verdict::Drop { status: code } => status(code),
        Verdict::Answer {
            status: code,
            content_type,
            body,
        } => (
            StatusCode::from_u16(code).unwrap_or(StatusCode::OK),
            [(axum::http::header::CONTENT_TYPE, content_type)],
            body,
        )
            .into_response(),
        // Who it's from and where it goes are the hook's, never what the
        // module says.
        Verdict::Take { dedupe, .. } => {
            // A post with no id would never be seen again: a module can't
            // turn the check of replays off.
            if dedupe.is_empty() {
                link_format::daemon::info!(
                    "lnk-{} took a post with no id: it isn't taken",
                    link_format::clean(&hook.connection)
                );
                return status(Refused::Unavailable.status());
            }
            // Pending while it's handed to the agent, so the same post at
            // once is refused; seen only once the agent has it.
            let pending = match Seen::begin(&way_in.seen, &id, &dedupe, now()) {
                Ok(Some(pending)) => pending,
                Ok(None) => return status(Refused::Replayed.status()),
                Err(err) => {
                    link_format::daemon::warn!("reading what a hook took: {err:#}");
                    return status(Refused::Unavailable.status());
                }
            };
            // A question asks the owner, the agent never woken.
            if let Some(questions) = &way_in.questions
                && questions::is_question(&body)
            {
                drop(permit);
                return match questions.ask(&hook, &body) {
                    Ok(asked) => {
                        if let Err(err) = pending.settle(now()) {
                            link_format::daemon::warn!("keeping what a hook took: {err:#}");
                        }
                        (StatusCode::ACCEPTED, axum::Json(asked)).into_response()
                    }
                    Err(code) => status(code),
                };
            }
            // From the hook's sender, in its own conversation, answered
            // nowhere: the poster is told only that it was taken.
            let firing = link_connection_contract::Firing {
                parent: None,
                delivery: None,
                occurrence: None,
                thread: hook.thread.clone(),
                from: hook.from.clone(),
                text: body,
                answer: link_connection_contract::AnswerTo::Nowhere,
                hops: 0,
                tools: None,
                model: None,
                standing: link_connection_contract::access::Standing::of(Some(&hook.from)),
            };
            let delivered = (way_in.deliver)(firing).await;
            drop(permit);
            match delivered {
                Ok(()) => {
                    // The agent has it: a failure to keep it now only
                    // lets a redelivery in, which is said.
                    if let Err(err) = pending.settle(now()) {
                        link_format::daemon::warn!("keeping what a hook took: {err:#}");
                    }
                    status(202)
                }
                // Not delivered: let go, so the sender's redelivery is
                // taken.
                Err(err) => {
                    link_format::daemon::warn!(
                        "a post to a hook of {}: {err:#}",
                        link_format::clean(&hook.connection)
                    );
                    status(503)
                }
            }
        }
    }
}

fn status(code: u16) -> Response {
    StatusCode::from_u16(code)
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        .into_response()
}

/// `hook`'s module's verdict on a post, sifted with its secret and, while
/// it rolls, the one before it.
async fn sift(
    program: &std::path::Path,
    hook: &Hook,
    headers: &BTreeMap<String, String>,
    body: &str,
) -> anyhow::Result<Verdict> {
    let line = Sifting {
        settings: hook.settings.clone(),
        secret: hook.secret.duplicate(),
        previous: hook
            .previous
            .as_ref()
            .map(link_secretive::Secret::duplicate)
            .unwrap_or_default(),
        headers: headers.clone(),
        body: body.to_string(),
        say: None,
        // A webhook is shared with no chat: its posts are a stranger's.
        shared: Vec::new(),
    };
    let mut input = serde_json::to_vec(&line)?;
    input.push(b'\n');
    // Handed nothing: it reads the hook's own secret on its stdin, and no
    // token or secret of this process's.
    let mut command = tokio::process::Command::from(link_secretive::process::command(program));
    command
        .arg("sift")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .within(format!("running {}", program.display()))?;
    let mut stdin = child.stdin.take().within("no stdin")?;
    let output = tokio::time::timeout(LIMITS.answer_wait, async move {
        use tokio::io::AsyncWriteExt as _;
        stdin.write_all(&input).await?;
        drop(stdin);
        child.wait_with_output().await
    })
    .await
    .within("sift took too long")??;
    let line = output
        .stdout
        .split(|b| *b == b'\n')
        .find(|line| !line.is_empty())
        .within("sift gave no verdict")?;
    serde_json::from_slice(line).within("reading sift's verdict")
}

/// Seconds since 1970.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Where what an agent's hooks took is kept, in its settings' folder.
pub const SEEN_FILE: &str = "hooks-seen.json";

/// What an agent's hooks took, by the id each post's module gave it
/// ([`Verdict::Take`]'s `dedupe`), each kept for [`LONGEST_KEEP`] once
/// it reached the agent: a repeat inside it is refused, and so is one
/// while the first is being handed to the agent ([`Pending`]). Kept in a
/// file of mode 600, read without following a link and written whole (a
/// file beside it renamed over it), the expired ones dropped as it's
/// written.
pub struct Seen {
    path: PathBuf,
    /// The posts being handed to the agent now, by hook and id.
    pending: Mutex<std::collections::BTreeSet<(String, String)>>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeenList {
    taken: Vec<Taken>,
}

link_files::specification!(SeenList, include_str!("pushed/seen.schema.toml"));

/// The hook posts taken, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<SeenList>()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Taken {
    hook: String,
    id: String,
    until: u64,
}

/// A post being handed to the agent ([`Seen::begin`]): kept by
/// [`Pending::settle`] once it reached it, else let go as it's dropped,
/// so a redelivery is taken.
pub struct Pending {
    seen: Arc<Seen>,
    /// Its hook and id, until it's settled or let go.
    post: Option<(String, String)>,
}

impl Pending {
    /// It reached the agent at `now`: kept, so a repeat is refused.
    pub fn settle(mut self, now: u64) -> anyhow::Result<()> {
        let Some((hook, id)) = self.post.take() else {
            return Ok(());
        };
        let mut pending = self.seen.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.remove(&(hook.clone(), id.clone()));
        let mut list = self.seen.read()?;
        list.taken.retain(|t| t.until > now);
        list.taken.push(Taken {
            hook,
            id,
            until: now + LONGEST_KEEP,
        });
        link_files::write_private(&self.seen.path, &link_files::saved::print_json(&list)?)
            .within(format!("writing {}", self.seen.path.display()))
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(post) = self.post.take() {
            let mut pending = self.seen.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.remove(&post);
        }
    }
}

impl Seen {
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            pending: Mutex::new(Default::default()),
        }
    }

    /// Hook `hook`'s post `id` at `now`, pending while it's handed to the
    /// agent; none when it was taken inside its keep, or is being handed
    /// now. An empty id is refused: every post has one.
    pub fn begin(
        seen: &Arc<Self>,
        hook: &str,
        id: &str,
        now: u64,
    ) -> anyhow::Result<Option<Pending>> {
        anyhow::ensure!(!id.is_empty(), "a post with no id");
        let post = (hook.to_string(), id.to_string());
        let mut pending = seen.pending.lock().unwrap_or_else(|e| e.into_inner());
        let taken = seen
            .read()?
            .taken
            .iter()
            .any(|t| t.until > now && t.hook == hook && t.id == id);
        if taken || !pending.insert(post.clone()) {
            return Ok(None);
        }
        Ok(Some(Pending {
            seen: seen.clone(),
            post: Some(post),
        }))
    }

    fn read(&self) -> anyhow::Result<SeenList> {
        use std::io::Read as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut file = match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SeenList::default());
            }
            Err(err) => {
                return Err(err).within(format!("reading {}", self.path.display()));
            }
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        link_files::saved::parse_json(&bytes).within(format!("reading {}", self.path.display()))
    }
}

pub(crate) mod questions;
pub use questions::PER_DAY as QUESTIONS_PER_DAY;

#[cfg(test)]
mod tests {
    use super::*;

    fn seen_in(dir: &std::path::Path) -> (Arc<Seen>, PathBuf) {
        let path = dir.join(SEEN_FILE);
        (Arc::new(Seen::at(path.clone())), path)
    }

    /// A post's id is refused while it's handed to the agent, and kept
    /// for its keep once it reached it, per hook; one that didn't reach it
    /// is let go, so a redelivery is taken; an empty one is refused; one
    /// past its keep is dropped as the file is written.
    #[test]
    fn a_repeat_is_refused_inside_its_keep() {
        let dir = link_test_kit::TempDir::new("pushed-seen");
        let (seen, path) = seen_in(&dir);
        let begin = |hook: &str, id: &str, at: u64| Seen::begin(&seen, hook, id, at).unwrap();
        let first = begin("h1", "d-1", 1_000).expect("new");
        assert!(begin("h1", "d-1", 1_000).is_none(), "while it's handed on");
        first.settle(1_000).unwrap();
        assert!(begin("h1", "d-1", 1_000 + LONGEST_KEEP - 1).is_none());
        assert!(begin("h2", "d-1", 1_000).is_some(), "another hook's");
        assert!(Seen::begin(&seen, "h1", "", 1_000).is_err());
        // Not delivered, so let go: its redelivery is taken.
        drop(begin("h3", "d-9", 1_000).expect("new"));
        assert!(begin("h3", "d-9", 1_000).is_some());
        // Past its keep: taken again, and the old entries gone.
        begin("h1", "d-1", 1_000 + LONGEST_KEEP)
            .expect("past its keep")
            .settle(1_000 + LONGEST_KEEP)
            .unwrap();
        let list: SeenList = link_files::saved::parse_json(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(list.taken.len(), 1);
        assert_eq!(list.taken[0].until, 1_000 + 2 * LONGEST_KEEP);
        // Readable only by you.
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&path).unwrap().permissions(),
        );
        assert_eq!(mode & 0o777, 0o600);
    }

    /// A link put where the file is is never followed: not read through,
    /// and not written through either.
    #[test]
    fn the_file_is_never_followed() {
        let dir = link_test_kit::TempDir::new("pushed-seen-link");
        let (seen, path) = seen_in(&dir);
        let elsewhere = dir.join("elsewhere");
        std::fs::write(&elsewhere, r#"{"taken":[]}"#).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
        assert!(Seen::begin(&seen, "h1", "d-1", 1_000).is_err());
        assert_eq!(
            std::fs::read_to_string(&elsewhere).unwrap(),
            r#"{"taken":[]}"#
        );
    }

    /// A `sift` that keeps what it was given (its environment) and answers
    /// the verdict in `verdict.json` beside it.
    const SIFT: &str = "#!/bin/sh\ndir=$(dirname \"$0\")\nenv > \"$dir/env\"\ncat > /dev/null\ncat \"$dir/verdict.json\"\necho\n";

    /// Through the listener, with a `sift` of its own and an agent that
    /// fails while `down` says: `sift` is handed no secret of the bridge's;
    /// a post with no id is refused as the module's fault; a post that
    /// didn't reach the agent is answered 503 and its redelivery taken; one
    /// that did is answered 202 and its repeat refused.
    #[test]
    fn a_post_is_seen_once_it_reached_the_agent() {
        let dir = link_test_kit::TempDir::new("pushed-delivered");
        let program = dir.join("lnk-hooky");
        link_test_kit::write_program(&program, SIFT).unwrap();
        let verdict = |dedupe: &str| {
            std::fs::write(
                dir.join("verdict.json"),
                format!(r#"{{"take":{{"thread":"t","chat":"c","dedupe":"{dedupe}"}}}}"#),
            )
            .unwrap();
        };
        // A secret this process holds, as the bridge holds its own: `ours`
        // would hand it on, and `sift` must never be.
        // SAFETY: a name of this test's own, read by no other.
        unsafe { std::env::set_var("LNK_PUSHED_TEST_TOKEN", "held") };
        let down = Arc::new(AtomicBool::new(true));
        let delivered = Arc::new(Mutex::new(Vec::<String>::new()));
        let deliver: Deliver = {
            let (down, delivered) = (down.clone(), delivered.clone());
            Arc::new(move |firing| {
                let (down, delivered) = (down.clone(), delivered.clone());
                Box::pin(async move {
                    anyhow::ensure!(!down.load(Ordering::Relaxed), "the harness is down");
                    delivered.lock().unwrap().push(firing.text);
                    Ok(())
                })
            })
        };
        let found = program.clone();
        let pushed = PushedIn::with(
            deliver,
            Arc::new(|id: &str| {
                (id == HOOK).then(|| Hook {
                    connection: "hooky".into(),
                    settings: serde_json::Value::Null,
                    secret: link_secretive::Secret::new("s".into()),
                    previous: None,
                    from: "webhook:deploys".into(),
                    thread: "webhook:deploys".into(),
                    questions_per_day: QUESTIONS_PER_DAY,
                })
            }),
            Arc::new(move |_: &str| Some(found.clone())),
            Seen::at(dir.join(SEEN_FILE)),
        );
        let (listener, port) = loopback();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = pushed.serve(listener, stop.clone()).unwrap();
        link_fetch::install_crypto_provider();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let hook = format!("http://127.0.0.1:{port}/h/{HOOK}");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let post = |text: &str| {
            runtime.block_on(async {
                let sent = client.post(&hook).body(text.to_string()).send();
                sent.await.unwrap().status().as_u16()
            })
        };

        // No id: refused, and never handed on.
        verdict("");
        assert_eq!(post("no id"), 503);
        // Handed nothing: neither the secret this process holds nor the
        // pipe `ours` hands it on.
        let env = std::fs::read_to_string(dir.join("env")).unwrap();
        assert!(!env.contains("LNK_PUSHED_TEST_TOKEN"), "{env}");
        assert!(!env.contains(link_secretive::process::HANDED_ENV), "{env}");
        // The agent is down: 503, and kept nowhere.
        verdict("d-1");
        assert_eq!(post("first"), 503);
        assert!(delivered.lock().unwrap().is_empty());
        // Up again: its redelivery is taken, and its repeat refused.
        down.store(false, Ordering::Relaxed);
        assert_eq!(post("first"), 202);
        assert_eq!(post("first"), 401);
        assert_eq!(*delivered.lock().unwrap(), ["first"]);
        stop.store(true, Ordering::Relaxed);
        thread.join().unwrap();
    }

    #[test]
    fn a_hooks_id_is_hex_of_128_bits_at_least() {
        assert!(is_hook_id(&"a".repeat(32)));
        assert!(!is_hook_id(&"a".repeat(31)));
        assert!(!is_hook_id(&format!("{}g", "a".repeat(32))));
        assert!(!is_hook_id("../../etc"));
    }

    /// A listener on 127.0.0.1, on a port of its own, and the port: held
    /// from the start, so no other test takes it between.
    fn loopback() -> (std::net::TcpListener, u16) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    fn nowhere() -> Deliver {
        Arc::new(|_| Box::pin(async { Ok(()) }))
    }

    fn nobody() -> PushedIn {
        PushedIn::with(
            nowhere(),
            Arc::new(|_: &str| None),
            Arc::new(|_: &str| None),
            Seen::at(PathBuf::from("/nonexistent/seen.json")),
        )
    }

    /// One with a hook at [`HOOK`], whose module isn't here.
    const HOOK: &str = "0123456789abcdef0123456789abcdef";

    fn one_hook() -> PushedIn {
        PushedIn::with(
            nowhere(),
            Arc::new(|id: &str| {
                (id == HOOK).then(|| Hook {
                    connection: "nowhere".into(),
                    settings: serde_json::Value::Null,
                    secret: link_secretive::Secret::new("s".into()),
                    previous: None,
                    from: "webhook:deploys".into(),
                    thread: "webhook:deploys".into(),
                    questions_per_day: QUESTIONS_PER_DAY,
                })
            }),
            Arc::new(|_: &str| None),
            Seen::at(PathBuf::from("/nonexistent/seen.json")),
        )
    }

    /// It listens on 127.0.0.1 alone: any other address is refused before
    /// anything is bound.
    #[test]
    fn it_listens_on_127_0_0_1_alone() {
        for at in ["0.0.0.0:0", "[::]:0", "[::1]:0", "192.168.1.2:0"] {
            let at: SocketAddr = at.parse().unwrap();
            let err = nobody()
                .listen(at, Arc::new(AtomicBool::new(true)))
                .err()
                .unwrap_or_else(|| panic!("listened on {at}"));
            assert!(format!("{err:#}").contains("127.0.0.1 alone"), "{err:#}");
        }
        // A listener bound elsewhere already: refused too.
        let anywhere = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let err = nobody()
            .serve(anywhere, Arc::new(AtomicBool::new(true)))
            .expect_err("served on 0.0.0.0");
        assert!(format!("{err:#}").contains("127.0.0.1 alone"), "{err:#}");
        let (listener, port) = loopback();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = nobody().serve(listener, stop.clone()).unwrap();
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
        stop.store(true, Ordering::Relaxed);
        thread.join().unwrap();
    }

    /// What it answers before any module runs: a hook that isn't there,
    /// first, holding no place and reading nothing; a body over the cap;
    /// anything but a post to a hook; and a hook whose module isn't here.
    #[test]
    fn posts_refused_before_any_module() {
        let (listener, port) = loopback();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = one_hook().serve(listener, stop.clone()).unwrap();
        link_fetch::install_crypto_provider();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let at = |path: &str| format!("http://127.0.0.1:{port}{path}");
        let hook = at(&format!("/h/{HOOK}"));
        let unknown = at(&format!("/h/{}", "a".repeat(32)));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let status = |request: reqwest::RequestBuilder| {
            runtime.block_on(async { request.send().await.unwrap().status().as_u16() })
        };
        // A body over the cap, only declared: it's answered before any of
        // it is read, so none is sent (a client still sending it could
        // find the connection closed under it).
        let declared = |path: &str| {
            use std::io::{Read, Write};
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                stream,
                "POST {path} HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-length: {}\r\n\r\n",
                UPDATE_MAX + 1
            )
            .unwrap();
            let mut head = [0u8; 12];
            stream.read_exact(&mut head).unwrap();
            String::from_utf8_lossy(&head[9..12])
                .parse::<u16>()
                .unwrap()
        };
        assert_eq!(declared(&format!("/h/{}", "a".repeat(32))), 404);
        assert_eq!(declared(&format!("/h/{HOOK}")), 413);
        // A body over the cap, declaring no length (chunked): refused once
        // the cap is read, 413 or the connection closed under the rest,
        // never past the cap (where the missing module would answer 503).
        let chunked = || {
            use std::io::{Read, Write};
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let head = format!(
                "POST /h/{HOOK} HTTP/1.1\r\nhost: 127.0.0.1\r\ntransfer-encoding: chunked\r\n\r\n"
            );
            stream.write_all(head.as_bytes()).unwrap();
            let chunk = format!("{:x}\r\n{}\r\n", 64 * 1024, "x".repeat(64 * 1024));
            for _ in 0..=UPDATE_MAX / (64 * 1024) {
                if stream.write_all(chunk.as_bytes()).is_err() {
                    break;
                }
            }
            let _ = stream.write_all(b"0\r\n\r\n");
            let mut head = [0u8; 12];
            match stream.read_exact(&mut head) {
                Ok(()) => String::from_utf8_lossy(&head[9..12]).parse::<u16>().ok(),
                Err(_) => None,
            }
        };
        let answer = chunked();
        assert!(matches!(answer, Some(413) | None), "{answer:?}");
        assert_eq!(status(client.post(&unknown).body("{}")), 404);
        assert_eq!(status(client.post(at("/h/nothing")).body("{}")), 404);
        assert_eq!(
            status(client.post(at("/v1/chat/completions")).body("{}")),
            404
        );
        assert_eq!(status(client.get(&hook)), 405);
        // Its module isn't installed: the sender tries again later.
        assert_eq!(status(client.post(&hook).body("{}")), 503);
        stop.store(true, Ordering::Relaxed);
        thread.join().unwrap();
    }
}

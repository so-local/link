//! The tools route on the bridge: a sandbox's tools and their sessions,
//! and each call, asked about as its tool's rules say, passed on.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, ensure};
use link_sandbox_client::grants::Authority;
use link_sandbox_route::Hangup;
use link_sandbox_route::Outcome;
use link_sandbox_route::Verdict;
use link_sandbox_route::Verdicts;
use link_sandbox_route::http::RequestHead;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) use super::metadata::{application, work_of};
use super::session::{Session, Started};
use super::{MAX_MESSAGE, START_TIMEOUT, Tool, load};

/// The notifications a client sends a server, the only messages without
/// an id passed on.
const NOTIFICATIONS: &[&str] = &[
    "notifications/initialized",
    "notifications/cancelled",
    "notifications/progress",
    "notifications/roots/list_changed",
];

/// How long a request's body may take to arrive.
const BODY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a server may wait for another's start to finish before it
/// is refused: a harness connecting to all its tools at once gets them
/// all, `tool_starts` at a time.
const START_WAIT: Duration = Duration::from_secs(60);

/// A sandbox's tools, on the bridge: the ones it may call, where their
/// definitions are, your desktop session where it asks you, and its
/// sessions.
pub struct Tools {
    pub sandbox: String,
    pub dir: PathBuf,
    pub runtime: PathBuf,
    pub named: Vec<String>,
    pub desktop: Vec<(String, String)>,
    /// Whom its questions are asked for, and how long each waits.
    pub asking: Option<link_sandbox_client::routes::Asking>,
    /// Who makes its calls: the model, unless the owner's own command
    /// line started it ([`super::By`]).
    pub by: super::By,
    sessions: Mutex<Vec<(String, Arc<Session>)>>,
    /// Its servers starting at once: the settings' `tool_starts`, its own,
    /// so one sandbox starting many never keeps another's waiting.
    starting: tokio::sync::Semaphore,
}

impl Tools {
    pub fn new(sandbox: &str, dir: PathBuf, named: Vec<String>) -> Self {
        Self {
            sandbox: sandbox.into(),
            dir,
            runtime: link_files::layout::Root::Runtime.path(),
            named,
            desktop: Vec::new(),
            asking: None,
            by: super::By::Model,
            sessions: Default::default(),
            starting: tokio::sync::Semaphore::new(crate::settings::get().tool_starts),
        }
    }

    fn session(&self, id: &str, tool: &str) -> Option<Arc<Session>> {
        self.sweep();
        let sessions = self.sessions.lock().ok()?;
        let session = sessions
            .iter()
            .find(|(i, s)| i == id && s.tool == tool)
            .map(|(_, s)| s.clone())?;
        *session.used.lock().unwrap_or_else(|e| e.into_inner()) = std::time::Instant::now();
        Some(session)
    }

    /// Ends the sessions with no call for the settings' `tool_idle`: a
    /// sandbox's are its own long after its proxy is gone otherwise, on
    /// a bridge that outlives it. Whether any are left.
    pub fn sweep(&self) -> bool {
        let idle = crate::settings::get().tool_idle;
        let Ok(mut sessions) = self.sessions.lock() else {
            return true;
        };
        sessions.retain(|(_, s)| s.used.lock().is_ok_and(|used| used.elapsed() < idle));
        !sessions.is_empty()
    }

    fn keep(&self, session: Arc<Session>) -> anyhow::Result<String> {
        let id = link_files::random_token()?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| anyhow::anyhow!("sessions' lock"))?;
        let kept: Vec<(&str, std::time::Instant)> = sessions
            .iter()
            .map(|(_, s)| {
                (
                    s.tool.as_str(),
                    *s.used.lock().unwrap_or_else(|e| e.into_inner()),
                )
            })
            .collect();
        if let Some(index) = to_end(&kept, &session.tool, crate::settings::get().tool_sessions) {
            sessions.remove(index);
        }
        sessions.push((id.clone(), session));
        Ok(id)
    }

    fn end(&self, id: &str) -> bool {
        let Ok(mut sessions) = self.sessions.lock() else {
            return false;
        };
        let before = sessions.len();
        sessions.retain(|(i, _)| i != id);
        sessions.len() < before
    }

    /// Forgets `session` (its server ended), so its id is answered as gone.
    fn end_tool_session(&self, session: &Arc<Session>) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.retain(|(_, s)| !Arc::ptr_eq(s, session));
        }
    }

    /// Ends every session of tool `name` (removed, or no longer named).
    fn end_tool(&self, name: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.retain(|(_, s)| s.tool != name);
        }
    }
}

/// Which of a sandbox's `kept` sessions (each its tool and when it was last
/// called) a new session of `tool` ends: none while `tool` has fewer than
/// `most`, else its own least recently called. The cap is each tool's, so
/// a sandbox starting more tools than `most` at once never ends another
/// tool's session, its own start among them.
fn to_end(kept: &[(&str, std::time::Instant)], tool: &str, most: usize) -> Option<usize> {
    let mine = || kept.iter().enumerate().filter(|(_, (t, _))| *t == tool);
    if mine().count() < most {
        return None;
    }
    mine()
        .min_by_key(|(_, (_, used))| *used)
        .map(|(index, _)| index)
}

/// A request's head, as the verb needs it.
struct Head {
    method: String,
    tool: String,
    length: usize,
    session: Option<String>,
}

fn parse(head: &[u8]) -> Result<Head, String> {
    let request = RequestHead::parse(head)?;
    let tool = request
        .path()
        .strip_prefix(link_sandbox_client::routes::TOOLS)
        .map(|t| t.trim_end_matches('/'))
        .filter(|t| link_sandbox_client::routes::check_tool_name(t).is_ok())
        .ok_or_else(|| format!("{} isn't a tool's address", request.target))?;
    if request.header("upgrade").is_some() {
        return Err("no switching protocols".into());
    }
    let head = Head {
        method: request.method.to_ascii_uppercase(),
        tool: tool.into(),
        length: request.length()? as usize,
        session: request.header("mcp-session-id").map(str::to_string),
    };
    if head.length > MAX_MESSAGE {
        return Err(format!("a body over {} MB", MAX_MESSAGE / 1024 / 1024));
    }
    Ok(head)
}

/// One connection on the verb (the head already read, and `rest` of what
/// came after it): one request, answered, then closed. `call` is logged
/// with the tool, and a tool call's name.
pub async fn serve<S>(
    mut client: S,
    head: &[u8],
    rest: Vec<u8>,
    tools: &Tools,
    authority: &Authority,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Verdicts + Hangup + Unpin,
{
    // Said before its answer, as the log keeps it: the tool, and a tool
    // call's name.
    let mut call = Verdict::default();
    let call = &mut call;
    let head = match parse(head) {
        Ok(head) => head,
        Err(why) => return refuse(&mut client, call, "400 Bad Request", &why).await,
    };
    call.to = head.tool.clone();
    if !authority.tool(&head.tool, &tools.named) {
        return refuse(
            &mut client,
            call,
            "403 Forbidden",
            &format!("{} isn't a tool this sandbox is granted now", head.tool),
        )
        .await;
    }
    let tool = match load(&tools.dir, &head.tool) {
        Ok(tool) => tool,
        Err(err) => {
            tools.end_tool(&head.tool);
            return refuse(&mut client, call, "403 Forbidden", &format!("{err:#}")).await;
        }
    };
    match head.method.as_str() {
        "POST" => {}
        "DELETE" => {
            let ended = head.session.as_deref().is_some_and(|id| tools.end(id));
            let status = if ended { "200 OK" } else { "404 Not Found" };
            return answer(&mut client, call, status, None, None).await;
        }
        // No stream from the server: its requests and notifications stay
        // outside.
        _ => return answer(&mut client, call, "405 Method Not Allowed", None, None).await,
    }
    let body =
        match tokio::time::timeout(BODY_TIMEOUT, read_body(&mut client, rest, head.length)).await {
            Ok(Ok(body)) => body,
            _ => {
                return refuse(
                    &mut client,
                    call,
                    "400 Bad Request",
                    "the request's body didn't arrive whole",
                )
                .await;
            }
        };
    let message: Value = match serde_json::from_slice(&body) {
        Ok(Value::Object(m)) => Value::Object(m),
        Ok(_) => {
            return refuse(
                &mut client,
                call,
                "400 Bad Request",
                "one JSON-RPC message per request, not a batch",
            )
            .await;
        }
        Err(_) => {
            return refuse(
                &mut client,
                call,
                "400 Bad Request",
                "a body that isn't JSON",
            )
            .await;
        }
    };
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let is_request = message.get("id").is_some() && !method.is_empty();

    if method == "initialize" && is_request {
        // A session of its own, and the client's capabilities kept from
        // the server: nothing asks the sandbox for its folders or a model.
        let mut message = message;
        if let Some(params) = message.get_mut("params").and_then(Value::as_object_mut) {
            params.insert("capabilities".into(), json!({}));
        }
        // A client that leaves while its server waits its turn or starts
        // (it gave up) gets none: the server is stopped, and never takes
        // the place of a session in use.
        let started = tokio::select! {
            started = start(tools, &tool, &message) => started,
            () = gone(&mut client) => bail!("the client left before tool {} started", tool.name),
        };
        let (session, result) = match started {
            Ok(started) => started,
            Err((status, why)) => return refuse(&mut client, call, status, &why).await,
        };
        let id = tools.keep(session)?;
        if let Err(err) = answer(&mut client, call, "200 OK", Some(&result), Some(&id)).await {
            // Nobody heard of it.
            tools.end(&id);
            return Err(err);
        }
        return Ok(());
    }
    let Some(id) = head.session.as_deref() else {
        return refuse(
            &mut client,
            call,
            "400 Bad Request",
            "no Mcp-Session-Id: initialize first",
        )
        .await;
    };
    let Some(session) = tools.session(id, &tool.name) else {
        // MCP's word for a session that's gone: the client starts another.
        return answer(&mut client, call, "404 Not Found", None, None).await;
    };
    if session.started != Started::of(&tool) {
        // Changed since it started: gone too, and the next one runs as
        // the tool is now.
        tools.end(id);
        return answer(&mut client, call, "404 Not Found", None, None).await;
    }
    if !is_request {
        // Only MCP's own notifications are passed on: a server that
        // dispatches on the method alone would run a `tools/call` sent
        // without an id, and nobody would be asked.
        if message.get("id").is_some() || !NOTIFICATIONS.contains(&method.as_str()) {
            return refuse(
                &mut client,
                call,
                "400 Bad Request",
                "only a request with an id, or one of MCP's notifications",
            )
            .await;
        }
        session.send(&message).await?;
        return answer(&mut client, call, "202 Accepted", None, None).await;
    }
    // What hands the sandbox something of the server's, each asked as a
    // call: a tool's call, by its name, and reading a resource or getting
    // a prompt, by the method, which `--allow` names alike.
    let asked = match method.as_str() {
        "tools/call" => Some(
            message
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        ),
        "resources/read" | "prompts/get" => Some(method.clone()),
        "ping" | "tools/list" | "resources/list" | "resources/templates/list" | "prompts/list" => {
            None
        }
        _ => {
            return refuse(
                &mut client,
                call,
                "403 Forbidden",
                "unsupported MCP request method",
            )
            .await;
        }
    };
    let mut approval = None;
    if let Some(name) = &asked {
        call.to = format!(
            "{}/{}",
            tool.name,
            name.chars().take(128).collect::<String>()
        );
        match gate(tools, &tool, &session, name, &message).await {
            Ok(reviewed) => approval = reviewed,
            Err(why) => {
                call.outcome = Outcome::Refused;
                let refused = json!({"jsonrpc": "2.0", "id": message["id"], "error": {
                    "code": -32001,
                    "message": format!("Link's sandbox: {why}")
                }});
                answer(&mut client, call, "200 OK", Some(&refused), None).await?;
                bail!("refused: {why}");
            }
        }
    }
    let source_store = crate::bridges::asks::store(&tools.dir)?;
    let fence = if approval.is_some() || message["params"]["_meta"]["link.local/work"].is_string() {
        Some(tokio::task::spawn_blocking(move || source_store.dispatch_fence()).await??)
    } else {
        None
    };
    let policy_dir = tools.dir.clone();
    let policy = tokio::task::spawn_blocking(move || super::policy_fence(&policy_dir)).await??;
    let current = load(&tools.dir, &tool.name)?;
    ensure!(
        session.started == Started::of(&current)
            && (approval.is_some()
                || asked.as_deref().is_none_or(|name| !current.asks_given_by(
                    name,
                    &message["params"]["arguments"],
                    tools.by
                ))),
        "tool or approval policy changed before dispatch; review the current operation again"
    );
    let application = application(tools, &tool.name, &message, approval)?;
    let observed = application.is_some();
    let mut fences: Vec<_> = fence.into_iter().collect();
    fences.push(policy);
    // Until it's answered, or its client leaves: a call that never
    // answers holds the connection only as long as the client waits.
    let result = tokio::select! {
        result = session.observed_request(&message, fences, application) => result,
        () = gone(&mut client) => {
            let _ = session.send(&json!({"jsonrpc":"2.0","method":"notifications/cancelled",
                "params":{"requestId":message["id"],"reason":"client disconnected"}})).await;
            if !observed {session.forget(&message["id"]);}
            bail!("the client left before tool {} answered", tool.name);
        }
    };
    let result = match result {
        Ok(result) => result,
        Err(err) => {
            // The server ended: the client hears why, and starts again.
            tools.end_tool_session(&session);
            let failed = json!({"jsonrpc": "2.0", "id": message["id"], "error": {
                "code": -32603,
                "message": format!("Link's sandbox: {err:#}")
            }});
            answer(&mut client, call, "200 OK", Some(&failed), None).await?;
            return Err(err);
        }
    };
    if method == "tools/list" {
        // A first page replaces what the server said before; a next page
        // (its cursor) adds to it.
        session.keep_hints(&result, message["params"]["cursor"].is_null());
    }
    answer(&mut client, call, "200 OK", Some(&result), None).await
}

/// A session of `tool`, one of `tools`, started for their sandbox, and
/// its answer to `initialize` (`message`); or the status and why not.
pub(super) async fn start(
    tools: &Tools,
    tool: &Tool,
    message: &Value,
) -> Result<(Arc<Session>, Value), (&'static str, String)> {
    // At most two of a sandbox's servers start at once, each within its
    // time: a sandbox can't have hundreds start, outside, with your
    // logins. The rest of its own wait their turn; another sandbox's don't.
    let Ok(Ok(_starting)) = tokio::time::timeout(START_WAIT, tools.starting.acquire()).await else {
        return Err((
            "429 Too Many Requests",
            "tools are starting already: try again in a moment".into(),
        ));
    };
    let session = match tool.shared {
        true => Session::connect(&tools.runtime, &tools.dir, tool).await,
        false => Session::start(tool, Some(&tools.sandbox)),
    };
    let session = session.map_err(|err| ("502 Bad Gateway", format!("{err:#}")))?;
    match tokio::time::timeout(START_TIMEOUT, session.request(message)).await {
        Ok(Ok(result)) => Ok((session, result)),
        _ => Err((
            "502 Bad Gateway",
            format!("tool {} didn't start", tool.name),
        )),
    }
}

/// Returns once `client` has left, or failed: it gave up on its request.
/// It sends nothing more after the one request it makes, so whatever it
/// does send is dropped. Its end of what it sends is leaving only where
/// that can't be told from shutting its writing (a half-close, after
/// which it still reads the answer); where it can, the socket is then
/// checked, unread, until it's hung up or failed.
async fn gone<S: AsyncRead + Hangup + Unpin>(client: &mut S) {
    let mut buf = [0u8; 512];
    loop {
        match client.read(&mut buf).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => return,
        }
    }
    let Some(socket) = client.hangup_socket() else {
        return;
    };
    link_sandbox_route::io::until_hung_up(socket).await
}

/// Whether call `name` of `tool` goes through: allowed by its rules, or
/// by you when it asks.
pub(super) use super::gating::gate;
pub use super::gating::question;

/// `length` bytes of body: `rest` and what follows it.
async fn read_body<S: AsyncRead + Unpin>(
    client: &mut S,
    mut body: Vec<u8>,
    length: usize,
) -> anyhow::Result<Vec<u8>> {
    ensure!(
        body.len() <= length,
        "more than one request on the connection"
    );
    let mut chunk = vec![0u8; 64 * 1024];
    while body.len() < length {
        let want = (length - body.len()).min(chunk.len());
        let n = client.read(&mut chunk[..want]).await?;
        ensure!(n > 0, "the connection closed mid-body");
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(body)
}

/// Answers the call: its verdict `call`, then its answer.
async fn answer<S: AsyncWrite + Verdicts + Unpin>(
    client: &mut S,
    call: &Verdict,
    status: &str,
    body: Option<&Value>,
    session: Option<&str>,
) -> anyhow::Result<()> {
    client.say(call).await?;
    let body = body
        .map(serde_json::to_vec)
        .transpose()?
        .unwrap_or_default();
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    if let Some(id) = session {
        head.push_str(&format!("Mcp-Session-Id: {id}\r\n"));
    }
    head.push_str("\r\n");
    client.write_all(head.as_bytes()).await?;
    client.write_all(&body).await?;
    client.shutdown().await?;
    Ok(())
}

/// Refuses the call, `why`: its verdict `call`, refused, then `status`.
/// One connection refused because what its sandbox may reach couldn't
/// be learned (`why`, the keeper's own words): the tool named, the reason
/// given, and the steps that help, so the agent's log says what happened
/// rather than that the tool isn't granted.
pub async fn unanswered<S>(mut client: S, head: &[u8], why: &str) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Verdicts + Unpin,
{
    let mut call = Verdict::default();
    let tool = parse(head).map(|head| head.tool).unwrap_or_default();
    call.to = tool.clone();
    let said = format!(
        "{tool} wasn't checked: Link couldn't learn what this sandbox may reach, so it reaches \
         nothing. {why}. A module from another build is brought to this one by lnk upgrade; a \
         keeper that didn't answer by lnk agent restart."
    );
    refuse(&mut client, &mut call, "403 Forbidden", &said).await
}

async fn refuse<S: AsyncWrite + Verdicts + Unpin>(
    client: &mut S,
    call: &mut Verdict,
    status: &str,
    why: &str,
) -> anyhow::Result<()> {
    call.outcome = Outcome::Refused;
    client.say(call).await?;
    let body = format!("Link's sandbox: {why}\n");
    let _ = client
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await;
    bail!("refused: {why}")
}

#[cfg(test)]
mod tests;

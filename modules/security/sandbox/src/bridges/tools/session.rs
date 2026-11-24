//! A tool's server, running outside for one session: its stdin, and
//! the requests waiting on its answers.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{bail, ensure};
use link_format::WithinExt;
use link_secretive::Duplicate;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::oneshot;

use super::{MAX_MESSAGE, START_TIMEOUT, Tool, server};
#[cfg(test)]
mod tests;

/// A server running outside, for one session: its stdin, and the
/// requests waiting on its answers.
pub struct Session {
    pub(super) tool: String,
    /// What it was started as: a session whose tool has changed since
    /// (a new command, folder or `--env`, such as a token replaced) is
    /// ended, so the old process and its environment don't stay in use.
    pub(super) started: Started,
    /// Its server's input: the process's own, or its hub's connection for
    /// a shared tool.
    stdin: tokio::sync::Mutex<Box<dyn AsyncWrite + Send + Unpin>>,
    pub(super) pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    /// What the server says of each of its tools (from `tools/list`),
    /// shown when a call asks.
    hints: Mutex<HashMap<String, Value>>,
    /// When it was last called.
    pub(super) used: Mutex<std::time::Instant>,
    /// Its server's process, when it runs its own: the leader of a group
    /// of its own, ended whole with the session.
    child: Mutex<Option<tokio::process::Child>>,
}

/// An ended session's server stops whole: its input closed, then it and
/// what it started, a language server or a browser, given the sandbox's
/// `tool_stop` and ended ([`crate::bridges::stop`]), apart from the
/// session's caller.
impl Drop for Session {
    fn drop(&mut self) {
        let Some(child) = self.child.get_mut().ok().and_then(Option::take) else {
            return;
        };
        *self.stdin.get_mut() = Box::new(tokio::io::sink());
        let Some(pid) = child.id() else {
            return;
        };
        let grace = crate::settings::get().tool_stop;
        std::thread::spawn(move || {
            crate::bridges::stop::server(pid, grace);
            // Reaped once ended, with Tokio's other orphans.
            drop(child);
        });
    }
}

/// A tool's command, folder and environment: what its process runs as.
#[derive(PartialEq)]
pub(super) struct Started {
    argv: Vec<String>,
    env: BTreeMap<String, link_secretive::Secret>,
    shared: bool,
}

impl Started {
    pub(super) fn of(tool: &Tool) -> Self {
        Self {
            argv: tool.argv.clone(),
            env: tool.env.duplicate(),
            shared: tool.shared,
        }
    }
}

impl Session {
    /// `tool`'s server, started.
    pub fn start(tool: &Tool) -> anyhow::Result<Arc<Self>> {
        let mut command = server(tool);
        // In a group of its own, so it ends whole; and on Linux, with this
        // proxy, as it did in the proxy's group.
        link_secretive::process::ends_with_parent(&mut command, libc::SIGKILL);
        let mut child = tokio::process::Command::from(command)
            .process_group(0)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .within(format!(
                "starting tool {}: {}",
                tool.name,
                tool.argv.join(" ")
            ))?;
        let stdin = child.stdin.take().within("the tool's stdin")?;
        let stdout = child.stdout.take().within("the tool's stdout")?;
        let session = Arc::new(Self {
            tool: tool.name.clone(),
            started: Started::of(tool),
            stdin: tokio::sync::Mutex::new(Box::new(stdin)),
            pending: Default::default(),
            hints: Default::default(),
            used: Mutex::new(std::time::Instant::now()),
            child: Mutex::new(Some(child)),
        });
        tokio::spawn(read_server(Arc::downgrade(&session), stdout));
        Ok(session)
    }

    /// A session of shared `tool`, its server the hub's (started first if
    /// it isn't running): a connection to it, ended when the session is.
    pub async fn connect(runtime: &Path, dir: &Path, tool: &Tool) -> anyhow::Result<Arc<Self>> {
        let stream = crate::bridges::shared::connect(runtime, dir, tool).await?;
        let (read, write) = stream.into_split();
        let session = Arc::new(Self {
            tool: tool.name.clone(),
            started: Started::of(tool),
            stdin: tokio::sync::Mutex::new(Box::new(write)),
            pending: Default::default(),
            hints: Default::default(),
            used: Mutex::new(std::time::Instant::now()),
            child: Mutex::new(None),
        });
        tokio::spawn(read_server(Arc::downgrade(&session), read));
        Ok(session)
    }

    /// Sends `message` (a request: it has an `id`) and waits for its
    /// answer; an error if the server ends first.
    pub async fn request(&self, message: &Value) -> anyhow::Result<Value> {
        self.request_fenced(message, Vec::new()).await
    }

    pub(super) async fn request_fenced(
        &self,
        message: &Value,
        fence: Vec<std::fs::File>,
    ) -> anyhow::Result<Value> {
        self.begin_request(message, fence)
            .await?
            .await
            .map_err(|_| anyhow::anyhow!("tool {} ended before answering", self.tool))
    }

    async fn begin_request(
        &self,
        message: &Value,
        fence: Vec<std::fs::File>,
    ) -> anyhow::Result<oneshot::Receiver<Value>> {
        let id = message.get("id").within("a request without an id")?;
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| anyhow::anyhow!("a session's lock"))?
            .insert(id.to_string(), tx);
        self.send(message).await?;
        drop(fence);
        Ok(rx)
    }

    /// Source observation outlives a caller that disconnects after handoff. No call is replayed.
    pub(super) async fn observed_request(
        self: &Arc<Self>,
        message: &Value,
        fence: Vec<std::fs::File>,
        application: Option<link_interactions::sources::Application>,
    ) -> anyhow::Result<Value> {
        if application.is_none() {
            return self.request_fenced(message, fence).await;
        }
        let session = self.clone();
        let message = message.clone();
        let (sent, received) = oneshot::channel();
        tokio::spawn(async move {
            use link_interactions::approval::EffectOutcome;
            let result = session.request_fenced(&message, fence).await;
            if let Some(application) = application {
                let outcome = match &result {
                    Ok(answer)
                        if answer.get("error").is_none() && answer["result"]["isError"] != true =>
                    {
                        EffectOutcome::Applied {
                            detail: "Source confirmed completion of the exact admitted call".into(),
                        }
                    }
                    Ok(answer) => EffectOutcome::Failed {
                        reason: answer.to_string(),
                    },
                    Err(err) => EffectOutcome::Unknown {
                        reason: format!("Source outcome unavailable: {err:#}"),
                    },
                };
                if let Err(err) = application.report(outcome) {
                    let _ = sent.send(Err(err));
                    return;
                }
            }
            let _ = sent.send(result);
        });
        received
            .await
            .map_err(|_| anyhow::anyhow!("source observer ended"))?
    }

    pub(super) fn forget(&self, id: &Value) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id.to_string());
        }
    }

    /// Sends `message` without waiting (a notification).
    pub async fn send(&self, message: &Value) -> anyhow::Result<()> {
        let mut line = serde_json::to_vec(message)?;
        line.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(&line).await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Keeps what `result` (a `tools/list` answer) says of each tool.
    pub(super) fn keep_hints(&self, result: &Value) {
        let Some(tools) = result.pointer("/result/tools").and_then(Value::as_array) else {
            return;
        };
        if let Ok(mut hints) = self.hints.lock() {
            for tool in tools {
                if let Some(name) = tool.get("name").and_then(Value::as_str) {
                    hints.insert(
                        name.into(),
                        tool.get("annotations").cloned().unwrap_or_default(),
                    );
                }
            }
        }
    }

    pub(super) fn hints_of(&self, call: &str) -> Value {
        self.hints
            .lock()
            .ok()
            .and_then(|h| h.get(call).cloned())
            .unwrap_or_default()
    }
}

/// The server's messages: answers to the requests waiting on them; its
/// own requests of the client answered here (a ping), or refused, since
/// the client is in the sandbox (its folders, a model to sample, you);
/// its notifications dropped.
async fn read_server(session: std::sync::Weak<Session>, stdout: impl AsyncRead + Unpin) {
    let mut reader = tokio::io::BufReader::new(stdout);
    loop {
        let line = match read_line(&mut reader).await {
            Ok(Some(line)) => line,
            _ => break,
        };
        let Some(session) = session.upgrade() else {
            break;
        };
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let (id, method) = (message.get("id"), message.get("method"));
        match (id, method) {
            (Some(id), Some(method)) => {
                let answer = match method.as_str() {
                    Some("ping") => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                    _ => json!({"jsonrpc": "2.0", "id": id, "error": {
                        "code": -32601,
                        "message": "Link's sandbox doesn't pass a tool's requests to the client"
                    }}),
                };
                let _ = session.send(&answer).await;
            }
            (Some(id), None) => {
                let waiting = session
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut p| p.remove(&id.to_string()));
                if let Some(tx) = waiting {
                    let _ = tx.send(message);
                }
            }
            _ => {}
        }
    }
    // Ended: whatever waits on it is answered with an error.
    if let Some(session) = session.upgrade()
        && let Ok(mut pending) = session.pending.lock()
    {
        pending.clear();
    }
}

/// One line of at most [`MAX_MESSAGE`], without its end; None at the end.
async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
) -> anyhow::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let n = (&mut *reader)
        .take(MAX_MESSAGE as u64 + 1)
        .read_until(b'\n', &mut line)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    ensure!(
        line.len() <= MAX_MESSAGE,
        "a message over {} MB",
        MAX_MESSAGE / 1024 / 1024
    );
    while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        line.pop();
    }
    Ok(Some(line))
}

/// `lnk sandbox tool-call list`: tool `tool`'s calls, from its server, started
/// here: each one's name, what the server says of it, and a line of its
/// description.
pub async fn calls(tool: &Tool) -> anyhow::Result<Vec<(String, Value, String)>> {
    let session = Session::start(tool)?;
    let init = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "lnk", "version": env!("CARGO_PKG_VERSION")}
    }});
    let answered = tokio::time::timeout(START_TIMEOUT, session.request(&init))
        .await
        .within(format!("tool {} didn't answer in time", tool.name))??;
    if let Some(error) = answered.get("error") {
        bail!("tool {} refused to start: {error}", tool.name);
    }
    session
        .send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await?;
    let mut calls = Vec::new();
    let mut cursor: Option<Value> = None;
    for page in 1..=100 {
        let mut params = json!({});
        if let Some(cursor) = &cursor {
            params["cursor"] = cursor.clone();
        }
        let list = json!({"jsonrpc": "2.0", "id": page, "method": "tools/list", "params": params});
        let answered = tokio::time::timeout(START_TIMEOUT, session.request(&list)).await??;
        if let Some(error) = answered.get("error") {
            bail!("tool {} wouldn't list its calls: {error}", tool.name);
        }
        for item in answered
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
            let about = item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default();
            calls.push((
                link_format::clean(name),
                item.get("annotations").cloned().unwrap_or_default(),
                link_format::clean(about.lines().next().unwrap_or_default()),
            ));
        }
        cursor = answered
            .pointer("/result/nextCursor")
            .filter(|c| !c.is_null())
            .cloned();
        if cursor.is_none() {
            break;
        }
    }
    Ok(calls)
}

//! A session with one MCP server over its HTTP transport ([`Session`]).

use super::*;
mod response;
pub(super) use response::Refused;

/// The longest name a tool may have, for any provider.
pub(super) const NAME_MAX: usize = 64;

/// `name` as a tool's name for any provider: letters, digits, `_` and
/// `-` only, at most [`NAME_MAX`].
pub(super) fn plain_name(name: &str) -> String {
    name.chars()
        .map(
            |c| match c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                true => c,
                false => '_',
            },
        )
        .take(NAME_MAX)
        .collect()
}

/// A session with one server, kept between turns.
pub struct Session {
    client: reqwest::Client,
    server: Server,
    /// The server's id for the session (`Mcp-Session-Id`).
    id: std::sync::Mutex<Option<String>>,
    next: AtomicU64,
    /// How long it has to start again, when the server ends it.
    timeout: std::time::Duration,
    /// What the server said of itself as the session started.
    said: std::sync::Mutex<String>,
}

/// The server said the session is gone (404): start another.
#[derive(Debug)]
struct Gone;

impl std::fmt::Display for Gone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the tool's session ended")
    }
}

impl std::error::Error for Gone {}

/// A transient bridge refusal, which did not start a tool operation.
#[derive(Debug)]
pub(super) struct Busy(String);

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Busy {}

pub(super) fn is_busy(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<Busy>())
}

/// The sandbox refused the tool (403): its grant or this machine's tools
/// don't name it, or don't yet, as while its agent starts.
#[derive(Debug)]
pub(super) struct NotGranted(String);

impl std::fmt::Display for NotGranted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotGranted {}

pub(super) fn is_not_granted(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<NotGranted>())
}

/// Whether a start's failure says nothing of the tool itself: it may
/// start once asked again ([`Busy`], [`NotGranted`]).
pub(super) fn is_passing(err: &anyhow::Error) -> bool {
    is_busy(err) || is_not_granted(err)
}

/// Whether a start waits for its tool's grant: the first time a process
/// asks for the tool, which its sandbox may not grant until its start is
/// done; later, a refusal is said at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Grant {
    Awaited,
    Refused,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    busy_retry_ms: u64,
    grant_retry_ms: u64,
    limit_ms: u64,
}

static START: std::sync::LazyLock<Start> = std::sync::LazyLock::new(|| {
    let start: Start = toml::from_str(include_str!("start.toml")).expect("tool start settings");
    assert!(start.busy_retry_ms > 0 && start.grant_retry_ms > 0);
    assert!(std::time::Duration::from_millis(start.limit_ms) >= super::OPEN_TIMEOUT);
    start
});

/// How long a tool's start may run in all, past the turn that began it
/// (`start.toml`'s `limit_ms`): never less than a turn waits for it.
pub fn start_limit() -> std::time::Duration {
    std::time::Duration::from_millis(START.limit_ms)
}

async fn starting<T, F>(
    timeout: std::time::Duration,
    grant: Grant,
    mut attempt: impl FnMut(std::time::Duration) -> F,
) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let retry = match attempt(left).await {
            Err(err) if is_busy(&err) => (err, START.busy_retry_ms),
            Err(err) if grant == Grant::Awaited && is_not_granted(&err) => {
                (err, START.grant_retry_ms)
            }
            result => return result,
        };
        let next = tokio::time::Instant::now() + std::time::Duration::from_millis(retry.1);
        if next >= deadline {
            return Err(retry.0);
        }
        tokio::time::sleep_until(next).await;
    }
}

/// The call took longer than its spec lets it, and was cancelled. The
/// session works.
#[derive(Debug)]
pub(super) struct TooLong(pub(super) std::time::Duration);

impl std::fmt::Display for TooLong {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "It took longer than {:?}, and was cancelled.", self.0)
    }
}

impl std::error::Error for TooLong {}

/// The call was cancelled: its turn was asked to end. The session works.
#[derive(Debug)]
pub(super) struct Interrupted;

impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("It was interrupted: its turn ended first.")
    }
}

impl std::error::Error for Interrupted {}

impl Session {
    /// Starts a session with `server`, and lists its tools, within
    /// `timeout` in all. Given up on, its connection is closed, so the
    /// server can end the start; a session started whose listing is
    /// given up on, or fails, is ended (MCP's DELETE). With `grant`
    /// awaited, a refusal of its sandbox's is asked again until then.
    pub(super) async fn open(
        client: &reqwest::Client,
        server: &Server,
        timeout: std::time::Duration,
        grant: Grant,
    ) -> anyhow::Result<(Self, Offered)> {
        starting(timeout, grant, |left| Self::open_once(client, server, left)).await
    }

    async fn open_once(
        client: &reqwest::Client,
        server: &Server,
        timeout: std::time::Duration,
    ) -> anyhow::Result<(Self, Offered)> {
        let deadline = tokio::time::Instant::now() + timeout;
        let session = tokio::time::timeout_at(deadline, Self::start_new(client, server, timeout))
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("it took too long to start")))?;
        let listed = tokio::time::timeout_at(deadline, session.list())
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("it took too long to list its tools")));
        match listed {
            Ok(offered) => Ok((session, offered)),
            Err(err) => {
                session.end().await;
                Err(err)
            }
        }
    }

    /// Starts a session with `server`.
    pub(super) async fn start_new(
        client: &reqwest::Client,
        server: &Server,
        timeout: std::time::Duration,
    ) -> anyhow::Result<Self> {
        starting(timeout, Grant::Refused, |left| {
            Self::start_once(client, server, left)
        })
        .await
    }

    async fn start_once(
        client: &reqwest::Client,
        server: &Server,
        timeout: std::time::Duration,
    ) -> anyhow::Result<Self> {
        let session = Self {
            client: client.clone(),
            server: server.clone(),
            id: std::sync::Mutex::new(None),
            next: AtomicU64::new(1),
            timeout,
            said: std::sync::Mutex::default(),
        };
        session.start().await?;
        Ok(session)
    }

    /// The server's tools: the model's name for each, and its own.
    pub(super) async fn list(&self) -> anyhow::Result<Offered> {
        let (session, server) = (self, &self.server);
        let mut tools: Vec<(Tool, String)> = Vec::new();
        let mut summaries = HashMap::new();
        let mut reads = std::collections::HashSet::new();
        let mut cursor: Option<String> = None;
        // A cursor seen before, or too many pages, ends the list.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..PAGES_MAX {
            let params = match &cursor {
                Some(cursor) => json!({"cursor": cursor}),
                None => json!({}),
            };
            let page = session.send("tools/list", params).await?;
            for tool in page["tools"].as_array().into_iter().flatten() {
                if tools.len() >= TOOLS_MAX {
                    break;
                }
                let Some(own) = tool["name"].as_str() else {
                    continue;
                };
                let full = format!("{}{SEPARATOR}{own}", server.name);
                let name = plain_name(&full);
                if full.chars().count() > NAME_MAX {
                    crate::warn!("tool {name} left out: its name is over {NAME_MAX} characters");
                    continue;
                }
                let schema = match &tool["inputSchema"] {
                    Value::Object(_) => tool["inputSchema"].clone(),
                    _ => json!({"type": "object"}),
                };
                let size = schema.to_string().len();
                if size > SCHEMA_MAX {
                    crate::warn!(
                        "tool {name} left out: its schema is {size} bytes, over {SCHEMA_MAX}"
                    );
                    continue;
                }
                let description = tool["description"].as_str().unwrap_or_default();
                if let Some(arguments) = summary(tool) {
                    summaries.insert(name.clone(), arguments);
                }
                if tool["annotations"]["readOnlyHint"] == true {
                    reads.insert(name.clone());
                }
                tools.push((
                    Tool {
                        name,
                        description: cut(description.to_string(), DESCRIPTION_MAX),
                        schema,
                    },
                    own.to_string(),
                ));
            }
            match page["nextCursor"].as_str() {
                Some(next) if tools.len() < TOOLS_MAX && seen.insert(next.to_string()) => {
                    cursor = Some(next.to_string());
                }
                _ => break,
            }
        }
        tools.truncate(TOOLS_MAX);
        // By name, whatever order the server lists them in, so the
        // prompt, and the provider's cache, stay the same.
        tools.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        let instructions = self.said.lock().unwrap_or_else(|e| e.into_inner()).clone();
        summaries.retain(|name, _| tools.iter().any(|(tool, _)| tool.name == *name));
        reads.retain(|name| tools.iter().any(|(tool, _)| tool.name == *name));
        Ok(Offered {
            tools,
            instructions,
            summaries,
            reads,
        })
    }

    /// `initialize`, then `notifications/initialized`: what the server
    /// says of itself kept for its listing.
    async fn start(&self) -> anyhow::Result<()> {
        *self.id.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let started = self
            .send(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "lnk-harness", "version": env!("CARGO_PKG_VERSION")},
                }),
            )
            .await
            .within(format!("starting tool {}", self.server.name))?;
        let said = started["instructions"].as_str().unwrap_or_default().trim();
        *self.said.lock().unwrap_or_else(|e| e.into_inner()) =
            cut(said.to_string(), INSTRUCTIONS_MAX);
        self.post(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await?;
        Ok(())
    }

    /// Ends the session (MCP's DELETE), not waiting long for it.
    pub(super) async fn end(&self) {
        let session = self.id.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(session) = session {
            let _ = self
                .client
                .delete(&self.server.url)
                .header("mcp-session-id", session)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await;
        }
    }

    /// `method` with `params`: its result, once more in a new session if
    /// the server ended this one; the id it was last sent with, in `sent`.
    pub(super) async fn request(
        &self,
        method: &str,
        params: Value,
        sent: &AtomicU64,
    ) -> anyhow::Result<Value> {
        match self.send_as(method, params.clone(), sent).await {
            Err(err) if err.is::<Gone>() => {
                tokio::time::timeout(self.timeout, self.start())
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("it took too long to start again")))?;
                self.send_as(method, params, sent).await
            }
            answered => answered,
        }
    }

    /// `method` with `params`: its result, or its error as one.
    async fn send(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.send_as(method, params, &AtomicU64::new(0)).await
    }

    /// [`Session::send`], its id put in `sent` before it's posted.
    async fn send_as(
        &self,
        method: &str,
        params: Value,
        sent: &AtomicU64,
    ) -> anyhow::Result<Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        sent.store(id, Ordering::Relaxed);
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let Some(answer) = self.post(&message).await? else {
            bail!("tool {} gave no answer to {method}", self.server.name);
        };
        response::checked(answer, &json!(id))
    }

    /// Posts `message`: the answer to it, if it's a request.
    pub(super) async fn post(&self, message: &Value) -> anyhow::Result<Option<Value>> {
        let session = self.id.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut request = self
            .client
            .post(&self.server.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(serde_json::to_vec(message)?);
        if let Some(session) = &session {
            request = request.header("mcp-session-id", session);
        }
        let mut response = request.send().await.map_err(|err| {
            anyhow::anyhow!("reaching tool {}: {}", self.server.name, err.without_url())
        })?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND && session.is_some() {
            return Err(Gone.into());
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let why: String = link_format::clean(body.trim()).chars().take(300).collect();
            let said = format!("tool {} answered {status} {why}", self.server.name);
            if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                return Err(Busy(said).into());
            }
            // Its sandbox's word, never the tool's: the bridge answers a
            // tool's own refusal inside JSON-RPC.
            if status == reqwest::StatusCode::FORBIDDEN {
                return Err(NotGranted(said).into());
            }
            bail!("tool {} answered {status} {why}", self.server.name);
        }
        if let Some(id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            *self.id.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.to_string());
        }
        let Some(wanted) = message.get("id") else {
            return Ok(None);
        };
        let streamed = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        if !streamed {
            let body = response.bytes().await?;
            let answer: Value = serde_json::from_slice(&body)
                .within(format!("reading tool {}'s answer", self.server.name))?;
            return Ok(Some(answer));
        }
        // Server-sent events: the one answering this request, whatever
        // else comes first.
        let mut lines = SseLines::default();
        while let Some(chunk) = response.chunk().await? {
            for data in lines.push(&chunk) {
                if let Ok(answer) = serde_json::from_str::<Value>(&data)
                    && answer.get("id") == Some(wanted)
                    && (answer.get("result").is_some() || answer.get("error").is_some())
                {
                    return Ok(Some(answer));
                }
            }
        }
        bail!("tool {} ended its answer without one", self.server.name)
    }
}

/// The arguments a listed `tool` names to sum up its calls
/// ([`SUMMARY_META`]), at most [`SUMMARY_MAX`], each one its schema
/// declares: none where it names none, or names one it doesn't take.
fn summary(tool: &Value) -> Option<Vec<String>> {
    let named = tool["_meta"][SUMMARY_META].as_array()?;
    let arguments: Vec<String> = named
        .iter()
        .map(|name| name.as_str().map(str::to_string))
        .collect::<Option<_>>()?;
    let declared = |name: &String| tool["inputSchema"]["properties"].get(name).is_some();
    (!arguments.is_empty() && arguments.len() <= SUMMARY_MAX && arguments.iter().all(declared))
        .then_some(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tool's summary is the arguments it named, in order, only where
    /// its schema takes each, at most [`SUMMARY_MAX`].
    #[test]
    fn a_summary_names_only_arguments_the_tool_takes() {
        let tool = |named: Value| {
            json!({"name": "command", "_meta": {SUMMARY_META: named},
                "inputSchema": {"properties": {"program": {}, "args": {}}}})
        };
        assert_eq!(
            summary(&tool(json!(["program", "args"]))),
            Some(vec!["program".to_string(), "args".to_string()])
        );
        assert_eq!(summary(&tool(json!(["program", "secret"]))), None);
        assert_eq!(summary(&tool(json!([]))), None);
        assert_eq!(summary(&tool(json!("program"))), None);
        assert_eq!(
            summary(&tool(json!(["args", "args", "args", "args", "args"]))),
            None
        );
        assert_eq!(summary(&json!({"name": "command"})), None);
    }

    #[test]
    fn names_any_provider_takes() {
        assert_eq!(plain_name("memory__search"), "memory__search");
        assert_eq!(plain_name("gh__repo.view"), "gh__repo_view");
        assert_eq!(plain_name(&"x".repeat(80)).len(), 64);
    }
}

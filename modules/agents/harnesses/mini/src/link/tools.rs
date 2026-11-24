//! A turn's Model Context Protocol (MCP) tools. Each server is initialized
//! and listed at the turn's start. The model sees `<server>__<tool>` names;
//! calls use the server's original name and carry Link's thread metadata.
//! Sessions end after the turn. Unreachable servers are left out.

use crate::core::{Answer, Call};
use anyhow::{Context, bail};
use serde_json::{Value, json};

/// One tool discovered on a server. Its model-facing name includes the
/// server name to distinguish tools with the same name on different servers.
struct Tool {
    name: String,
    server_name: String,
    description: Value,
    schema: Value,
    url: String,
    session: Option<String>,
}

/// Each server at `$LNK_TOOL_<NAME>`, by its name in lower case.
pub(crate) fn from_env() -> Vec<(String, String)> {
    link_secretive::process::variables(|name| name.starts_with("LNK_TOOL_"))
        .filter_map(|(key, url)| {
            let server = key.strip_prefix("LNK_TOOL_")?;
            Some((server.to_ascii_lowercase(), url))
        })
        .collect()
}

/// A turn's tools: each of its servers, a session started and its tools
/// listed at the turn's start, ended at its end. A server that doesn't
/// answer is left out of the turn.
pub(crate) struct Tools {
    tools: Vec<Tool>,
    sessions: Vec<(String, String)>,
}

impl Tools {
    pub(crate) async fn open(client: &reqwest::Client, servers: Vec<(String, String)>) -> Self {
        let mut tools = Vec::new();
        let mut sessions = Vec::new();
        for (server, url) in servers {
            match list(client, &url).await {
                Ok((session, listed)) => {
                    if let Some(id) = &session {
                        sessions.push((url.clone(), id.clone()));
                    }
                    for tool in listed {
                        let server_name = tool["name"].as_str().unwrap_or_default().to_string();
                        tools.push(Tool {
                            name: format!("{server}__{server_name}"),
                            server_name,
                            description: tool["description"].clone(),
                            schema: tool["inputSchema"].clone(),
                            url: url.clone(),
                            session: session.clone(),
                        });
                    }
                }
                Err(_) => continue,
            }
        }
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        Self { tools, sessions }
    }

    pub(crate) fn restrict(&mut self, only: &[String]) {
        self.tools.retain(|tool| only.contains(&tool.name));
    }

    pub(crate) fn names(&self) -> Vec<&str> {
        self.tools.iter().map(|tool| tool.name.as_str()).collect()
    }

    /// What the model is offered.
    pub(crate) fn offered(&self) -> Vec<link_model_contract::chat::Tool> {
        self.tools
            .iter()
            .map(|t| link_model_contract::chat::Tool {
                name: t.name.clone(),
                description: t.description.as_str().unwrap_or_default().into(),
                schema: t.schema.clone(),
            })
            .collect()
    }

    /// `call`'s answer, as text: a tool it doesn't have, or an error, is
    /// an answer too.
    pub(crate) async fn call_tool(
        &self,
        client: &reqwest::Client,
        call: &Call,
        meta: &Value,
    ) -> Answer {
        let name = &call.name;
        let Some(tool) = self.tools.iter().find(|t| t.name == *name) else {
            return Answer {
                call: call.id.clone(),
                text: format!("There is no tool {name}."),
                failed: true,
                parts: Vec::new(),
            };
        };
        let arguments = &call.arguments;
        let params = json!({"name": tool.server_name, "arguments": arguments, "_meta": meta});
        let result = mcp(
            client,
            &tool.url,
            tool.session.as_deref(),
            "tools/call",
            params,
        )
        .await;
        let (text, failed) = match result {
            Ok((result, _)) => (
                result["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                result["isError"].as_bool().unwrap_or(false),
            ),
            Err(err) => (format!("The tool failed: {err:#}"), true),
        };
        Answer {
            call: call.id.clone(),
            text,
            failed,
            parts: Vec::new(),
        }
    }

    /// Ends each session.
    pub(crate) async fn close(self, client: &reqwest::Client) {
        for (url, session) in self.sessions {
            let _ = client
                .delete(url)
                .header("mcp-session-id", session)
                .send()
                .await;
        }
    }
}

/// A session with the server at `url`, and its tools.
async fn list(client: &reqwest::Client, url: &str) -> anyhow::Result<(Option<String>, Vec<Value>)> {
    let start = json!({"protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "lnk-mini-harness", "version": env!("CARGO_PKG_VERSION")}});
    let (_, session) = mcp(client, url, None, "initialize", start).await?;
    let mut initialized = client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string());
    if let Some(session) = &session {
        initialized = initialized.header("mcp-session-id", session);
    }
    initialized.send().await?;
    let (listed, _) = mcp(client, url, session.as_deref(), "tools/list", json!({})).await?;
    let tools = listed["tools"].as_array().cloned().unwrap_or_default();
    Ok((session, tools))
}

/// `method` with `params`, to the MCP server at `url`: its result, and
/// the session it names.
async fn mcp(
    client: &reqwest::Client,
    url: &str,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> anyhow::Result<(Value, Option<String>)> {
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let mut request = client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(message.to_string());
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let response = request
        .send()
        .await
        .map_err(|err| anyhow::anyhow!("reaching it: {}", err.without_url()))?;
    if !response.status().is_success() {
        bail!("{method} was answered {}", response.status());
    }
    let session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or(session.map(str::to_string));
    let streamed = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    let body = response.bytes().await?;
    // Server-sent events: the one that answers.
    let answer = match streamed {
        true => link_connection_contract::SseLines::default()
            .push(&body)
            .iter()
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .find(|event| event.get("id").is_some())
            .context("no answer in its stream")?,
        false => serde_json::from_slice(&body)?,
    };
    if let Some(why) = answer["error"]["message"].as_str() {
        bail!("{}", link_format::clean(why));
    }
    Ok((answer["result"].clone(), session))
}

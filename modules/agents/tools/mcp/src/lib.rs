//! A transport over installed commands, never a second command implementation.
pub mod cli;
pub mod transport;

use anyhow::{Context, ensure};
use link_component::{Resolved, tools::Tool};
use serde_json::{Value, json};
use std::{collections::BTreeMap, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

/// Session limits and instructions, kept beside the transport that uses them.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub calls: usize,
    pub message_max: u64,
    pub stderr_max: u64,
    pub protocol_version: String,
    pub instructions: String,
}

pub static SETTINGS: std::sync::LazyLock<Settings> = std::sync::LazyLock::new(|| {
    toml::from_str(include_str!("../settings.toml")).expect("checked MCP settings")
});

pub struct Catalog {
    tools: BTreeMap<String, Tool>,
    agent: Option<String>,
}

impl Catalog {
    /// Build from installed descriptors, already resolved from action declarations.
    pub fn new(
        descriptors: impl IntoIterator<Item = Resolved>,
        agent: Option<String>,
    ) -> anyhow::Result<Self> {
        let mut tools = BTreeMap::new();
        for descriptor in descriptors {
            for tool in link_component::tools::commands(&descriptor.commands) {
                ensure!(
                    tools.insert(tool.name(), tool).is_none(),
                    "duplicate MCP tool name"
                );
            }
        }
        Ok(Self { tools, agent })
    }

    pub fn installed(agent: Option<String>) -> anyhow::Result<Self> {
        let output = link_component::asking(&mut link_core::lnk())
            .arg("--link-registry")
            .stdin(Stdio::null())
            .output()?;
        ensure!(
            output.status.success(),
            "the core could not describe its registry: {}",
            link_format::clean(&String::from_utf8_lossy(&output.stderr))
        );
        let registry = link_component::parse_registry(&output.stdout)?;
        Self::new(
            registry
                .into_iter()
                .filter(|(id, _)| link_core::find_module(id).is_some())
                .map(|(_, d)| d),
            agent,
        )
    }

    pub fn reads(&self) -> Vec<String> {
        self.tools
            .iter()
            .filter(|(_, tool)| tool.command.tool.as_ref().is_some_and(|p| p.read_only))
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub async fn answer(&self, line: &str) -> Option<Value> {
        let message: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => return Some(error(Value::Null, -32700, "Invalid JSON")),
        };
        if !message.is_object()
            || message["jsonrpc"] != "2.0"
            || !message["method"].is_string()
            || message
                .get("params")
                .is_some_and(|params| !params.is_object())
            || message
                .get("id")
                .is_some_and(|id| !id.is_null() && !id.is_string() && !id.is_i64())
        {
            return Some(error(Value::Null, -32600, "Invalid JSON-RPC request"));
        }
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id")?.clone();
        let Some(method) = method else {
            return Some(error(id, -32600, "Missing method"));
        };
        let params = &message["params"];
        let result = match method {
            "initialize" => {
                json!({"protocolVersion":SETTINGS.protocol_version, "capabilities":{"tools":{}},
                "serverInfo":{"name":"link","version":env!("CARGO_PKG_VERSION")},"instructions":SETTINGS.instructions})
            }
            "ping" => json!({}),
            "tools/list" => {
                json!({"tools":self.tools.values().map(|tool| tool.offered(self.agent.is_some())).collect::<Vec<_>>()})
            }
            "tools/call" => {
                let Some(tool) = params["name"]
                    .as_str()
                    .and_then(|name| self.tools.get(name))
                else {
                    return Some(error(id, -32602, "Unknown or unavailable tool"));
                };
                let input = params.get("arguments").cloned().unwrap_or(json!({}));
                let args = match tool.arguments(&input, self.agent.as_deref()) {
                    Ok(args) => args,
                    Err(err) => return Some(error(id, -32602, &err.to_string())),
                };
                match execute(tool, args).await {
                    Ok(answer) => answer,
                    Err(err) => {
                        json!({"isError":true,"content":[{"type":"text","text":err.to_string()}]})
                    }
                }
            }
            _ => return Some(error(id, -32601, "Unknown method")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
}

async fn execute(tool: &Tool, args: Vec<String>) -> anyhow::Result<Value> {
    let policy = tool.command.tool.as_ref().context("not a command tool")?;
    let mut command = link_core::lnk();
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut exits = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::child())?;
    let mut child = link_secretive::process::OwnedChild::spawn(&mut command)?;
    let stdout = std::os::fd::OwnedFd::from(
        child
            .child()
            .stdout
            .take()
            .context("missing command stdout")?,
    );
    let stderr = std::os::fd::OwnedFd::from(
        child
            .child()
            .stderr
            .take()
            .context("missing command stderr")?,
    );
    let mut stdout =
        tokio::fs::File::from_std(std::fs::File::from(stdout)).take(policy.limits.bytes as u64 + 1);
    let mut stderr =
        tokio::fs::File::from_std(std::fs::File::from(stderr)).take(SETTINGS.stderr_max + 1);
    let status = async move {
        while !child.exited()? {
            exits
                .recv()
                .await
                .context("child exit observer closed")
                .map_err(std::io::Error::other)?;
        }
        child.finish()
    };
    let (status, data, diagnostic) =
        tokio::time::timeout(Duration::from_millis(policy.limits.time_ms), async {
            tokio::try_join!(
                status,
                bounded(&mut stdout, policy.limits.bytes as u64),
                bounded(&mut stderr, SETTINGS.stderr_max)
            )
        })
        .await
        .context("command exceeded its declared time limit")??;
    ensure!(
        status.success(),
        "command failed: {}",
        link_format::clean(&String::from_utf8_lossy(&diagnostic))
    );
    let text = String::from_utf8(data).context("command returned non-text output")?;
    content(text, policy.json)
}

async fn bounded(
    mut input: impl tokio::io::AsyncRead + Unpin,
    max: u64,
) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).await?;
    if bytes.len() as u64 > max {
        return Err(std::io::Error::other("command exceeded its answer limit"));
    }
    Ok(bytes)
}

fn content(text: String, structured: bool) -> anyhow::Result<Value> {
    let mut result = json!({"content":[{"type":"text","text":text}]});
    if structured {
        let value: Value = serde_json::from_str(&text).context("command returned invalid JSON")?;
        result["structuredContent"] = if value.is_object() {
            value
        } else {
            json!({"result":value})
        };
    }
    Ok(result)
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(unit);

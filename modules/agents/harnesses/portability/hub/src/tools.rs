//! Link's tools and memory, each an MCP server at its address on Link's
//! proxy, given to the harness in each session it makes, resumes or
//! loads (ACP's `mcpServers`), never written into its own settings. An
//! adapter's `configure` keeps them ([`Tools::keep`]) and its `serve`
//! hands them to the hub ([`crate::Config::tools`]).

use std::collections::BTreeMap;
use std::path::Path;

use agent_client_protocol::schema::v1::{AgentCapabilities, McpServer, McpServerHttp};
use anyhow::Context;
use link_harness_contract::harness::Settings;

/// Each server by name, at its address.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tools {
    servers: BTreeMap<String, String>,
    pub(crate) actions: link_harness_contract::sandbox::namespace::Endpoints,
}

/// What [`Tools::keep`] writes.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept {
    servers: BTreeMap<String, String>,
    actions: link_harness_contract::sandbox::namespace::Endpoints,
}

link_files::specification!(Kept, include_str!("tools.schema.toml"));

impl Tools {
    /// What `settings` give: each of its `tools`, and `memory`.
    pub fn of(settings: &Settings) -> Self {
        Self {
            servers: settings.mcp_servers(),
            actions: settings.actions.clone(),
        }
    }

    /// Kept at `path`, readable only by this user, as each address holds
    /// the proxy's secret.
    pub fn keep(&self, path: &Path) -> anyhow::Result<()> {
        let text = link_files::saved::print_json(&Kept {
            servers: self.servers.clone(),
            actions: self.actions.clone(),
        })?;
        link_files::write_private(path, &text)
            .with_context(|| format!("writing {}", path.display()))
    }

    /// What [`Tools::keep`] kept at `path`; none before the first.
    pub fn kept(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let Kept { servers, actions } = link_files::saved::parse_json(&bytes)
                    .with_context(|| format!("reading {}", path.display()))?;
                Ok(Self { servers, actions })
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    /// A session's `mcpServers`, each over HTTP, for a harness that says it
    /// takes them so; none for one that doesn't ([`Tools::unserved`] says so).
    pub(crate) fn servers(&self, caps: &AgentCapabilities) -> Vec<McpServer> {
        if !caps.mcp_capabilities.http {
            return Vec::new();
        }
        self.servers
            .iter()
            .map(|(name, url)| McpServer::Http(McpServerHttp::new(name, url)))
            .collect()
    }

    /// What's left out for a harness that takes no MCP server over HTTP.
    pub(crate) fn unserved(&self, caps: &AgentCapabilities) -> Option<String> {
        (!caps.mcp_capabilities.http && !self.servers.is_empty()).then(|| {
            let names: Vec<&str> = self.servers.keys().map(String::as_str).collect();
            format!(
                "The harness takes no MCP server over HTTP: Link's {} aren't served to it",
                names.join(", ")
            )
        })
    }
}

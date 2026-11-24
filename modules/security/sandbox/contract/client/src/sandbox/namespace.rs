//! One MCP namespace over existing local server definitions. The host
//! chooses every target; callers choose only an action and its arguments.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Namespace {
    #[serde(default)]
    pub asking: Option<crate::routes::Asking>,
    pub name: String,
    pub dir: PathBuf,
    pub timeout_ms: u64,
    pub actions: BTreeMap<String, Action>,
}

// A namespace's definition file, written by the program that defines it
// and served by `lnk-sandbox mcp-namespace <file>`, naming its schema.
link_files::specification!(Namespace, include_str!("namespace.schema.toml"));

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    /// Keep an explicit provider selector even when only one target is available.
    #[serde(default)]
    pub select: bool,
    /// Multiple targets are selected by `kind`, the map's keys.
    pub targets: BTreeMap<String, Target>,
    /// With no kind, call every target. Only read actions use this.
    #[serde(default)]
    pub broadcast: bool,
    /// Never listed (`tools/list`), so only a caller that knows its name
    /// reaches it, checked and gated as any other when called.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unlisted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub server: String,
    pub tool: String,
}

impl Namespace {
    pub fn check(&self) -> anyhow::Result<()> {
        crate::routes::check_tool_name(&self.name)?;
        anyhow::ensure!(self.timeout_ms > 0, "a namespace needs a positive timeout");
        anyhow::ensure!(!self.actions.is_empty(), "a namespace has actions");
        for (name, action) in &self.actions {
            crate::routes::check_tool_name(name)?;
            anyhow::ensure!(!action.targets.is_empty(), "{name} has no target");
            for (kind, target) in &action.targets {
                crate::routes::check_tool_name(kind)?;
                crate::routes::check_tool_name(&target.server)?;
                anyhow::ensure!(
                    !target.tool.is_empty()
                        && target.tool.len() <= 128
                        && target
                            .tool
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte)),
                    "invalid MCP operation name"
                );
            }
        }
        Ok(())
    }
}

/// A caller's granted action routes, handed with its harness settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoints {
    pub urls: BTreeMap<String, String>,
    pub actions: Vec<String>,
}
impl Endpoints {
    pub fn is_empty(&self) -> bool {
        self.urls.is_empty() && self.actions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_operation_names_are_distinct_from_registered_server_names() {
        let mut namespace = Namespace {
            asking: None,
            name: "tool".into(),
            dir: "/tmp/example".into(),
            timeout_ms: 1000,
            actions: [(
                "list".into(),
                Action {
                    select: false,
                    broadcast: false,
                    unlisted: false,
                    targets: [(
                        "context".into(),
                        Target {
                            server: "context-main".into(),
                            tool: "tool_list".into(),
                        },
                    )]
                    .into(),
                },
            )]
            .into(),
        };
        namespace.check().unwrap();
        let target = namespace
            .actions
            .get_mut("list")
            .unwrap()
            .targets
            .get_mut("context")
            .unwrap();
        target.tool = "../tool_list".into();
        assert!(namespace.check().is_err());
    }
}

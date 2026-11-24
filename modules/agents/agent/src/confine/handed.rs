//! The names an agent's harness is handed its tools by: a team's memory,
//! a screen, a namespace and Link MCP by a name of their own, not
//! the sandbox's per-agent one, in its settings and its environment alike.

use link_agent::settings::Settings;

use super::{team_tools, tools_for};
use crate::screens;

/// The tools harness `name` is set up with, behind its proxy: each one's
/// address on its proxy, by name. Its memory is `memory`
/// ([`Settings::memory_url`]). None without a proxy, which serves them.
pub(crate) fn harness_tools(
    settings: &Settings,
    name: &str,
) -> std::collections::BTreeMap<String, String> {
    if settings.proxy_of(name).is_none() {
        return Default::default();
    }
    let own: Vec<String> = settings.memory_tool().into_iter().collect();
    let Ok(secret) = settings.url_secret() else {
        return Default::default();
    };
    let renamed = handed_names(settings);
    tools_for(settings, name)
        .into_iter()
        .filter(|tool| !own.contains(tool))
        .map(|tool| {
            let url = link_sandbox_client::routes::tool_url(settings.proxy_port(), &tool, &secret);
            let handed = renamed
                .iter()
                .find(|(_, t)| *t == tool)
                .map_or(tool, |(handed, _)| handed.clone());
            (handed, url)
        })
        .collect()
}

/// The tools an agent's harness is handed under a name of their own,
/// each as `(handed, tool)`: a team's memory by the team's name, a screen,
/// a namespace and Link MCP by theirs (`browser`, not
/// `browser-<key>`), so the model calls `browser__open` whatever the
/// agent's key. The sandbox's tool keeps its per-agent name, which keeps
/// two agents' tools apart on one machine.
pub(super) fn handed_names(settings: &Settings) -> Vec<(String, String)> {
    team_tools(settings)
        .into_iter()
        .chain(screens::handed(settings))
        .chain(crate::namespaces::handed(settings))
        .chain(crate::mcp_tool::handed(settings))
        .collect()
}

/// Moves each of `renamed`'s tools that the proxy serves (`tools`) from
/// the variable its route gave it (`$LNK_TOOL_BROWSER_<KEY>`) to its
/// handed name's (`$LNK_TOOL_BROWSER`), at its address on the proxy
/// (`url`), so a harness names its tools as the agent's docs do.
pub(super) fn rename_tool_env(
    renamed: &[(String, String)],
    tools: &[String],
    url: impl Fn(&str) -> String,
    env: &mut std::collections::BTreeMap<String, String>,
) {
    use link_sandbox_client::routes::tool_variable;
    for (handed, tool) in renamed {
        if tools.contains(tool) {
            env.remove(&tool_variable(tool));
            env.insert(tool_variable(handed), url(tool));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tool handed under a name of its own reaches the harness under
    /// that name, whatever the agent's key: `browser-<key>`'s address is
    /// `$LNK_TOOL_BROWSER`, so the model calls `browser__open`. One the
    /// proxy doesn't serve, and every other variable, is left alone.
    #[test]
    fn handed_tools_reach_the_harness_by_their_handed_names() {
        let renamed = [
            ("browser".to_string(), "browser-ci9zhty85c49".to_string()),
            ("mcp".to_string(), "mcp-ci9zhty85c49".to_string()),
            ("research-memory".to_string(), "memory-team-7".to_string()),
            ("desktop".to_string(), "desktop-ci9zhty85c49".to_string()),
        ];
        let tools = [
            "browser-ci9zhty85c49".to_string(),
            "mcp-ci9zhty85c49".to_string(),
            "memory-team-7".to_string(),
            "gh".to_string(),
        ];
        let mut env: std::collections::BTreeMap<String, String> = [
            ("LNK_TOOL_BROWSER_CI9ZHTY85C49", "b"),
            ("LNK_TOOL_MCP_CI9ZHTY85C49", "l"),
            ("LNK_TOOL_MEMORY_TEAM_7", "m"),
            ("LNK_TOOL_GH", "g"),
            ("HOME", "/home/me"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        rename_tool_env(&renamed, &tools, |tool| format!("url:{tool}"), &mut env);
        let names: Vec<_> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        assert_eq!(
            names,
            [
                "HOME=/home/me",
                "LNK_TOOL_BROWSER=url:browser-ci9zhty85c49",
                "LNK_TOOL_GH=g",
                "LNK_TOOL_MCP=url:mcp-ci9zhty85c49",
                "LNK_TOOL_RESEARCH_MEMORY=url:memory-team-7",
            ]
        );
    }
}

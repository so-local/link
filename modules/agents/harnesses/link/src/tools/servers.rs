//! The MCP servers an agent may call, as its environment names them
//! ([`Server`], [`from_env`]).

use super::*;

/// An MCP server the agent may call: its name, as the model sees it
/// before each of its tools' (`memory`, `gh`), its address, and whether
/// its session is kept between turns.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Server {
    pub name: String,
    pub url: String,
    /// Memory's, run's, file's and code's, and the `task` and `file`
    /// namespaces over them: they're the agent's own in
    /// every conversation and keep nothing of one in their session (each
    /// call says its conversation; code's servers hold only the files
    /// folder), so one session serves them all and stays started. Any other tool's session ends with its turn: a tool may
    /// keep what one conversation did (a browser's pages, a shell's
    /// folder), and one person's turn mustn't find it in another's.
    pub kept: bool,
}

/// The servers in an environment: memory at `$LNK_MEMORY`, run at
/// `$LNK_RUN`, file at `$LNK_FILE` and code at `$LNK_CODE`, then each
/// `$LNK_TOOL_<NAME>` as `<name>` in lowercase, by name, an address given
/// twice kept once. A name with __, or starting or ending with `_`, is
/// refused, so no two servers' tools can have one name.
/// Whether `key` is a variable [`from_env`] reads: a tool's address.
pub fn names_a_tool(key: &str) -> bool {
    [
        link_memory_tool_contract::MEMORY_ENV,
        link_harness_contract::harness::RUN_ENV,
        link_harness_contract::harness::FILE_ENV,
        link_harness_contract::harness::CODE_ENV,
    ]
    .contains(&key)
        || key.starts_with("LNK_TOOL_")
}

pub fn from_env(vars: impl IntoIterator<Item = (String, String)>) -> Vec<Server> {
    let mut memory = None;
    let (mut run, mut file, mut code) = (None, None, None);
    let mut named = Vec::new();
    for (key, url) in vars {
        if url.is_empty() {
            continue;
        }
        if key == link_memory_tool_contract::MEMORY_ENV {
            memory = Some(url);
        } else if key == link_harness_contract::harness::RUN_ENV {
            run = Some(url);
        } else if key == link_harness_contract::harness::FILE_ENV {
            file = Some(url);
        } else if key == link_harness_contract::harness::CODE_ENV {
            code = Some(url);
        } else if let Some(name) = key.strip_prefix("LNK_TOOL_")
            && !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            if name.contains(SEPARATOR) || name.starts_with('_') || name.ends_with('_') {
                crate::warn!(
                    "tool {key} left out: its name can't have __, or start or end with an underscore"
                );
                continue;
            }
            let name = name.to_ascii_lowercase();
            let kept = link_harness_contract::harness::KEPT_NAMESPACES.contains(&name.as_str());
            named.push(Server { name, url, kept });
        }
    }
    named.sort_by(|a, b| a.name.cmp(&b.name));
    let mut servers: Vec<Server> = memory
        .map(|url| Server {
            name: "memory".into(),
            url,
            kept: true,
        })
        .into_iter()
        .chain(run.map(|url| Server {
            name: "run".into(),
            url,
            kept: true,
        }))
        .chain(file.map(|url| Server {
            name: "file".into(),
            url,
            kept: true,
        }))
        .chain(code.map(|url| Server {
            name: "code".into(),
            url,
            kept: true,
        }))
        .collect();
    for server in named {
        if !servers
            .iter()
            .any(|s| s.url == server.url || s.name == server.name)
        {
            servers.push(server);
        }
    }
    servers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn servers_from_the_environment() {
        let servers = from_env(vars(&[
            ("LNK_TOOL_GH", "http://127.0.0.1:1/_link/mcp/gh"),
            (
                "LNK_TOOL_MEMORY_MAIN",
                "http://127.0.0.1:1/_link/mcp/memory-main",
            ),
            ("LNK_MEMORY", "http://127.0.0.1:1/_link/mcp/memory-main"),
            ("LNK_FILE", "http://127.0.0.1:1/_link/mcp/file-main"),
            ("LNK_RUN", "http://127.0.0.1:1/_link/mcp/run-main"),
            ("LNK_CODE", "http://127.0.0.1:1/_link/mcp/code-main"),
            (
                "LNK_TOOL_RUN",
                "http://127.0.0.1:1/_link/mcp/someone-elses-run",
            ),
            ("LNK_TOOL_AWS_PROD", "http://127.0.0.1:1/_link/mcp/aws-prod"),
            (
                "LNK_TOOL_TASK",
                "http://127.0.0.1:1/_link/mcp/namespace-task-a1",
            ),
            (
                "LNK_TOOL_AGENT",
                "http://127.0.0.1:1/_link/mcp/namespace-agent-a1",
            ),
            ("LNK_TOOL_", "http://x"),
            ("LNK_TOOL_bad", "http://x"),
            ("LNK_TOOL_EMPTY", ""),
            ("LNK_TOOL_GH__X", "http://127.0.0.1:1/_link/mcp/gh-x"),
            ("LNK_TOOL__GH", "http://127.0.0.1:1/_link/mcp/gh-y"),
            ("LNK_TOOL_GH_", "http://127.0.0.1:1/_link/mcp/gh-z"),
            ("HOME", "/h"),
        ]));
        let names: Vec<_> = servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "memory", "run", "file", "code", "agent", "aws_prod", "gh", "task"
            ]
        );
        assert!(servers[1].url.ends_with("/run-main"));
        // Memory's, run's, file's and code's sessions are kept between
        // turns, and the task namespace's over them; not agent's.
        let kept: Vec<bool> = servers.iter().map(|s| s.kept).collect();
        assert_eq!(kept, [true, true, true, true, false, false, false, true]);
        assert!(from_env(Vec::new()).is_empty());
    }
}

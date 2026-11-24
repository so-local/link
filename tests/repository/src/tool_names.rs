//! The tool names a model is told. Link hands an agent its actions as
//! namespaces (`task__run`, `agent__send`), whichever harness runs it, so
//! every `<namespace>__<action>` a page, a program's own words or a test
//! case names must be one the agent module's `actions.toml` lets an agent
//! call as a tool. A provider's own name for an action a namespace now
//! holds (`run__command`, `scheduler__send`) is refused by name: a model
//! is never offered it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The namespaces whose names are checked: the harness contract's, and
/// the screens'. Web's tool server keeps its own names (`web__fetch`).
const NAMESPACES: &[&str] = &[
    "task", "cron", "tool", "file", "agent", "question", "browser", "desktop",
];

/// The providers whose actions namespaces took, with where each went.
const RETIRED: &[(&str, &str)] = &[
    (
        "run",
        "task (task__run, task__list, task__read, task__stop)",
    ),
    (
        "scheduler",
        "task (task__agent, task__send), cron (cron__create, cron__delete, cron__list) and agent (agent__send)",
    ),
];

/// The names an agent may call as tools, `<namespace>__<action>`: each
/// `["<namespace> <action>"]` of `actions.toml` that allows an agent on
/// the tool surface, read as the agent module reads it.
pub fn callable(actions: &str) -> BTreeSet<String> {
    use link_component::actions::{Caller, Surface};
    link_component::actions::parse(actions)
        .expect("actions.toml parses")
        .iter()
        .filter(|(_, action)| action.allows(Caller::Agent, Surface::Tool))
        .filter_map(
            |(path, _)| match path.split(' ').collect::<Vec<_>>().as_slice() {
                [namespace, action] => Some(format!("{namespace}__{action}")),
                _ => None,
            },
        )
        .collect()
}

/// The names a tool module offers an agent itself, `<namespace>__<name>`:
/// each action of `actions` on the tool surface alone, by its `[tool]`
/// name, the namespace its folder's name less `-tool` (`browser-tool`'s
/// `back` is `browser__back`), as a screen's calls that are the agent's
/// alone are.
pub fn offered(namespace: &str, actions: &str) -> BTreeSet<String> {
    let table: toml::Table = toml::from_str(actions).expect("a tool's actions.toml parses");
    let Some(toml::Value::Table(actions)) = table.get("actions") else {
        return BTreeSet::new();
    };
    actions
        .values()
        .filter_map(|action| {
            let on_tool = action
                .get("surfaces")
                .and_then(|s| s.as_array())
                .is_some_and(|s| s.iter().any(|s| s.as_str() == Some("tool")));
            let name = action.get("tool")?.get("name")?.as_str()?;
            on_tool.then(|| format!("{namespace}__{name}"))
        })
        .collect()
}

/// What every tool module beside the agent module (`tools/<name>-tool`)
/// offers an agent itself ([`offered`]).
fn offered_by_tools(agent_actions: &Path) -> BTreeSet<String> {
    let Some(tools) = agent_actions
        .parent()
        .and_then(Path::parent)
        .map(|agents| agents.join("tools"))
    else {
        return BTreeSet::new();
    };
    let Ok(entries) = std::fs::read_dir(&tools) else {
        return BTreeSet::new();
    };
    let mut names = BTreeSet::new();
    for entry in entries.flatten() {
        let folder = entry.file_name().to_string_lossy().into_owned();
        let Some(namespace) = folder.strip_suffix("-tool") else {
            continue;
        };
        if let Ok(text) = std::fs::read_to_string(entry.path().join("actions.toml")) {
            names.extend(offered(namespace, &text));
        }
    }
    names
}

/// Each `<word>__<word>` in `text`, with its line, its namespace and its
/// action.
fn names(text: &str) -> Vec<(usize, String, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let word = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
        for token in line.split(|c: char| !word(c)) {
            if let Some((namespace, action)) = token.split_once("__")
                && !namespace.is_empty()
                && !action.is_empty()
                && !namespace.starts_with('_')
                // A deeper name is an action of a tool's own, such as
                // Link's (`link_<id>__agent__tool__list`): no namespace's.
                && !action.contains("__")
            {
                found.push((index + 1, namespace.to_string(), action.to_string()));
            }
        }
    }
    found
}

/// What a file says a model sees: a page's whole text, and a Rust file's
/// code up to its tests, without comments, which may name a provider's
/// own tools. A test file says nothing.
fn said(path: &Path, text: &str) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    if name.ends_with(".md") || name.ends_with(".sh") {
        return Some(text.to_string());
    }
    let tests = path.components().any(|part| part.as_os_str() == "tests");
    if !name.ends_with(".rs") || tests || name == "tests.rs" {
        return None;
    }
    let code: Vec<&str> = text
        .lines()
        .take_while(|line| !line.contains("#[cfg(test)]"))
        .map(|line| match line.trim_start().starts_with("//") {
            true => "",
            false => line,
        })
        .collect();
    Some(code.join("\n"))
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            if !["target", "node_modules", ".git"].contains(&name.to_string_lossy().as_ref()) {
                walk(&path, found);
            }
        } else {
            found.push(path);
        }
    }
}

/// The tool names under `dirs` of `root` that no agent is offered, each
/// as `<file>:<line>: <name>: <why>`, none when every one is callable
/// as `actions` (the agent module's `actions.toml`) says.
pub fn problems(root: &Path, dirs: &[&str], actions: &Path) -> Vec<String> {
    let mut callable = callable(&std::fs::read_to_string(actions).expect("actions.toml reads"));
    callable.extend(offered_by_tools(actions));
    let mut files = Vec::new();
    for dir in dirs {
        walk(&root.join(dir), &mut files);
    }
    files.sort();
    let mut problems = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Some(said) = said(&file, &text) else {
            continue;
        };
        let shown = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string();
        for (line, namespace, action) in names(&said) {
            let name = format!("{namespace}__{action}");
            if let Some((_, went)) = RETIRED.iter().find(|(retired, _)| *retired == namespace) {
                problems.push(format!(
                    "{shown}:{line}: {name}: no agent is offered {namespace}'s own tools; its actions are in {went}"
                ));
            } else if NAMESPACES.contains(&namespace.as_str()) && !callable.contains(&name) {
                problems.push(format!(
                    "{shown}:{line}: {name}: actions.toml has no [\"agent {namespace} {action}\"] an agent may call as a tool"
                ));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The agent module's own registry, as the agent module reads it.
    const ACTIONS: &str = include_str!("../../../modules/agents/agent/actions.toml");

    #[test]
    fn only_an_action_an_agent_may_call_as_a_tool_is_callable() {
        let callable = callable(ACTIONS);
        for offered in [
            "task__run",
            "task__read",
            "file__read",
            "agent__send",
            "question__create",
        ] {
            assert!(callable.contains(offered), "{offered} isn't callable");
        }
        // An owner's command, never a tool: lnk agent start.
        assert!(!callable.contains("agent__start"));
        assert!(!callable.contains("run__command"));
        // A background command is task__run's run_in_background.
        assert!(!callable.contains("task__start"));
    }

    #[test]
    fn a_tool_offers_its_tool_surface_alone_by_its_namespace() {
        let offered = offered(
            "browser",
            "[actions.\"browser-tool.browser-tool.back\"]\nsurfaces = [\"tool\"]\n\
             [actions.\"browser-tool.browser-tool.back\".tool]\nname = \"back\"\n\
             [actions.\"browser-tool.browser-tool.serve\"]\nrole = \"plumbing\"\n",
        );
        assert_eq!(offered, BTreeSet::from(["browser__back".to_string()]));
    }

    #[test]
    fn a_page_names_and_a_rust_file_names_only_outside_comments_and_tests() {
        let found = |path: &str, text: &str| {
            said(Path::new(path), text).map(|said| {
                names(&said)
                    .into_iter()
                    .map(|(_, n, a)| format!("{n}__{a}"))
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(
            found("a.md", "call `task__run`, then LNK_TOOL_X"),
            Some(vec!["task__run".into()])
        );
        assert_eq!(
            found(
                "a.rs",
                "// run__command\nlet s = \"task__read\";\n#[cfg(test)]\nlet t = \"run__stop\";"
            ),
            Some(vec!["task__read".into()])
        );
        assert_eq!(found("x/tests/a.rs", "\"run__stop\""), None);
        assert_eq!(found("a.sh", "link_x__agent__tool__list"), Some(vec![]));
        assert_eq!(found("a/__init__.py", "x"), None);
    }
}

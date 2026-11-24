//! Namespace MCP servers over the agent's existing action providers.
//! Definitions retain the providers' own confinement and approval rules.

use crate::{confine, run_file, scheduler};
use link_agent::settings::{self, Settings};
use link_format::WithinExt;
use link_harness_contract::harness::namespaces::{self, Kind};
use link_sandbox_client::namespace::{Action, Namespace, Target};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) const NAMES: &[&str] = &link_harness_contract::harness::NAMESPACES;
pub(crate) const DIRECT: &[&str] = &["web", "browser", "desktop"];

pub(crate) fn file(agent: &Settings, namespace: &str) -> PathBuf {
    settings::config_of(&agent.name).join(format!("namespace-{namespace}.json"))
}

fn tool(agent: &Settings, namespace: &str) -> String {
    format!("namespace-{namespace}-{}", agent.key())
}

pub(crate) fn handed(agent: &Settings) -> Vec<(String, String)> {
    NAMES
        .iter()
        .filter(|name| file(agent, name).is_file())
        .map(|name| (name.to_string(), tool(agent, name)))
        .collect()
}

/// The tool serving each kind of provider for `agent` on this machine.
fn provider(agent: &Settings, kind: Kind) -> Option<String> {
    match kind {
        Kind::Run => run_file::tool(agent, "run"),
        Kind::Schedule => scheduler::tool(agent),
        Kind::File => run_file::tool(agent, "file"),
        Kind::Code => run_file::tool(agent, "code"),
        Kind::Context => Some(context(agent)),
    }
}

fn context(agent: &Settings) -> String {
    format!("context-{}", agent.key())
}

fn actions(agent: &Settings, namespace: &str) -> BTreeMap<String, Action> {
    if !DIRECT.contains(&namespace) {
        return namespaces::actions(namespace, |kind| provider(agent, kind));
    }
    let source = if namespace == "web" {
        agent
            .tools_of(agent.harness())
            .into_iter()
            .find(|name| name == "web")
    } else {
        run_file::tool(agent, namespace)
    };
    let Some(source) = source else {
        return BTreeMap::new();
    };
    link_agent_contract::actions()
        .into_iter()
        .filter_map(|(path, action)| {
            let name = path.strip_prefix(&format!("{namespace} "))?;
            action
                .allows(
                    link_agent_contract::action_registry::Caller::Agent,
                    link_agent_contract::action_registry::Surface::Tool,
                )
                .then(|| {
                    let target = Target {
                        server: source.clone(),
                        tool: name.into(),
                    };
                    (
                        name.to_string(),
                        Action {
                            select: false,
                            targets: [(namespace.to_string(), target)].into(),
                            broadcast: false,
                            unlisted: false,
                        },
                    )
                })
        })
        .collect()
}

/// Register each available namespace before the harness receives its tools.
pub(crate) fn define(agent: &Settings) -> anyhow::Result<()> {
    let context = context(agent);
    let output = link_core::lnk()
        .env("LNK_INSTALLATION_DIR", link_files::try_installation_dir()?)
        .args([
            "sandbox",
            "tool",
            "add",
            &context,
            "--ask",
            "none",
            "--env",
            "LNK_INSTALLATION_DIR",
            "--",
        ])
        .arg(std::env::current_exe()?)
        .args([
            "--agent",
            &agent.name,
            "context-mcp",
            "--agent-id",
            &agent.key(),
        ])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "registering the agent's context provider failed"
    );
    // Only the context broker uses this definition. Keep the original
    // teammate provider's approval, fingerprints and scheduler caps.
    let peer = file(agent, "agent-peer");
    let peers = namespaces::actions("agent", |kind| {
        (kind == Kind::Schedule)
            .then(|| scheduler::tool(agent))
            .flatten()
    });
    let timeout = agent
        .spec()?
        .spec
        .tools
        .and_then(|tools| tools.timeout)
        .within("the agent spec sets no tools.timeout")?;
    if peers.is_empty() {
        let _ = std::fs::remove_file(peer);
    } else {
        let definition = Namespace {
            asking: Some(link_sandbox_client::routes::Asking {
                agent: agent.name.clone(),
                wait_secs: question_wait(agent)?.div_ceil(1000),
            }),
            name: "agent-peer".into(),
            dir: link_files::layout::Hidden::Sandboxes.try_path()?,
            timeout_ms: link_agent_spec::millis(&timeout),
            actions: peers,
        };
        definition.check()?;
        link_files::write_private(&peer, &link_files::saved::print_json(&definition)?)?;
    }
    for namespace in NAMES.iter().chain(DIRECT) {
        let file = file(agent, namespace);
        let tool = tool(agent, namespace);
        let actions = actions(agent, namespace);
        if actions.is_empty() {
            let _ = std::fs::remove_file(file);
            let _ = link_core::lnk()
                .args(["sandbox", "tool", "remove", &tool])
                .output();
            continue;
        }
        let program = run_file::program("sandbox")?;
        let timeout = agent
            .spec()?
            .spec
            .tools
            .and_then(|tools| tools.timeout)
            .within("the agent spec sets no tools.timeout")?;
        let definition = Namespace {
            asking: Some(link_sandbox_client::routes::Asking {
                agent: agent.name.clone(),
                wait_secs: question_wait(agent)?.div_ceil(1000),
            }),
            name: namespace.to_string(),
            dir: link_files::layout::Hidden::Sandboxes.try_path()?,
            timeout_ms: if *namespace == "question" {
                // The owner deadline is separate from ordinary tool execution time.
                question_wait(agent)?.saturating_add(link_agent_spec::millis(&timeout))
            } else {
                link_agent_spec::millis(&timeout)
            },
            actions,
        };
        definition.check()?;
        link_files::write_private(&file, &link_files::saved::print_json(&definition)?)?;
        if DIRECT.contains(namespace) {
            continue;
        }
        // The namespace does no effects of its own. Its server checks
        // each original provider's approval policy before forwarding.
        let output = link_core::lnk()
            .args(["sandbox", "tool", "add", &tool, "--ask", "none", "--"])
            .arg(&program)
            .arg("mcp-namespace")
            .arg(&file)
            .output()
            .within("registering a namespace MCP server")?;
        anyhow::ensure!(
            output.status.success(),
            "registering {namespace}: {}",
            link_format::clean(&String::from_utf8_lossy(&output.stderr))
        );
    }
    confine::forget_defined_tools();
    Ok(())
}

fn question_wait(agent: &Settings) -> anyhow::Result<u64> {
    const MAX_WAIT_MS: u64 = 24 * 60 * 60 * 1000;
    let spec = agent.spec()?.spec;
    let mut wait = spec
        .connections
        .as_ref()
        .and_then(|c| c.ask_wait)
        .map(|wait| u64::try_from(wait.get().as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default();
    for domain in spec.approvals.iter().flat_map(|a| a.domains.values()) {
        if domain.allow_request_overrides == Some(true) {
            return Ok(MAX_WAIT_MS);
        }
        if let Some(seconds) = domain.wait_secs {
            wait = wait.max(seconds.saturating_mul(1000));
        }
    }
    Ok(wait)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider for every kind but the context service, which a
    /// machine's agent alone has.
    fn served(kind: Kind) -> Option<String> {
        (kind != Kind::Context).then(|| format!("{}-fixture", kind.as_str()))
    }

    /// Derive exposed names from production routing, not another list of aliases.
    #[test]
    fn owner_acting_provider_routes_are_never_stranger_grants() {
        let mut checked = 0;
        for namespace in NAMES {
            for (action, route) in namespaces::actions(namespace, served) {
                let tool = format!("{namespace}__{action}");
                let text = format!("schema = 1\n[tools]\nstrangers = [\"{tool}\"]\n");
                let result = link_agent_spec::parse(&text);
                if route
                    .targets
                    .keys()
                    .any(|provider| matches!(provider.as_str(), "run" | "schedule" | "code"))
                {
                    checked += 1;
                    assert!(result.is_err(), "{tool} would lose the stranger's standing");
                } else {
                    assert!(result.is_ok(), "{tool} remains explicitly delegable");
                }
            }
        }
        assert!(
            checked > 0,
            "the fixture must include owner-acting providers"
        );
        // The agent wrapper replaces the scheduler target with its owner
        // context service; sending still acts as this agent.
        let agent = actions(&Settings::new("stranger-policy-fixture"), "agent");
        assert!(agent.contains_key("send"));
        assert!(
            link_agent_spec::parse("schema = 1\n[tools]\nstrangers = [\"agent__send\"]\n").is_err()
        );
        assert!(
            link_agent_spec::parse("schema = 1\n[tools]\nstrangers = [\"agent__show\"]\n").is_ok()
        );
    }

    /// Every declared action a model is offered is one the registry lets
    /// an agent call as a tool, so no namespace offers what its routes then
    /// refuse; the harness's own calls are offered to no model, and are
    /// none of the registry's.
    #[test]
    fn each_declared_action_is_an_agents_tool() {
        for (namespace, actions) in namespaces::declared() {
            for (action, declared) in actions {
                let path = format!("{namespace} {action}");
                if declared.harness {
                    assert!(
                        !link_agent_contract::actions().contains_key(&path),
                        "{path} is the harness's alone"
                    );
                    continue;
                }
                assert!(
                    link_agent_contract::actions()
                        .get(&path)
                        .is_some_and(|declared| declared.allows(
                            link_agent_contract::action_registry::Caller::Agent,
                            link_agent_contract::action_registry::Surface::Tool,
                        )),
                    "{path} isn't an agent's tool"
                );
            }
        }
    }

    #[test]
    fn task_uses_one_server_and_file_holds_language_server_actions() {
        let task = namespaces::actions("task", served);
        assert_eq!(task["run"].targets["run"].tool, "command");
        // A background command is task__run's run_in_background, as
        // Bash's: no call of its own.
        assert!(!task.contains_key("start"));
        assert_eq!(task["agent"].targets["schedule"].tool, "task");
        assert!(!task.contains_key("schedule"));
        assert_eq!(task["send"].targets["schedule"].tool, "message");
        assert!(!task.contains_key("message"));
        assert!(task["list"].broadcast);
        assert_eq!(task["list"].targets.len(), 2);
        assert!(!task["stop"].broadcast);
        let file = namespaces::actions("file", served);
        assert_eq!(file["read"].targets["file"].server, "file-fixture");
        // As Claude Code's Read, Write, Edit, Glob and Grep.
        for name in ["read", "write", "edit", "glob", "grep"] {
            assert_eq!(file[name].targets["file"].tool, name, "{name}");
        }
        assert!(!file.contains_key("find") && !file.contains_key("replace"));
        assert_eq!(file["definition"].targets["code"].server, "code-fixture");
        // Schedules as CronCreate, CronDelete and CronList.
        let cron = namespaces::actions("cron", served);
        assert_eq!(cron["create"].targets["schedule"].tool, "schedule");
        assert_eq!(cron["delete"].targets["schedule"].tool, "unschedule");
        assert_eq!(cron["list"].targets["schedule"].tool, "schedules");
        // Each with only its trained arguments: no `kind` to choose a
        // provider, as the task namespace's list, read and stop have.
        assert!(cron.values().all(|action| !action.select));
        assert!(task["list"].select && task["stop"].select);
    }

    /// Here, Link Harness keeps a quiet conversation's compaction through
    /// the `cron` namespace it's handed: `compact`, the harness's own call
    /// to the scheduler, listed to no model, and one the scheduler's
    /// definition here never asks, even of a spec that asks first.
    #[test]
    fn compaction_reaches_the_scheduler_through_cron_here() {
        let names = link_scheduler_tool_contract::compaction::compaction();
        let cron = namespaces::actions(&names.tool, served);
        let compact = &cron[&names.call];
        assert!(compact.unlisted);
        assert_eq!(compact.targets["schedule"].server, "schedule-fixture");
        assert_eq!(compact.targets["schedule"].tool, names.call);
        for ask in [false, true] {
            let caps = link_agent_spec::Tool {
                ask: Some(ask),
                ..Default::default()
            };
            assert!(
                link_scheduler_tool_contract::allowed(&caps)
                    .unwrap()
                    .contains(&names.call),
                "ask = {ask}"
            );
        }
    }
}

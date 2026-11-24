//! Which tools a harness is handed and the addresses it receives for them.
use super::*;
use crate::{memory, run_file, screens};

/// The tools defined on this machine (`lnk sandbox tool list --json`).
pub(crate) fn defined_tools() -> anyhow::Result<Vec<String>> {
    if let Some(tools) = DEFINED_TOOLS.lock().ok().and_then(|d| d.clone()) {
        return Ok(tools);
    }
    let tools = list_defined_tools()?;
    if let Ok(mut defined) = DEFINED_TOOLS.lock() {
        *defined = Some(tools.clone());
    }
    Ok(tools)
}

/// [`defined_tools`] as last listed by this process: a start asks once.
pub(crate) static DEFINED_TOOLS: std::sync::Mutex<Option<Vec<String>>> =
    std::sync::Mutex::new(None);

/// Forgets [`defined_tools`], after a tool is added or removed.
pub(crate) fn forget_defined_tools() {
    if let Ok(mut defined) = DEFINED_TOOLS.lock() {
        *defined = None;
    }
}

pub(crate) fn list_defined_tools() -> anyhow::Result<Vec<String>> {
    Ok(crate::defined::list_tools()?
        .into_iter()
        .map(|tool| tool.name)
        .collect())
}

/// Of harness `name`'s tools, those still defined: one removed since
/// (`lnk sandbox tool remove`) is left out, said once, rather than keep
/// the agent from starting.
///
/// Its memory's tool too (`lnk agent memory`), whichever harness runs it.
pub(crate) fn tools_for(settings: &Settings, name: &str) -> Vec<String> {
    let named = settings.tools_of(name);
    let own = own_tools(settings);
    if named.is_empty() && own.is_empty() {
        return named;
    }
    let Ok(defined) = defined_tools() else {
        return Vec::new();
    };
    let (mut kept, gone): (Vec<String>, Vec<String>) =
        named.into_iter().partition(|t| defined.contains(t));
    for tool in gone {
        aside(gone_tool(settings, name, &tool));
    }
    for tool in own {
        if defined.contains(&tool) && !kept.contains(&tool) {
            kept.push(tool);
        }
    }
    kept
}

/// What's said of `tool`, named for harness `name` but not defined
/// here. One of Link's (`lnk agent tool add`), such as web on a machine an
/// agent was just imported to, was never added here: adding it is the
/// step. A tool of your own was removed since: forgetting it is.
fn gone_tool(settings: &Settings, name: &str, tool: &str) -> Report {
    if crate::tools::offered(tool) {
        let shown = Name::new("tool", tool);
        return Report::warning(sentence!(
            "Tool {} isn't added on this machine, so your agent starts without it.",
            &shown
        ))
        .next(
            cmd!(
                "lnk agent tool add {} --agent {}",
                &shown,
                Name::new("agent", &settings.name)
            ),
            "add it",
        );
    }
    Report::warning(sentence!(
        "Tool {} isn't defined anymore.",
        Foreign::new(tool)
    ))
    .next(
        cmd!(
            "lnk agent harness-permission remove tool {} --harness {}",
            Foreign::new(tool),
            Name::new("harness", name)
        ),
        "forget it",
    )
}

/// The tools Link hands an agent of its own, whichever harness runs it:
/// its memory's, its namespaces', its screens' and its teams'.
fn own_tools(settings: &Settings) -> Vec<String> {
    settings
        .memory_tool()
        .into_iter()
        .chain(
            crate::namespaces::handed(settings)
                .into_iter()
                .map(|(_, tool)| tool),
        )
        .chain(
            crate::mcp_tool::handed(settings)
                .into_iter()
                .map(|(_, tool)| tool),
        )
        .chain(screens::handed(settings).into_iter().map(|(_, tool)| tool))
        .chain(team_tools(settings).into_iter().map(|(_, tool)| tool))
        .collect()
}

/// What harness `name` of agent `settings` may reach now on the
/// sandbox's own routes, as its sandbox file and the harness's grants
/// say at this moment: the tools named for it and Link's own for it, and
/// the hosts its pages may be on. Whether a tool is still defined is the
/// bridge's to say.
pub(crate) fn reach_now(settings: &Settings, name: &str) -> link_sandbox_client::grants::Current {
    let mut tools = settings.tools_of(name);
    for tool in own_tools(settings) {
        if !tools.contains(&tool) {
            tools.push(tool);
        }
    }
    let browser = settings
        .spec_of(name, link_agent_contract::agent_sandbox::Route::Direct)
        .browser
        .and_then(|browser| browser.hosts);
    link_sandbox_client::grants::Current { tools, browser }
}

/// The tools its teams bring an agent, each by the name its harness is
/// handed it as (`<team>-memory`, at `$LNK_TOOL_<TEAM>_MEMORY`), and the
/// tool's own: each team's memory, once it's defined, for a team whose
/// whole shared folder it's granted (`.`, read or written). The memory
/// searches all of the folder, so a member granted only some of it
/// isn't handed it.
pub(crate) fn team_tools(settings: &Settings) -> Vec<(String, String)> {
    let Some(id) = settings.agent_id() else {
        return Vec::new();
    };
    let granted = sandbox_file::of(&settings.name).spec.teams;
    settings::teams_of(id)
        .unwrap_or_default()
        .iter()
        .filter(|team| {
            granted
                .get(&team.name)
                .is_some_and(link_harness_contract::harness::TeamFolders::whole)
        })
        .filter_map(|team| Some((format!("{}-memory", team.name), memory::team_tool(team)?)))
        .collect()
}

/// What harness `name`'s grants hand it as it's configured, `sandboxed`
/// or not: its tools, the risks of its own you allowed, whether it runs
/// with no sandbox, and whether behind its proxy.
pub(crate) fn hand_grants(
    handed: &mut link_harness_contract::harness::Settings,
    settings: &Settings,
    name: &str,
    sandboxed: bool,
) {
    handed.tools = harness_tools(settings, name);
    handed.read_sources = crate::read_sources::registrations(settings, handed);
    handed.allowed = Grants::of(name).risks.into_iter().collect();
    handed.unsandboxed = !sandboxed;
    handed.proxied = sandboxed && settings.proxy_of(name).is_some();
}

/// `$LNK_MEMORY` for a harness behind its proxy on `port`, calling
/// `tools`: its memory's address, when it has one among them, and its
/// run's, file's and code's beside it.
pub(crate) fn memory_env(
    settings: &Settings,
    tools: &[String],
    port: u16,
    env: &mut std::collections::BTreeMap<String, String>,
) {
    let Ok(secret) = settings.url_secret() else {
        return;
    };
    if let Some(tool) = settings.memory_tool().filter(|t| tools.contains(t)) {
        env.insert(
            link_memory_tool_contract::MEMORY_ENV.to_string(),
            link_sandbox_client::routes::tool_url(port, &tool, &secret),
        );
    }
    rename_tool_env(
        &handed_names(settings),
        tools,
        |tool| link_sandbox_client::routes::tool_url(port, tool, &secret),
        env,
    );
    // Its run, file and code, beside memory: its scheduler only through
    // its namespaces.
    let own = [
        (
            run_file::tool(settings, "run"),
            link_harness_contract::harness::RUN_ENV,
        ),
        (
            run_file::tool(settings, "file"),
            link_harness_contract::harness::FILE_ENV,
        ),
        (
            run_file::tool(settings, "code"),
            link_harness_contract::harness::CODE_ENV,
        ),
    ];
    for (tool, variable) in own {
        if let Some(tool) = tool.filter(|t| tools.contains(t)) {
            env.insert(
                variable.to_string(),
                link_sandbox_client::routes::tool_url(port, &tool, &secret),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tool of Link's an agent names but this machine never added, such
    /// as web on a machine it was just imported to, says to add it; a tool
    /// of your own removed since says to forget it.
    #[test]
    fn a_missing_tool_names_the_step_that_helps() {
        let mut settings = Settings::default();
        settings.name = "main".into();
        let said = |tool: &str| {
            let report = gone_tool(&settings, "link", tool);
            (
                report.headline.to_string(),
                report
                    .next
                    .0
                    .iter()
                    .map(|n| n.command.text())
                    .collect::<Vec<_>>(),
            )
        };
        let (headline, next) = said("web");
        assert!(
            headline.contains("isn't added on this machine"),
            "{headline}"
        );
        assert_eq!(next, ["lnk agent tool add web --agent main"]);
        let (headline, next) = said("example");
        assert!(headline.contains("isn't defined anymore"), "{headline}");
        assert_eq!(
            next,
            ["lnk agent harness-permission remove tool example --harness link"]
        );
    }

    /// A team's memory searches all its shared folder: only a member
    /// granted all of it, read or written, is handed it.
    #[test]
    fn team_memory_only_for_the_whole_folder() {
        let folders = |write: &[&str], read: &[&str]| link_harness_contract::harness::TeamFolders {
            write: write.iter().map(|s| s.to_string()).collect(),
            read: read.iter().map(|s| s.to_string()).collect(),
        };
        assert!(folders(&["."], &[]).whole());
        assert!(folders(&["drafts"], &["."]).whole());
        assert!(!folders(&["drafts"], &["notes"]).whole());
        assert!(!folders(&[], &[]).whole());
    }
}

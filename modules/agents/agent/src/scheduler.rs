//! The agent's scheduler (Link scheduler, `lnk-scheduler-tool`): its
//! subagents and schedules, for a harness that reads who a message is
//! from (`Contract::from`).
//!
//! Two halves. The tool (`scheduler-<agent>`, defined at a start like
//! memory's, `lnk sandbox tool add`), which the harness reaches through
//! its namespaces (`task`, `cron` and `agent`), confined to writing its store
//! ([`state`]). And the runner, which fires each job as it falls due
//! through the bridge's way in: run by the agent's bridge, a background
//! service restarting with the machine ([`crate::bridge::run`]). Their
//! caps are the agent's spec's (`[tool.scheduler]`).

use link_format::WithinExt;
use link_scheduler_tool_contract::cli::Flags;
use link_scheduler_tool_contract::cli::Runner;
use std::path::PathBuf;

use anyhow::bail;
use link_agent::settings::{self, Settings};

/// The scheduler's module.
pub const MODULE: &str = "scheduler-tool";

/// The tool the scheduler of the agent keyed `key`
/// ([`Settings::key`]) is served as.
pub use link_scheduler_tool_contract::tool_of;

/// Where its jobs are kept on `file`, and the rest of what it keeps
/// (`~/Link/Agents/<name>/agent/schedule`): the agent's `schedule`
/// collection, in its own folder, which no harness's sandbox reaches.
pub fn state(agent: &Settings) -> PathBuf {
    agent
        .folder()
        .of(link_store_contract::storage::Collection::Schedule)
}

/// Its record, beside the agent's settings: there while it's defined.
fn record(agent: &Settings) -> PathBuf {
    settings::config_of(&agent.name).join("scheduler.json")
}

/// Its tool, when it's defined for the agent.
pub fn tool(agent: &Settings) -> Option<String> {
    record(agent).exists().then(|| tool_of(&agent.key()))
}

/// Its caps, from the agent's spec, as its program takes them
/// ([`link_scheduler_tool_contract::caps_of`]).
pub fn caps(agent: &Settings) -> anyhow::Result<Flags> {
    link_scheduler_tool_contract::caps(&agent.spec()?.tool("scheduler"))
}

/// Its teams, as its program takes them: the agent's ID, each team it's
/// on, and each other member (`--agent-id`, `--team-id`, `--teammate`),
/// whom `send` may reach; none for an agent on no team
/// ([`link_scheduler_tool_contract::teams_of`]).
pub fn teams(agent: &Settings) -> anyhow::Result<Flags> {
    let Some(id) = agent.agent_id() else {
        return Ok(Flags::default());
    };
    Ok(link_scheduler_tool_contract::teams(
        id,
        &settings::teams_of(id)?,
    ))
}

/// Its connections (`--connection`), where a schedule's answer may go
/// besides the chat it's made in: each connected (`lnk agent connect`).
pub fn connections(agent: &Settings) -> Flags {
    Flags {
        connection: agent.connections(),
        ..Flags::default()
    }
}

/// Its floor (`--floor`), the least standing anyone reaches the agent
/// with, from its spec and its teams
/// ([`link_scheduler_tool_contract::floor_of`]).
pub fn floor(agent: &Settings) -> anyhow::Result<Flags> {
    let teams = match agent.agent_id() {
        Some(id) => settings::teams_of(id)?,
        None => Vec::new(),
    };
    Ok(link_scheduler_tool_contract::floor_of(
        &agent.spec()?.spec,
        agent.agent_id(),
        &teams,
    ))
}

/// The runner's team relay (`--team-relay`, `--team-owner`): for an
/// agent on a team that an account hosting agents carries through its
/// team relay, as that account's `agent-team list` says, else as a move
/// here brought word of it ([`link_agent::hosted_teams::listed`]),
/// so a teammate off this machine is reached through it
/// ([`link_scheduler_tool_contract::relay_of`]). None for an agent on no
/// team, and none, said in the bridge's log, when no account answers.
fn relay(agent: &Settings) -> Flags {
    let Some(id) = agent.agent_id() else {
        return Flags::default();
    };
    let teams = settings::teams_of(id).unwrap_or_default();
    if teams.is_empty() {
        return Flags::default();
    }
    let relayed: Vec<_> = link_agent::hosted_teams::listed()
        .into_iter()
        .filter_map(|team| {
            Some(link_scheduler_tool_contract::RelayedTeam {
                id: team.id,
                owner: team.owner?,
                relay: team.relay?,
            })
        })
        .collect();
    link_scheduler_tool_contract::relay(id, &teams, &relayed)
}

/// Each teammate's way in on this machine, by its agent's ID
/// (`--way-in-of <id>=<socket>`): a teammate not here is reached by none.
fn ways_in(agent: &Settings) -> Flags {
    let Some(id) = agent.agent_id() else {
        return Flags::default();
    };
    let mates: Vec<String> = settings::teams_of(id)
        .unwrap_or_default()
        .iter()
        .flat_map(|team| team.members.values().map(|m| m.id.clone()))
        .filter(|mate| mate != id)
        .collect();
    let mut flags = Flags::default();
    for name in settings::names().unwrap_or_default() {
        let key = settings::key_of(&name);
        if mates.contains(&key) && !flags.way_in_of.iter().any(|a| a.starts_with(&key)) {
            flags.way_in_of.push(format!(
                "{key}={}",
                link_connection_contract::way_in(&name).display()
            ));
        }
    }
    flags
}

/// The scheduler's program, its module installed first if it's missing
/// (`lnk module add`); for a `lnk` built from source, only one built
/// beside it, never a release's of another version.
pub fn program() -> anyhow::Result<PathBuf> {
    if let Some(program) = link_core::find_module(MODULE) {
        return Ok(program);
    }
    if !link_core::RELEASED {
        bail!(
            "lnk-{MODULE} isn't beside lnk: this lnk is built from source, so build Link's tools \
             too (docs/more/developing/install.md in its source)"
        );
    }
    link_core::modules::add(MODULE, "Starting your agent")
}

/// Defines the agent's scheduler as its tool, for a harness that reads
/// who a message is from (`from`) unless its spec turns it off
/// (`[tool.scheduler] enabled = false`); forgets it, its tool too,
/// otherwise. The tool's name, or none. A spec whose driver is
/// `address` is refused, and its scheduler forgotten: this module gives
/// the scheduler no address, so it would never run its jobs.
pub fn define(agent: &Settings, from: bool) -> anyhow::Result<Option<String>> {
    link_agent::schedules::recover(agent)?;
    link_agent::schedules::publish(agent)?;
    let record = record(agent);
    let tool = tool_of(&agent.key());
    let scheduler = agent.spec()?.tool("scheduler");
    // Off unless its spec turns it on: Link's does.
    let enabled = scheduler.enabled == Some(true);
    let address = scheduler.driver == Some(link_agent_spec::SchedulerDriver::Address);
    if !from || !enabled || address {
        if std::fs::remove_file(&record).is_ok() {
            let _ = link_core::lnk()
                .args(["sandbox", "tool", "remove", &tool])
                .output();
            crate::confine::forget_defined_tools();
        }
        anyhow::ensure!(
            !(from && enabled && address),
            "its spec's [tool.scheduler] driver is \"address\", for a machine that gives the \
             scheduler an address to say when its jobs are due, which this one doesn't: \
             \"service\" or \"cron\" here"
        );
        return Ok(None);
    }
    let caps = caps(agent)?;
    zone_known(agent)?;
    let program = program()?;
    let state = state(agent);
    // Whether starting a subagent, setting a schedule and cancelling one
    // ask you first: the agent's spec's (`[tool.scheduler] ask`).
    let scheduler = agent.spec()?.tool("scheduler");
    let ask = scheduler.ask.unwrap_or(false);
    let argv = Flags {
        state: Some(state.clone()),
        // As its runner fires: this agent, so a firing, its retries and
        // the checks on its work keep one identity, on a team or not.
        r#as: Some(agent.key()),
        ..caps
    }
    .with(teams(agent)?)
    .with(floor(agent)?)
    .with(connections(agent))
    .serve(&program);
    // A recurring schedule asks you first unless its call says once:
    // each argument the tool declares that asks, but none of the
    // recurring argument where the agent's spec lets those through
    // (`[tool.scheduler] recurring = "allow"`).
    let ask_if = link_scheduler_tool_contract::ask_if(&scheduler)?;
    let defined = crate::defined::Tool {
        sandbox: Some(agent.sandbox),
        ask: Some(ask),
        ask_if,
        ..crate::defined::Tool::new(argv.clone(), &program)
    };
    if state.is_dir()
        && crate::defined::kept(&record).as_ref() == Some(&defined)
        && crate::confine::defined_tools().is_ok_and(|tools| tools.contains(&tool))
    {
        return Ok(Some(tool));
    }
    let _ = std::fs::remove_file(&record);
    link_files::create_private_dir(&state).within(format!("creating {}", state.display()))?;
    let mut add: Vec<String> = vec!["sandbox".into(), "tool".into(), "add".into(), tool.clone()];
    // `list` only reads; the rest ask you first unless allowed, and a
    // recurring schedule unless let through, as the agent's spec says.
    add.extend(link_scheduler_tool_contract::sandbox_flags(&scheduler)?);
    add.push("--".into());
    match agent.sandbox {
        true => {
            add.push(
                link_core::lnk()
                    .get_program()
                    .to_string_lossy()
                    .into_owned(),
            );
            add.extend(crate::memory::confined(&program, &state, &[], &[], &argv));
        }
        false => add.extend(argv),
    }
    let output = link_core::lnk()
        .args(&add)
        .current_dir(&state)
        .output()
        .within("running lnk sandbox tool add")?;
    crate::confine::forget_defined_tools();
    if !output.status.success() {
        bail!(
            "lnk sandbox tool add {tool} failed: {}",
            link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
        );
    }
    crate::defined::keep(&record, &defined)?;
    Ok(Some(tool))
}

/// Refuses a `[tool.scheduler] zone` this machine's zone database (on
/// Linux or a Mac) lacks, as the scheduler would at each call: an agent
/// moved from a machine that has it.
fn zone_known(agent: &Settings) -> anyhow::Result<()> {
    let zone = agent.spec()?.tool("scheduler").zone.unwrap_or_default();
    let databases = ["/usr/share/zoneinfo", "/var/db/timezone/zoneinfo"].map(std::path::Path::new);
    if zone == "local" || databases.iter().any(|dir| dir.join(&zone).is_file()) {
        return Ok(());
    }
    bail!(
        "its spec's [tool.scheduler] zone {:?} isn't a time zone this machine knows",
        link_format::clean(&zone)
    )
}

/// Whether the machine's calendar wakes the agent's runner, once a
/// minute, rather than its bridge running it for good
/// (`[tool.scheduler] driver = "cron"`).
pub fn by_calendar(agent: &Settings) -> bool {
    agent.spec().is_ok_and(|spec| {
        spec.tool("scheduler").driver == Some(link_agent_spec::SchedulerDriver::Cron)
    })
}

/// The runner the agent's bridge runs: none when the calendar wakes it
/// ([`by_calendar`], [`calendar`]).
pub fn bridge_runner(agent: &Settings) -> Option<Runner> {
    match by_calendar(agent) {
        true => None,
        false => runner(agent),
    }
}

/// Has the machine's calendar wake the agent's runner once a minute
/// (`lnk-scheduler-tool cron install`, labelled by the agent's ID) when
/// its spec says `cron` and it has a scheduler, and keeps how to take it
/// away for `lnk agent stop` (`service::remove_calendar`); else takes
/// away one a start before made.
pub fn calendar(agent: &Settings) -> anyhow::Result<()> {
    let runner = match by_calendar(agent) {
        true => runner(agent),
        false => None,
    };
    let Some(runner) = runner else {
        crate::service::remove_calendar(&agent.name);
        return Ok(());
    };
    let (install, remove) = calendar_argvs(&runner, &agent.key());
    let output = link_secretive::process::command(&install[0])
        .args(&install[1..])
        .output()
        .within("running lnk-scheduler-tool cron install")?;
    if !output.status.success() {
        bail!(
            "{}",
            link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
        );
    }
    crate::service::keep_calendar(&agent.name, remove)
}

/// From the agent's `runner`, what installs the calendar entry starting
/// it, labelled `label`, and what removes it.
fn calendar_argvs(runner: &Runner, label: &str) -> (Vec<String>, Vec<String>) {
    let program = runner.program.display().to_string();
    let install = link_scheduler_tool_contract::cli::CronInstallArgs {
        label: label.into(),
        flags: runner.flags.clone(),
    };
    let remove = link_scheduler_tool_contract::cli::CronUninstallArgs {
        label: label.into(),
    };
    (
        [vec![program.clone()], install.argv()].concat(),
        [vec![program], remove.argv()].concat(),
    )
}

/// `lnk-scheduler-tool run`: the runner, firing through the agent's
/// bridge's way in, given the caps and `[tool.scheduler] firing`; none
/// when the agent has no scheduler, and why in the bridge's log when it
/// should have one.
pub fn runner(agent: &Settings) -> Option<Runner> {
    tool(agent)?;
    let Some(program) = link_core::find_module(MODULE) else {
        link_format::daemon::warn!("not running the scheduler: lnk-{MODULE} isn't installed");
        return None;
    };
    let own = Flags {
        state: Some(state(agent)),
        definitions: Some(state(agent).join(link_agent::schedules::DEFINITIONS)),
        way_in: Some(link_connection_contract::way_in(&agent.name)),
        // It fires as this agent alone, proved with its key.
        r#as: Some(agent.key()),
        ..Flags::default()
    }
    // How long it waits on a firing: the runner's alone.
    .with(link_scheduler_tool_contract::timeout(&agent.spec().ok()?.tool("scheduler")).ok()??);
    let flags = match caps(agent).and_then(|caps| Ok(own.with(caps).with(teams(agent)?))) {
        Ok(flags) => flags,
        Err(err) => {
            link_format::daemon::warn!("not running the scheduler: {err:#}");
            return None;
        }
    };
    // A teammate on none of those ways in, through its team's relay.
    Some(Runner {
        program,
        flags: flags.with(ways_in(agent)).with(relay(agent)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The calendar starts the runner with the bridge's flags, by the
    /// agent's label; removing it needs only the label.
    #[test]
    fn the_calendar_starts_the_runner_with_its_flags() {
        let runner = Runner {
            program: "/bin/lnk-scheduler-tool".into(),
            flags: Flags {
                state: Some("/s".into()),
                hops: Some(8),
                ..Flags::default()
            },
        };
        let (install, remove) = calendar_argvs(&runner, "a1b2c3");
        assert_eq!(
            install,
            [
                "/bin/lnk-scheduler-tool",
                "cron",
                "install",
                "--hops",
                "8",
                "--label",
                "a1b2c3",
                "--state",
                "/s"
            ]
        );
        assert_eq!(
            remove,
            [
                "/bin/lnk-scheduler-tool",
                "cron",
                "uninstall",
                "--label",
                "a1b2c3"
            ]
        );
    }
}

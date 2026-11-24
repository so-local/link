//! The agent's background services: the bridge, a service with no
//! sandbox, the kill switch, and what the measure module measured.

use link_format::WithinExt;
use std::path::Path;

use anyhow::{bail, ensure};
use link_agent::cli::{self};
use link_agent::{
    service::{self, Job},
    settings::{self, Settings},
};

/// `lnk agent unsandboxed --machine <file> --settings <file> --service
/// <file> -- <command>`: what a service runs a harness with no sandbox
/// through, so `unsandboxed = false` set after the service was installed
/// holds at its next restart or boot, as it does at a start, and so does
/// the agent's own setting: runs `command`, unless the machine forbids
/// it, its `agents.toml` can't be read, or the agent's settings don't
/// say `sandbox = false`; then removes the service, said in its
/// log. Its paths are given, since it runs with the harness's home as
/// `HOME`.
pub(crate) fn unsandboxed(guard: &cli::AgentUnsandboxedArgs) -> anyhow::Result<()> {
    let command = &guard.command;
    let why = match link_agent::machine_settings::check_unsandboxed_at(&guard.machine_file)
        .and_then(|()| sandbox_off(&guard.agent_settings))
    {
        Ok(()) => {
            let err = std::os::unix::process::CommandExt::exec(
                link_secretive::process::command(&command[0]).args(&command[1..]),
            );
            return Err(err).within(format!("running {}", command[0]));
        }
        Err(err) => format!("{err:#}"),
    };
    link_service::stop_from_within(&guard.service_file);
    bail!(
        "your agent didn't start with no sandbox: {why}. Its service is removed; lnk agent \
         start starts it in its sandbox, and lnk agent start --no-sandbox without."
    )
}

/// An error unless the agent's settings at `settings` say `sandbox =
/// false`, the one setting that runs it with none.
pub(crate) fn sandbox_off(settings: &Path) -> anyhow::Result<()> {
    let text =
        std::fs::read_to_string(settings).within(format!("reading {}", settings.display()))?;
    let agent: Settings =
        link_files::saved::parse(&text).within(format!("reading {}", settings.display()))?;
    ensure!(
        !agent.sandbox,
        "its settings don't turn its sandbox off (lnk agent start --no-sandbox)"
    );
    Ok(())
}

/// The kill switch, `lnk sandbox stop`, stops agent `agent` too, however
/// it runs (`lnk sandbox on-stop`: on the network as it is it has no wire
/// to close);
/// with `on` false, no longer. A failure is said, never fatal: its wire
/// closes regardless.
pub(crate) fn on_stop(agent: &str, on: bool) {
    let key = format!("agent-{agent}");
    let mut command = link_core::lnk();
    command.args(["sandbox", "on-stop", &key]);
    if on {
        command.args(["--", "agent", "stop", "-a", agent]);
    }
    let done = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !done && on {
        link_format::aside(
            link_format::Report::warning(link_format::sentence!(
                "{} won't stop {}.",
                link_format::cmd!("lnk sandbox stop"),
                link_format::Name::new("agent", agent)
            ))
            .why(link_format::prose!(
                "{} failed.",
                link_format::cmd!("lnk sandbox on-stop")
            ))
            .next(link_format::cmd!("lnk agent stop"), "stop it"),
        );
    }
}

/// Starts the measure module's watcher (`lnk measure start`), if that
/// module is here, so a background agent is measured while it runs.
/// Quietly: an agent starts either way.
pub(crate) fn watch_measures() {
    if !link_core::module_present("measure") {
        return;
    }
    let _ = link_core::lnk()
        .args(["measure", "start", "--quiet"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Each agent here's memory in use now and what its model cost over the
/// last day, by its ID, as the measure module's watcher last saw them
/// (`lnk measure --json --last`); none without a watcher running, or
/// without the measure module.
pub(crate) fn measured() -> std::collections::HashMap<String, (Option<f64>, Option<f64>)> {
    if !link_core::module_present("measure") {
        return std::collections::HashMap::new();
    }
    let last = link_core::lnk()
        .args(["measure", "--json", "--last"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .unwrap_or_default();
    let mut found = std::collections::HashMap::new();
    for unit in last["units"].as_array().into_iter().flatten() {
        if unit["kind"] != "agent" {
            continue;
        }
        let measures = &unit["measures"];
        found.insert(
            unit["id"].as_str().unwrap_or_default().to_string(),
            (
                measures["link.memory.used.sum"]["now"].as_f64(),
                measures["link.cost.sum"]["day"].as_f64(),
            ),
        );
    }
    found
}

/// Whether variable `name` goes from here to the bridge service: a
/// connection module's address or program to use instead of its own
/// (`LNK_<NAME>_API` or `_CLI`, for tests). Never a secret
/// (a model key, a token): the service's definition is readable.
pub(crate) fn passed_to_bridge(name: &str) -> bool {
    name.strip_prefix("LNK_").is_some_and(|rest| {
        ["_API", "_CLI"]
            .iter()
            .any(|suffix| rest.len() > suffix.len() && rest.ends_with(suffix))
    }) && !["KEY", "TOKEN", "SECRET", "PASSWORD", "AUTH"]
        .iter()
        .any(|secret| name.contains(secret))
}

/// `argv`, run with no sandbox as agent `agent`'s service, through `lnk
/// agent unsandboxed` ([`unsandboxed`]).
pub(crate) fn unsandboxed_argv(agent: &str, argv: Vec<String>) -> anyhow::Result<Vec<String>> {
    let me = std::env::current_exe().within("finding lnk-agent")?;
    let mut guarded = vec![
        me.display().to_string(),
        "unsandboxed".into(),
        "--machine-file".into(),
        link_files::layout::Hidden::AgentPolicy
            .try_path()?
            .display()
            .to_string(),
        "--agent-settings".into(),
        Settings::path_of(agent).display().to_string(),
        "--service-file".into(),
        service::file(agent).display().to_string(),
        "--".into(),
    ];
    guarded.extend(argv);
    Ok(guarded)
}

/// The background service carrying agent `agent`'s connections and running
/// its scheduler: `lnk agent bridge`, with its settings' key and tokens
/// read there, never in the service's definition.
pub(crate) fn bridge_job(agent: &str) -> anyhow::Result<Job> {
    let me = std::env::current_exe().within("finding lnk-agent")?;
    let mut env = std::collections::BTreeMap::from([
        (
            "HOME".to_string(),
            link_files::home_dir().display().to_string(),
        ),
        ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
        ("LANG".into(), "en_US.UTF-8".into()),
    ]);
    // Where Link's settings are, and the connections' own test addresses and
    // programs, whichever connections there are.
    if let Ok(value) = std::env::var("XDG_CONFIG_HOME") {
        env.insert("XDG_CONFIG_HOME".into(), value);
    }
    env.extend(link_secretive::process::variables(passed_to_bridge));
    let dir = settings::config_of(agent);
    Ok(Job {
        argv: vec![
            me.display().to_string(),
            "-a".into(),
            agent.into(),
            "bridge".into(),
        ],
        env,
        workdir: dir.clone(),
        log: dir.join("bridge.log"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_connections_test_addresses_reach_the_bridge_service() {
        for passed in [
            "LNK_TELEGRAM_API",
            "LNK_SLACK_API",
            "LNK_DISCORD_API",
            "LNK_SIGNAL_CLI",
            "LNK_MATRIX_CLI",
            "LNK_MATRIX_API",
        ] {
            assert!(passed_to_bridge(passed), "{passed}");
        }
        for kept in [
            "ANTHROPIC_API_KEY",
            "LNK_TELEGRAM_TOKEN",
            "LNK_API",
            "LNK_TOKEN_API",
            "LNK_EXE",
            "TELEGRAM_API",
            "LNK_SLACK_BOT_TOKEN",
            "LNK_MATRIX_ENGINE",
        ] {
            assert!(!passed_to_bridge(kept), "{kept}");
        }
    }
}

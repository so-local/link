//! An agent's background commands (Link run's `start`, `lnk-run-tool`):
//! where their records are, and what the bridge, a stop, a move and a
//! delete do with them. Each runs under a supervisor of its own, up only
//! while it runs; this only finds them, ends them and sweeps them.

use std::path::PathBuf;

use crate::settings::{self, Settings};

/// Link run's module.
const RUN: &str = "run-tool";

/// Where `run` keeps its background commands' records
/// (`~/.lnk/data/agents/<name>/run`): runtime, never the agent's state,
/// in its folder there, which no harness reaches.
pub fn state(agent: &Settings) -> PathBuf {
    agent.data(link_files::layout::AgentData::Background)
}

/// `run`'s sweep, as the agent's bridge runs it once as it starts: its
/// background commands lost while it was down found, and each one's end
/// told. None when it has no `run`.
pub fn sweeper(agent: &Settings) -> Option<Vec<String>> {
    // Only while its `run` is defined.
    settings::config_of(&agent.name)
        .join("run.json")
        .exists()
        .then_some(())?;
    let program = link_core::find_module(RUN)?;
    let sweep = link_run_tool_contract::cli::RunToolSweepArgs {
        state: state(agent),
    }
    .argv();
    Some([vec![program.display().to_string()], sweep].concat())
}

/// Stops the agent's background commands still running (`lnk-run-tool
/// stop`), each lost and told when it's back; with `remove`, removes
/// them all, their output too (`remove`: it's deleted or moves away).
/// Each that ran or wasn't told yet, in words.
pub fn stop_background(agent: &Settings, forget: bool) -> Vec<String> {
    let state = state(agent);
    if !state.exists() {
        return Vec::new();
    }
    let Some(program) = link_core::find_module(RUN) else {
        return Vec::new();
    };
    let line = match forget {
        true => link_run_tool_contract::cli::RunToolDeleteArgs {
            state: state.clone(),
        }
        .argv(),
        false => link_run_tool_contract::cli::RunToolStopArgs {
            state: state.clone(),
        }
        .argv(),
    };
    let output = link_secretive::process::ours(program)
        .args(line)
        .stdin(std::process::Stdio::null())
        .output();
    match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| link_format::clean(line).to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

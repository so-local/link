//! Agent scheduler launch settings and caps.

/// The calls a model makes of the tool, as its declarations render them:
/// the template its program fills ([`link_actions::tools::offer`]).
pub const TOOLS: &str = include_str!(concat!(env!("OUT_DIR"), "/tools.json"));

/// Every call the tool declares, by the name a model calls it.
pub fn calls() -> anyhow::Result<Vec<String>> {
    Ok(link_actions::tools::Template::parse(TOOLS)?.calls())
}

/// Each call's argument that asks you first unless it's given false, as
/// `<call>.<argument>` (`schedule.recurring`): what the sandbox asks by
/// (`lnk sandbox tool add --ask-if`).
pub fn asks() -> anyhow::Result<Vec<String>> {
    Ok(link_actions::tools::Template::parse(TOOLS)?.asks())
}

/// The calls the tool declares as only reading.
pub fn reads() -> anyhow::Result<Vec<String>> {
    Ok(link_actions::tools::Template::parse(TOOLS)?.reads())
}

pub use link_agent_spec as agent_spec;
pub use link_core::*;
mod checks;
pub mod cli;
pub mod compaction;
pub mod cron;
pub mod definitions;
pub mod listed;
mod scheduler;
pub mod timing;
pub use scheduler::*;

/// `run`'s rule across its flags: many agents (`--agents`) fire in one
/// time zone, so `--time-zone` comes with them. The check hook `agents`
/// of the tool's declarations, for the arguments a caller builds and
/// those the tool reads.
pub fn agents(many: bool, zone: bool) -> Result<(), String> {
    match many && !zone {
        true => Err("--agents needs --time-zone".into()),
        false => Ok(()),
    }
}

#[cfg(test)]
link_test_kit::program!(unit);

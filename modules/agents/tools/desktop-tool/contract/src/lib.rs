//! What Link desktop, `lnk-desktop-tool`, takes: its command line ([`cli`]),
//! which the agent module builds from these types and the tool reads
//! back through them.

/// The calls a model makes of the tool, as its declarations render them:
/// the template its program fills ([`link_actions::tools::offer`]).
pub const TOOLS: &str = include_str!(concat!(env!("OUT_DIR"), "/tools.json"));

/// Every call the tool declares, by the name a model calls it.
pub fn calls() -> anyhow::Result<Vec<String>> {
    Ok(link_actions::tools::Template::parse(TOOLS)?.calls())
}

pub mod cli;

#[cfg(test)]
link_test_kit::program!(unit);

//! Connection settings, pacing and tool-header validation.

pub use link_core::*;
/// The shapes this contract declares in its `types.toml`.
pub mod declared {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}

/// A connection's opinions, how it shows tool calls, and which group
/// messages it answers, declared in the contract's `types.toml`.
pub use declared::{Activity, Connection, Groups};

impl Activity {
    /// Whether its calls show at all.
    pub fn shows(self) -> bool {
        self != Self::Off
    }
}

/// Between a tool's server's name and its own, in the name the model
/// calls (`file__read`).
pub const TOOL_SEPARATOR: &str = link_spec::follow::TOOL_SEPARATOR;

/// A tool call's name as a person reads it, its server's name and its
/// own (`file__read` is `file read`).
pub fn call_words(name: &str) -> String {
    match name.split_once(TOOL_SEPARATOR) {
        Some((server, own)) => format!("{server} {own}"),
        None => name.to_string(),
    }
}

/// The most characters of a tool call's line's mark or separator.
pub const ACTIVITY_MARK_MAX: usize = 8;

/// `text` as milliseconds (`"10m"`): every spec's durations are read alike.
pub use link_spec::duration_ms;

/// The most tools a [`crate::acp::Said::tools`] names.
pub const TOOLS_MAX: usize = 256;

/// Whether `name` is a tool's name a [`crate::acp::Said::tools`] may hold: 1 to 128
/// letters, digits, `_`, `-` and `.`.
pub fn is_tool_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}

/// `tools` as a [`crate::acp::Said::tools`]: an error for a name it may not hold,
/// or more than [`TOOLS_MAX`].
pub fn tools_header(tools: &[String]) -> anyhow::Result<String> {
    anyhow::ensure!(
        tools.len() <= TOOLS_MAX,
        "a conversation is given at most {TOOLS_MAX} tools"
    );
    if let Some(bad) = tools.iter().find(|t| !is_tool_name(t)) {
        anyhow::bail!("{:?} isn't a tool's name", link_format::clean(bad));
    }
    Ok(tools.join(","))
}

/// The tools a [`crate::acp::Said::tools`] names, each once, in its order: none for
/// an empty one, and nothing at all for one that isn't a list of tools'
/// names, or names more than [`TOOLS_MAX`].
pub fn tools_of(header: &str) -> Option<Vec<String>> {
    let mut tools: Vec<String> = Vec::new();
    for name in header.split(',').map(str::trim) {
        if name.is_empty() && !header.contains(|c: char| !c.is_whitespace()) {
            continue;
        }
        if !is_tool_name(name) {
            return None;
        }
        if !tools.iter().any(|t| t == name) {
            tools.push(name.to_string());
        }
    }
    (tools.len() <= TOOLS_MAX).then_some(tools)
}

#[cfg(test)]
link_test_kit::program!(unit);

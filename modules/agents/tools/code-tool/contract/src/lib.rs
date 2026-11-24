//! What Link code, `lnk-code-tool`, takes: its command line ([`cli`]),
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

/// The programs Link code names, as `programs.toml` sets them.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Programs {
    /// The program that sets a language server's variables inside its
    /// sandbox.
    env: String,
}

/// [`Programs`], read once.
fn programs() -> &'static Programs {
    static PROGRAMS: std::sync::LazyLock<Programs> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("programs.toml")).expect("Link code's programs.toml")
    });
    &PROGRAMS
}

/// The program that sets a language server's variables inside its
/// sandbox, which clears the environment (`serve`'s `--var`).
pub fn env_program() -> &'static str {
    &programs().env
}

#[cfg(test)]
link_test_kit::program!(unit);

#[cfg(test)]
mod tests {
    /// `programs.toml` reads, each program by its absolute path: a
    /// sandbox's commands allow a program by its path, never by a name
    /// looked up on its `PATH`.
    #[test]
    fn the_programs_read() {
        assert!(std::path::Path::new(super::env_program()).is_absolute());
    }
}

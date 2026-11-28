//! The core's own commands, `lnk up`, `module`, `location`, `upgrade`,
//! `uninstall`, `issue` and the rest, generated from its declarations
//! (`actions.toml` and `types.toml`). Every other `lnk <group>` is a
//! module's, forwarded unparsed (`Command::External`).

use link_core::help::Help;

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// The variables core defines (`variables.toml`), its own commands' inputs.
pub const VARIABLES: &str = include_str!("../variables.toml");

/// Where its help is, in the workspace's the component's command declarations.
pub const HELP: Help = Help {
    toml: include_str!("../actions.toml"),
    own: Some("lnk"),
    under: "",
};

/// The core's own commands, as clap sees them.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module's command is forwarded whole, and `--json` before the
    /// core's own words is theirs.
    #[test]
    fn the_core_owns_its_inputs_and_forwards_the_modules_arguments() {
        let read = |argv: &[&str]| {
            <Cli as clap::Parser>::try_parse_from(argv.iter().copied()).map(|c| c.command)
        };
        let Ok(Command::External(arguments)) = read(&[
            "lnk",
            "agent",
            "start",
            "--model.thinking",
            "high",
            "--",
            "--json",
        ]) else {
            panic!("module invocation")
        };
        assert_eq!(
            arguments,
            ["agent", "start", "--model.thinking", "high", "--", "--json"]
                .map(std::ffi::OsString::from)
        );
        let Ok(Command::ModuleList(list)) = read(&["lnk", "--json", "module", "list"]) else {
            panic!("lnk --json module list")
        };
        assert!(list.json);
        assert!(matches!(
            read(&["lnk", "module"]),
            Ok(Command::ModuleList(_))
        ));
        assert!(matches!(
            read(&["lnk", "location"]),
            Ok(Command::LocationList(_))
        ));
    }
}

//! `lnk-desktop-tool`'s command line and its outputs, generated from the
//! module's declarations (`actions.toml` and `types.toml`). Link runs
//! every command, never a person.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk-desktop-tool`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

//! `lnk cloud`'s command line and its outputs, generated from the
//! module's declarations (`actions.toml` and `types.toml`).

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line: its command and its arguments.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk cloud`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

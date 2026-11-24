//! `lnk archive`'s command line and its outputs, generated from the
//! module's declarations (`actions.toml` and `types.toml`).

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> Command {
    PROGRAM.parse::<Cli>().command.command
}

/// `lnk archive`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

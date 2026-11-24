//! `lnk mcp`'s command line and its output, generated from the module's
//! declarations (`actions.toml` and `types.toml`).

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> Command {
    PROGRAM.parse::<Cli>().command.command
}

/// `lnk mcp`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

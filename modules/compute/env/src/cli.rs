//! `lnk env`'s command line, generated from the module's declarations
//! (`actions.toml`), over the environment spec and outputs its contract
//! crate generates from `types.toml`.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

pub use link_env_contract::cli::{At, Slots, at};

/// Reads this process's command line: its command and the field flags of
/// the spec it binds.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk env`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

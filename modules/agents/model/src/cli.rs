//! `lnk model`'s command line, generated from the module's declarations
//! (`actions.toml`), over the outputs its contract generates from
//! `types.toml`.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

// The outputs, each the contract's one type.
pub use link_model_contract::declared::*;

/// Reads this process's command line.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk model`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

//! `lnk-document-converter`'s command line, generated from the module's
//! declarations over the shapes its contract generates. Link Memory runs
//! it, never a person.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// Its whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

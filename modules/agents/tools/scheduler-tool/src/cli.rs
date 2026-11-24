//! `lnk-scheduler-tool`'s command line, generated from the module's
//! declarations over the shapes, flag group and runtime arguments its
//! contract generates, so the flags Link builds and the ones the tool
//! reads are one type. Link runs every command, never a person.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk-scheduler-tool`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

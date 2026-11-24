//! `lnk box`'s command line and its outputs, generated from the module's
//! declarations (`actions.toml` and `types.toml`), its spec among them
//! ([`crate::spec::BoxSpec`]).

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line: its command and the field flags of
/// the spec it binds.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk box`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

//! What the fixture's tests share: its program, and its command line read
//! as `lnk` reads it.

use std::ffi::OsString;

use link_declarations_fixture::module;
use link_spec::{Binding, Program, history::History};

/// The example's program: its parser and its field flags' bindings, with
/// no help file and no history of its own.
pub struct Example;

impl Program for Example {
    fn before(&self, _: &[OsString]) {}
    fn styled(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn helped(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn history(&self) -> History {
        History::default()
    }
    fn binding(&self, words: &[String]) -> Option<Binding> {
        module::binding(words)
    }
    fn namespace(&self) -> Option<&str> {
        Some(module::NAMESPACE)
    }
}

/// `words` as a person types them after `lnk`, run as `lnk` runs them:
/// the program `lnk-env`, handed the words after its namespace.
pub fn parse(words: &[&str]) -> Result<link_spec::Parsed<module::Cli>, String> {
    let words = words.strip_prefix(&[module::NAMESPACE]).unwrap_or(words);
    let args = std::iter::once("lnk-env")
        .chain(words.iter().copied())
        .map(OsString::from);
    link_spec::try_run::<module::Cli>(&Example, args, |c| c).map_err(|refused| match refused {
        link_spec::entry::Refused::Usage(e) => e.to_string(),
        link_spec::entry::Refused::Retired(report) => format!("{report:?}"),
    })
}

pub fn refused(words: &[&str]) -> String {
    match parse(words) {
        Ok(_) => panic!("{words:?} parsed"),
        Err(e) => e,
    }
}

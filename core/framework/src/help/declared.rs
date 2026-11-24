//! A module whose command line is generated from its declarations, as
//! the entry runs it: its bindings, its namespace and its Next graph.

use std::ffi::OsString;

use super::{asks_nexts, styles};

/// A module whose command line is generated from its declarations
/// (`link-actions-build`): its parser carries its help, and `binding`
/// names the spec kind each command binds, whose fields are its field
/// flags.
pub struct Declared {
    pub binding: fn(&[String]) -> Option<link_spec::Binding>,
    /// The namespace `lnk` dispatches here, which the program never
    /// reads: its generated `NAMESPACE`.
    pub namespace: &'static str,
    /// Next's graph, its generated `NEXTS`: each command, the commands it
    /// leads to, and those a failure leads to.
    pub nexts: &'static [(
        &'static str,
        &'static [&'static str],
        &'static [&'static str],
    )],
}

impl Declared {
    /// This process's command line, read as `T` with its field flags.
    pub fn parse<T: clap::Parser>(&self) -> link_spec::Parsed<T> {
        link_spec::entry::current::<T>(self, |command| command)
    }
}

impl Declared {
    /// Its Next graph, by each command as typed.
    fn graph(&self) -> std::collections::BTreeMap<String, link_format::nexts::Declared> {
        let owned = |list: &[&str]| list.iter().map(|c| c.to_string()).collect();
        self.nexts
            .iter()
            .map(|(command, next, fail)| {
                (
                    command.to_string(),
                    link_format::nexts::Declared::new(owned(next), owned(fail)),
                )
            })
            .collect()
    }
}

impl link_spec::Program for Declared {
    /// The hidden `nexts --json`: its Next graph on stdout, and this
    /// program's end.
    fn before(&self, args: &[OsString]) {
        if asks_nexts(args) {
            let graph = self.graph();
            link_format::select_json(true);
            let _ = link_settings::output::run(|out| {
                out.reply(&graph);
                Ok(link_format::Done::shown())
            });
            std::process::exit(0)
        }
    }

    /// What the command `matches` names declares, for the check that a
    /// debug build gives no Next it doesn't.
    fn parsed(&self, matches: &clap::ArgMatches) {
        let mut words = vec![self.namespace.to_string()];
        let mut at = matches;
        while let Some((name, sub)) = at.subcommand() {
            words.push(name.to_string());
            at = sub;
        }
        // Each row is its command as typed, its program's word first.
        let typed = words.join(" ");
        if let Some((_, declared)) = self.graph().into_iter().find(|(command, _)| {
            command
                .split_once(' ')
                .is_some_and(|(_, rest)| rest == typed)
        }) {
            link_format::nexts::declare(declared);
        }
    }

    fn styled(&self, command: clap::Command) -> clap::Command {
        command.styles(styles())
    }

    fn helped(&self, command: clap::Command) -> clap::Command {
        command.styles(styles())
    }

    fn history(&self) -> link_spec::history::History {
        crate::descriptor::history()
    }

    fn release(&self) -> Option<fn() -> anyhow::Result<link_spec::history::Wire>> {
        Some(crate::descriptor::release)
    }

    fn binding(&self, words: &[String]) -> Option<link_spec::Binding> {
        (self.binding)(words)
    }

    fn namespace(&self) -> Option<&str> {
        Some(self.namespace)
    }
}

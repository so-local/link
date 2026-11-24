//! The one entry a program's command line goes through ([`run`]): what
//! Link answers before parsing, the field flags of the spec the command
//! binds, the runtime arguments clap reads, the program's retired
//! spellings, and the output the parsed command asks for, so no program
//! reads its arguments any other way.

use std::ffi::OsString;

use crate::history::{History, Surface};
use crate::{Binding, FieldFlags};

mod invocation;
pub use invocation::{
    Discovery, Replay, UPGRADE, current, discovery, initialize, replay_typed_without,
    replay_without, try_current,
};

/// A standalone utility with no component help, binding, or history.
/// It still uses the shared invocation, validation, and output selection.
pub fn plain<T: clap::Parser>() -> T {
    current::<T>(&Bare, |command| command).command
}

struct Bare;
impl Program for Bare {
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
}

/// What a program gives the entry beside its command type: the answers
/// Link gives before any parser, its help, its history and its bindings.
/// `link-core`'s help implements it for every module.
pub trait Program {
    /// Answer what's asked before parsing, such as discovery or a
    /// command's next steps, and exit when it's answered.
    fn before(&self, args: &[OsString]);

    /// `command` in the look a person reads it in.
    fn styled(&self, command: clap::Command) -> clap::Command;

    /// `command` with this program's help put in, shown where a command
    /// line is wrong or asks for help.
    fn helped(&self, command: clap::Command) -> clap::Command;

    /// The command line as the program's parser takes it, such as with a
    /// one-group program's group put in.
    fn arranged(&self, command: &clap::Command, args: Vec<OsString>) -> Vec<OsString> {
        let _ = command;
        args
    }

    /// Its retired commands and runtime flags.
    fn history(&self) -> History;

    /// How it asks for the release's history, which the names its specs
    /// and conversations hold follow ([`crate::follow`]): asked only when
    /// one is read. None reads every name as written.
    fn release(&self) -> Option<fn() -> anyhow::Result<crate::history::Wire>> {
        None
    }

    /// The namespace `lnk` dispatches to this program, which it never
    /// reads in its arguments: `env` for `lnk-env`. Its history names
    /// commands as a person types them, so the namespace is put back
    /// before a retired spelling is looked for.
    fn namespace(&self) -> Option<&str> {
        None
    }

    /// The spec kind the command at `words` binds, whose fields are its
    /// field flags.
    fn binding(&self, words: &[String]) -> Option<Binding> {
        let _ = words;
        None
    }

    /// What follows a parse, such as declaring the command's next steps.
    fn parsed(&self, matches: &clap::ArgMatches) {
        let _ = matches;
    }

    /// The retired variables of `history` set where it runs: those of this
    /// process, by their names alone.
    fn variables_set<'h>(&self, history: &'h History) -> Vec<&'h crate::history::Gone> {
        history.variables_set().collect()
    }
}

/// A command line read whole: the command, its words and the field flags
/// of the spec it binds.
pub struct Parsed<T> {
    pub command: T,
    /// The command's words, `box create`.
    pub words: Vec<String>,
    pub fields: FieldFlags,
    /// The validated, normalized invocation, retained for forwarding.
    pub arguments: Vec<OsString>,
    pub replay: Replay,
}

/// Read `args`, the program's name first, as `T`: answer what's asked
/// before parsing, take the bound spec's field flags, parse the runtime
/// arguments with `T`, answer a retired spelling with its replacement,
/// and choose the output the parsed command asks for. `adjust` changes
/// the command first, such as hiding an argument where it means nothing.
/// A command line that doesn't parse exits with clap's usage error.
pub fn run<T: clap::Parser>(
    program: &impl Program,
    args: impl IntoIterator<Item = OsString>,
    adjust: impl Fn(clap::Command) -> clap::Command,
) -> Parsed<T> {
    try_run(program, args, adjust).unwrap_or_else(|e| e.exit())
}

/// Why a command line was refused: clap's usage error, or a retired
/// spelling answered as a report with its replacement as the next step.
#[derive(Debug)]
pub enum Refused {
    Usage(clap::Error),
    Retired(Box<link_format::Report>),
}

impl Refused {
    /// Show it, as `--json` asks where the command line has it, and exit.
    pub fn exit(self) -> ! {
        match self {
            Refused::Usage(error) => error.exit(),
            Refused::Retired(report) => {
                let out = link_format::Out::with(link_format::Machine::default());
                std::process::exit(i32::from(out.finish(Err(*report))))
            }
        }
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Usage(error) => error.fmt(f),
            Refused::Retired(report) => f.write_str(&link_format::render::block(
                &link_format::Block::from((**report).clone()),
                &link_format::render::Ctx::plain(),
            )),
        }
    }
}

impl From<clap::Error> for Refused {
    fn from(error: clap::Error) -> Self {
        Refused::Usage(error)
    }
}

/// [`run`], with a command line that doesn't parse its refusal.
pub fn try_run<T: clap::Parser>(
    program: &impl Program,
    args: impl IntoIterator<Item = OsString>,
    adjust: impl Fn(clap::Command) -> clap::Command,
) -> Result<Parsed<T>, Refused> {
    let args: Vec<OsString> = args.into_iter().collect();
    if let Some(ask) = program.release() {
        crate::follow::provide(ask);
    }
    program.before(&args);
    // Its command tree, made once: every reading below is of this one.
    let command = T::command();
    let args = program.arranged(&command, args);
    let usage = |said: String| {
        adjust(program.helped(command.clone())).error(clap::error::ErrorKind::UnknownArgument, said)
    };
    // An old spelling is refused before anything parses it, so one clap
    // would still take never runs.
    let history = program.history();
    // The command line and its commands as a person types them, its
    // namespace first, for its history to read.
    let namespace = program.namespace();
    let with_namespace = |args: &[OsString]| -> Vec<OsString> {
        args.first()
            .cloned()
            .into_iter()
            .chain(namespace.map(OsString::from))
            .chain(args.iter().skip(1).cloned())
            .collect()
    };
    let typed = with_namespace(&args);
    // A history matches commands by their words: a global flag before
    // them is no part of the spelling it looks for.
    let words_typed = with_namespace(&without_leading_globals(&command, &args));
    let typed_surface: Surface = surface(&command)
        .into_iter()
        .map(|(path, flags)| match namespace {
            Some(namespace) => (format!("{namespace} {path}"), flags),
            None => (path, flags),
        })
        .collect();
    let given = typed.get(1..).unwrap_or_default();
    // A refusal answers as the command line asks, before anything parses.
    let asked_json = given
        .iter()
        .take_while(|a| *a != "--")
        .any(|a| a == "--json");
    let json = move || link_format::select_json(asked_json);
    if let Some(renamed) =
        history.find_renamed(words_typed.get(1..).unwrap_or_default(), &typed_surface)
    {
        json();
        return Err(Refused::Retired(Box::new(renamed_report(
            &history, renamed, given,
        ))));
    }
    if let Some(removed) =
        history.find_removed(words_typed.get(1..).unwrap_or_default(), &typed_surface)
    {
        json();
        return Err(Refused::Retired(Box::new(
            link_format::Report::error(link_format::Sentence::adopt(&removed.message()))
                .exit(link_format::Exit::Usage),
        )));
    }
    // A retired variable still set is said, never a refusal: what it set
    // is ignored, and the command runs.
    for warning in variable_warnings(program.variables_set(&history).into_iter()) {
        link_format::aside(warning);
    }
    if let Some(report) = retired_report(&history, &typed_surface, &words_typed) {
        json();
        return Err(Refused::Retired(Box::new(report)));
    }
    let arguments = args.iter().skip(1).cloned().collect();
    let words = leading_words(&command, &args);
    let binding = program.binding(&words);
    // Built once, for both the external command's boundary and the replay.
    let mut built = command.clone();
    built.build();
    let (fields, args) = if external_payload(&mut built, &args) {
        // The target program owns every word after its external command,
        // including dotted flags. Clap locates that boundary from this type.
        (FieldFlags::default(), args)
    } else {
        crate::fields::split_claimed(
            args,
            binding.as_ref(),
            trailing(&command, &words),
            &runtime_flags(&command, &words),
        )
        .map_err(
            |error| match error.downcast_ref::<crate::fields::RetiredFlag>() {
                Some(retired) => {
                    json();
                    let typed: Vec<String> = namespace
                        .map(str::to_string)
                        .into_iter()
                        .chain(words.clone())
                        .collect();
                    Refused::Retired(Box::new(retired_flag_report(retired, &typed)))
                }
                None => Refused::Usage(usage(format!("{error:#}"))),
            },
        )?
    };
    // A bound command's help lists its field flags after its own.
    let fielded = |command: clap::Command| match &binding {
        Some(binding) => bound_help(command, &words, &binding.schema),
        None => command,
    };
    let matches = adjust(fielded(program.styled(command.clone())))
        .try_get_matches_from(&args)
        .or_else(|_| adjust(fielded(program.helped(command.clone()))).try_get_matches_from(&args))
        .map_err(
            |e| match retired_report(&history, &typed_surface, &words_typed) {
                Some(report) => {
                    json();
                    Refused::Retired(Box::new(report))
                }
                None => Refused::Usage(e),
            },
        )?;
    link_format::select_json(asks_json(&matches));
    program.parsed(&matches);
    let words = words_of(&matches);
    let command = T::from_arg_matches(&matches)?;
    let replay = Replay::new(
        &built,
        namespace,
        &words,
        args.into_iter().skip(1).collect(),
        fields.args(),
    );
    Ok(Parsed {
        command,
        words,
        fields,
        arguments,
        replay,
    })
}

/// `command` with the field flags of `schema` in the help of its
/// subcommand at `words`.
fn bound_help(
    command: clap::Command,
    words: &[String],
    schema: &serde_json::Value,
) -> clap::Command {
    match words.split_first() {
        None => crate::fields::with_field_help(command, schema),
        Some((first, rest)) => {
            command.mut_subcommand(first.as_str(), |sub| bound_help(sub, rest, schema))
        }
    }
}

/// Whether the parsed command asks for JSON: `--json`, or
/// `--stream-json`'s JSON lines, set on it or on a command above it.
pub fn asks_json(matches: &clap::ArgMatches) -> bool {
    let mut at = Some(matches);
    while let Some(matches) = at {
        let set = |id: &str| matches.try_get_one::<bool>(id).ok().flatten() == Some(&true);
        if set("json") || set("stream-json") {
            return true;
        }
        at = matches.subcommand().map(|(_, sub)| sub);
    }
    false
}

/// An external subcommand is the next program's opaque command line.
fn external_payload(command: &mut clap::Command, args: &[OsString]) -> bool {
    let Ok(matches) = command.try_get_matches_from_mut(args) else {
        return false;
    };
    let mut at = &*command;
    let mut selected = &matches;
    while let Some((word, nested)) = selected.subcommand() {
        match at.find_subcommand(word) {
            Some(command) => {
                at = command;
                selected = nested;
            }
            None => return at.is_allow_external_subcommands_set(),
        }
    }
    false
}

/// Where the command at `words` starts the program it runs, where it
/// takes the rest of its line as its own: how many words name it with
/// the program's own, and its long flags that take a value.
fn trailing(command: &clap::Command, words: &[String]) -> Option<crate::Trailing> {
    let mut at = command;
    for word in words {
        at = at.find_subcommand(word)?;
    }
    // A `last` argument starts only after `--`, which the split already
    // keeps whole, so a word before it is another positional's.
    let takes = at.get_arguments().any(|a| a.is_trailing_var_arg_set());
    takes.then(|| crate::Trailing {
        words: words.len() + 1,
        valued: at
            .get_arguments()
            .filter(|a| a.get_action().takes_values())
            .filter_map(|a| a.get_long().map(str::to_string))
            .collect(),
    })
}

/// The long runtime flags of the command at `words`: its own, and those
/// of every command above it, which the shared flags are, with `help`
/// and `version` clap adds.
pub fn runtime_flags(command: &clap::Command, words: &[String]) -> Vec<String> {
    let mut flags: Vec<String> = ["help", "version"].map(str::to_string).into();
    let mut at = Some(command);
    let mut rest = words.iter();
    while let Some(here) = at {
        flags.extend(
            here.get_arguments()
                .filter_map(|a| a.get_long().map(str::to_string)),
        );
        at = rest.next().and_then(|word| here.find_subcommand(word));
    }
    flags
}

/// The words of the command `matches` parsed.
fn words_of(matches: &clap::ArgMatches) -> Vec<String> {
    let mut words = Vec::new();
    let mut at = matches;
    while let Some((name, sub)) = at.subcommand() {
        words.push(name.to_string());
        at = sub;
    }
    words
}

/// The words of the command `args` names: as far as they're `command`'s
/// subcommands, past any global flag given between them (`agent -a main
/// start`) and its value, up to the first other flag.
fn leading_words(command: &clap::Command, args: &[OsString]) -> Vec<String> {
    let mut words = Vec::new();
    let mut at = command;
    let mut globals: Vec<&clap::Arg> = Vec::new();
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        let Some(word) = arg.to_str() else { break };
        globals.extend(at.get_arguments().filter(|a| a.is_global_set()));
        if word.starts_with('-') {
            let Some(takes_next) = global_flag(&globals, word) else {
                break;
            };
            if takes_next {
                rest.next();
            }
            continue;
        }
        match at.find_subcommand(word) {
            Some(sub) => {
                words.push(word.to_string());
                at = sub;
            }
            None => break,
        }
    }
    words
}

/// `args` without the global flags, and their values, given before its
/// first word that isn't a subcommand: `lnk-agent -a main allow` is
/// `lnk-agent allow` to a history, which matches commands by their words.
fn without_leading_globals(command: &clap::Command, args: &[OsString]) -> Vec<OsString> {
    let mut kept: Vec<OsString> = args.iter().take(1).cloned().collect();
    let mut at = command;
    let mut globals: Vec<&clap::Arg> = Vec::new();
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        globals.extend(at.get_arguments().filter(|a| a.is_global_set()));
        let word = arg.to_str().unwrap_or_default();
        if let Some(takes_next) = global_flag(&globals, word) {
            if takes_next {
                rest.next();
            }
            continue;
        }
        kept.push(arg.clone());
        match at.find_subcommand(word) {
            Some(sub) => at = sub,
            None => break,
        }
    }
    kept.extend(rest.cloned());
    kept
}

/// Whether `word` is one of `globals`, and if so whether its value is the
/// next word. `--agent=main`, `-a=main` and `-amain` carry their value; a
/// short flag is its first letter.
fn global_flag(globals: &[&clap::Arg], word: &str) -> Option<bool> {
    let flag = word.strip_prefix('-')?;
    let (global, inline) = match flag.strip_prefix('-') {
        Some(long) => {
            let (name, inline) = match long.split_once('=') {
                Some((name, _)) => (name, true),
                None => (long, false),
            };
            (globals.iter().find(|a| a.get_long() == Some(name)), inline)
        }
        None => {
            let mut letters = flag.chars();
            let short = letters.next();
            let inline = letters.next().is_some();
            (
                globals
                    .iter()
                    .find(|a| short.is_some() && a.get_short() == short),
                inline,
            )
        }
    };
    global.map(|global| !inline && global.get_action().takes_values())
}

/// What `history` says of `args`, with the program's name first: the replacement of a command or runtime flag
/// it retired, or nothing. Only syntax is matched, never a value or
/// anything after `--`.
pub fn retired(history: &History, command: &clap::Command, args: &[OsString]) -> Option<String> {
    let (gone, _) = history.find(args.get(1..)?, &surface(command))?;
    Some(gone.message())
}

/// A warning for each retired variable that is set, by its name alone,
/// never its value.
fn variable_warnings<'a>(
    set: impl Iterator<Item = &'a crate::history::Gone>,
) -> Vec<link_format::Report> {
    set.map(|gone| link_format::Report::warning(link_format::Sentence::adopt(&gone.message())))
        .collect()
}

/// [`retired`] as the report a person reads: a usage error, with what
/// replaced it as the next step.
fn retired_report(
    history: &History,
    surface: &Surface,
    args: &[OsString],
) -> Option<link_format::Report> {
    let (gone, _) = history.find(args.get(1..)?, surface)?;
    let mut report = link_format::Report::error(link_format::Sentence::adopt(&gone.message()))
        .exit(link_format::Exit::Usage);
    if let Some(now) = &gone.now {
        // A declared replacement: its words fixed, a `<name>` word the
        // person's to fill.
        report = report.next(
            link_format::Command::fill(now, Vec::new()),
            "what replaced it",
        );
    }
    Some(report)
}

/// A field flag a schema step retired, as the report a person reads: a
/// usage error, with the command and the flag that replaces it as the
/// next step, where one does.
fn retired_flag_report(
    retired: &crate::fields::RetiredFlag,
    words: &[String],
) -> link_format::Report {
    let report = link_format::Report::error(link_format::Sentence::adopt(&retired.message))
        .exit(link_format::Exit::Usage);
    match &retired.now {
        Some(now) => report.next(
            link_format::Command::fill(
                &std::iter::once("lnk")
                    .chain(words.iter().map(String::as_str))
                    .chain([now.as_str(), "<value>"])
                    .collect::<Vec<_>>()
                    .join(" "),
                Vec::new(),
            ),
            "the flag that replaced it",
        ),
        None => report,
    }
}

/// A renamed module's old name, answered with the name it has today,
/// through every rename since, and the person's command line again with
/// that name in the old one's place (`lnk memory list` as `lnk notes
/// list`, `lnk module remove memory` as `lnk module remove notes`).
fn renamed_report(
    history: &History,
    renamed: &crate::history::Renamed,
    given: &[OsString],
) -> link_format::Report {
    let now = history.name(&renamed.module).unwrap_or(&renamed.now);
    let mut past = false;
    let again = std::iter::once("lnk".to_string()).chain(given.iter().map(|arg| {
        let arg = arg.to_string_lossy().into_owned();
        past |= arg == "--";
        match !past && arg == renamed.module {
            true => now.to_string(),
            false => arg,
        }
    }));
    link_format::Report::error(link_format::Sentence::adopt(&renamed.message(now)))
        .exit(link_format::Exit::Usage)
        .next(link_format::Command::typed(again), "by its name today")
}

/// Every command under `command` by its words, with each long and short
/// flag and whether it takes a value: what a history matches against.
pub fn surface(command: &clap::Command) -> Surface {
    fn walk(command: &clap::Command, prefix: &str, globals: &[String], out: &mut Surface) {
        // The globals given to every command under this one.
        let mut globals = globals.to_vec();
        globals.extend(
            command
                .get_arguments()
                .filter(|a| a.is_global_set())
                .map(|a| a.get_id().to_string()),
        );
        for sub in command.get_subcommands() {
            let path = match prefix {
                "" => sub.get_name().to_string(),
                _ => format!("{prefix} {}", sub.get_name()),
            };
            let mut flags = std::collections::BTreeMap::new();
            if sub.get_arguments().any(|a| {
                a.is_trailing_var_arg_set() || (a.is_positional() && a.is_allow_hyphen_values_set())
            }) {
                flags.insert(crate::history::TRAILING.to_string(), false);
            }
            // A local argument by a global's ID is the stand-in of a command
            // leaving that global out (`link_actions::cli::stand_ins`): no
            // flag of its own.
            let stand_in = |a: &clap::Arg| {
                !a.is_global_set() && globals.iter().any(|g| g == a.get_id().as_str())
            };
            for arg in sub.get_arguments().filter(|a| !stand_in(a)) {
                let takes = arg.get_action().takes_values();
                if let Some(long) = arg.get_long() {
                    flags.insert(format!("--{long}"), takes);
                }
                if let Some(short) = arg.get_short() {
                    flags.insert(format!("-{short}"), takes);
                }
            }
            out.insert(path.clone(), flags);
            walk(sub, &path, &globals, out);
        }
    }
    let mut out = Surface::new();
    walk(command, "", &[], &mut out);
    out
}

#[cfg(test)]
mod tests;

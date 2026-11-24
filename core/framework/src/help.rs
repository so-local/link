//! Command help from its owner's component descriptor.
//!
//! A command's entry is a table at the words you type, without `lnk`
//! (a command and its subcommands), and a program's own at
//! its declared program name. In it, `about` is the
//! command's help and `after` what follows its options; each argument's
//! help is at `"--<long>"`, `"-<short>"` when it has no long, or
//! `"<id>"` for a positional. A text is read as clap reads a doc comment:
//! paragraphs split by a blank line, each paragraph's lines joined, the
//! first paragraph alone the short help without its last period, and all
//! of them the long help.
//!
//! The file is read only when help is shown: a command line that parses
//! never reads it ([`Help::parse`]). For a program with no clap, its
//! commands' `about` make its `--help` ([`Help::usage`]).

use std::collections::BTreeSet;
use std::ffi::OsString;

/// The command line `args` as `T`, in the look: for a program whose
/// help isn't a [`Help`]'s, through the same entry ([`link_spec::run`]).
pub fn parse_styled<T: clap::Parser, A: Into<OsString> + Clone>(
    args: impl IntoIterator<Item = A>,
) -> T {
    link_spec::run::<T>(&Plain, args.into_iter().map(Into::into), |command| command).command
}

/// Parse this process through the shared entry with Link's output style.
pub fn current_styled<T: clap::Parser>() -> T {
    link_spec::entry::current::<T>(&Plain, |command| command).command
}

/// A program with no help file of its own: its look and its history.
struct Plain;

impl link_spec::Program for Plain {
    fn before(&self, _: &[OsString]) {}

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
}

/// clap's styles, from the look: headings, commands and flags bold, an
/// error's word in the error's color, nothing else, as this machine's
/// `[output]` has emphasis and color.
pub fn styles() -> clap::builder::Styles {
    use clap::builder::styling::Style;
    use link_format::look::{self, Choice};
    let output = link_settings::output::machine().output.under_env();
    let bold = match output.emphasis {
        Choice::Never => Style::new(),
        _ => look::BOLD,
    };
    let bad = match output.color {
        Choice::Never => Style::new(),
        _ => look::tone(link_format::Tone::Bad),
    };
    clap::builder::Styles::plain()
        .header(bold)
        .usage(bold)
        .literal(bold)
        .error(bad)
}

pub use declared::Declared;
mod declared;

/// A module's help is its program to the entry: the answers before any
/// parser, its look, its help, its history and its commands' bindings.
impl link_spec::Program for Help {
    fn before(&self, args: &[OsString]) {
        if asks_nexts(args) {
            self.answer_nexts();
        }
    }

    fn styled(&self, command: clap::Command) -> clap::Command {
        command.styles(styles())
    }

    fn helped(&self, command: clap::Command) -> clap::Command {
        self.apply(command)
    }

    fn arranged(&self, command: &clap::Command, args: Vec<OsString>) -> Vec<OsString> {
        grouped(command, args, self.own)
    }

    fn history(&self) -> link_spec::history::History {
        crate::descriptor::history()
    }

    fn release(&self) -> Option<fn() -> anyhow::Result<link_spec::history::Wire>> {
        Some(crate::descriptor::release)
    }

    fn binding(&self, words: &[String]) -> Option<link_spec::Binding> {
        // A released program's declarations already read: its build refuses
        // one that doesn't (`link-component-build`).
        let actions = link_component::actions::parse(self.toml).ok()?;
        let action = actions.get(&words.join(" "))?;
        if !action.takes_fields() {
            return None;
        }
        crate::descriptor::binding(action.spec.as_ref()?)
    }

    fn parsed(&self, matches: &clap::ArgMatches) {
        self.declare(matches);
    }
}

mod grouped;
use grouped::grouped;

/// Where a program's help is: its workspace's the component's command declarations, its own entry
/// (`None` for a program with no help of its own) and the table its
/// commands are under (`""`: at the top).
#[derive(Clone, Copy, Debug)]
pub struct Help {
    pub toml: &'static str,
    pub own: Option<&'static str>,
    pub under: &'static str,
}

impl Help {
    /// Its the component's command declarations, parsed once a process however often it's asked
    /// (a `--help` asks it for each command it lists); none when it
    /// doesn't parse.
    fn file(&self) -> Option<std::sync::Arc<toml::Table>> {
        type Parsed = std::collections::HashMap<usize, Option<std::sync::Arc<toml::Table>>>;
        static PARSED: std::sync::Mutex<Option<Parsed>> = std::sync::Mutex::new(None);
        let mut parsed = PARSED.lock().unwrap_or_else(|e| e.into_inner());
        parsed
            .get_or_insert_default()
            // A program's file is one static string: known by its place.
            .entry(self.toml.as_ptr() as usize)
            .or_insert_with(|| self.parse_file().map(std::sync::Arc::new))
            .clone()
    }

    fn parse_file(&self) -> Option<toml::Table> {
        let file: toml::Table = self.toml.parse().ok()?;
        self.project_file(file)
    }

    fn project_file(&self, file: toml::Table) -> Option<toml::Table> {
        let declarations = file
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("link"))
            .and_then(toml::Value::as_table)
            .unwrap_or(&file);
        // A module's `actions.toml` is its commands, each with its `help`,
        // in the format before version 1 or in version 1's, which reads
        // its actions into the same commands.
        let versioned = file.contains_key("schema");
        let actions = versioned
            || (!file.is_empty()
                && file
                    .values()
                    .all(|action| action.get("help").is_some_and(toml::Value::is_str)));
        let converted;
        let commands = match declarations.get("commands").and_then(toml::Value::as_table) {
            Some(commands) => commands,
            None if actions => {
                let actions = link_component::actions::parse(self.toml).ok()?;
                converted = toml::Value::try_from(link_component::actions::commands(&actions))
                    .ok()?
                    .as_table()?
                    .clone();
                &converted
            }
            None => return Some(file),
        };
        let mut result = toml::Table::new();
        for (path, command) in commands {
            let command = command.as_table()?;
            let mut words: Vec<_> = if path == "root" {
                self.own
                    .unwrap_or(if self.under.is_empty() {
                        "lnk"
                    } else {
                        self.under
                    })
                    .split('.')
                    .collect()
            } else {
                self.under
                    .split('.')
                    .filter(|w| !w.is_empty())
                    .chain(path.split(' '))
                    .collect()
            };
            let mut at = &mut result;
            for word in words.drain(..) {
                at = at
                    .entry(word.to_string())
                    .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                    .as_table_mut()?;
            }
            if let Some(help) = command.get("help") {
                at.insert("about".into(), help.clone());
            }
            for name in ["after", "usage"] {
                if let Some(value) = command.get(name) {
                    at.insert(name.into(), value.clone());
                }
            }
            if let Some(flags) = command.get("flags").and_then(toml::Value::as_table) {
                at.extend(flags.clone());
            }
            // The arguments it declares hidden, by how help spells them.
            let hidden: Vec<toml::Value> = command
                .get("options")
                .and_then(toml::Value::as_table)
                .into_iter()
                .flatten()
                .filter(|(_, o)| o.get("hidden").and_then(toml::Value::as_bool) == Some(true))
                .map(|(name, o)| {
                    let long = o.get("long").and_then(toml::Value::as_str);
                    let short = o.get("short").and_then(toml::Value::as_str);
                    toml::Value::String(match (long, short) {
                        (Some(long), _) => format!("--{long}"),
                        (None, Some(short)) => format!("-{short}"),
                        (None, None) => format!("<{name}>"),
                    })
                })
                .collect();
            if !hidden.is_empty() {
                at.insert(HIDDEN.into(), toml::Value::Array(hidden));
            }
        }
        if let Some(own) = self.own
            && !result.contains_key(own)
            && let Some((_, command)) = commands
                .iter()
                .find(|(_, c)| c.get("route").and_then(toml::Value::as_bool) == Some(true))
            && let Some(help) = command.get("help")
        {
            result.insert(
                own.into(),
                toml::Value::Table(toml::Table::from_iter([("about".into(), help.clone())])),
            );
        }
        Some(result)
    }

    /// `command` with its help, and every subcommand's, in the look.
    pub fn apply(&self, command: clap::Command) -> clap::Command {
        let mut command = command.styles(styles());
        let Some(file) = self.file() else {
            return command;
        };
        let file = &*file;
        let own = self.own.and_then(|own| table_at(file, own));
        if let Some(own) = own {
            command = describe(command, own);
        }
        let under = match self.under {
            "" => Some(file),
            under => table_at(file, under),
        };
        let Some(under) = under else {
            return command;
        };
        if self.own.is_none_or(|own| own != self.under) {
            command = args(command, under);
        }
        children(command, under)
    }

    /// The command line as `T`, read with `command`'s help shown only if
    /// it's asked for or the line is wrong.
    pub fn parse<T: clap::Parser>(&self) -> T {
        self.parse_current(|command| command)
    }

    /// Parse without exiting on a usage error, for a machine protocol
    /// whose caller distinguishes an unsupported command from a failure.
    pub fn try_parse<T: clap::Parser>(&self) -> Result<T, clap::Error> {
        // A retired spelling is answered as its report, never as a
        // protocol's unsupported command.
        let parsed = match link_spec::entry::try_current::<T>(self, |command| command) {
            Ok(parsed) => parsed,
            Err(link_spec::entry::Refused::Usage(error)) => return Err(error),
            Err(retired) => retired.exit(),
        };
        self.checked_command(parsed)
    }

    /// A shared protocol parser, labelled with the program running it.
    pub fn parse_named<T: clap::Parser>(&self, name: &'static str) -> T {
        self.parse_current(|command| command.name(name).bin_name(name))
    }

    /// Parse a shared protocol with the commands this program supports.
    pub fn parse_command<T: clap::Parser>(&self, command: clap::Command) -> T {
        self.parse_current(|_| command.clone())
    }

    fn parse_current<T: clap::Parser>(&self, adjust: impl Fn(clap::Command) -> clap::Command) -> T {
        let parsed = self.parse_fields::<T>(adjust);
        self.without_fields(parsed)
    }

    /// Parse this invocation and the field flags of its bound spec.
    pub fn parse_fields<T: clap::Parser>(
        &self,
        adjust: impl Fn(clap::Command) -> clap::Command,
    ) -> link_spec::Parsed<T> {
        link_spec::entry::current::<T>(self, adjust)
    }

    /// [`Help::parse`] for `args`, the program's name first.
    pub fn parse_from<T: clap::Parser>(&self, args: impl IntoIterator<Item = OsString>) -> T {
        self.parse_adjusted(args, |command| command)
    }

    /// [`Help::parse_from`], with the command line changed by `adjust`
    /// first, such as a global argument hidden where it means nothing,
    /// through the one entry every program's command line goes through
    /// ([`link_spec::run`]).
    pub fn parse_adjusted<T: clap::Parser>(
        &self,
        args: impl IntoIterator<Item = OsString>,
        adjust: impl Fn(clap::Command) -> clap::Command,
    ) -> T {
        let parsed = link_spec::run::<T>(self, args, adjust);
        self.without_fields(parsed)
    }

    fn without_fields<T: clap::Parser>(&self, parsed: link_spec::Parsed<T>) -> T {
        self.checked_command(parsed)
            .unwrap_or_else(|error| error.exit())
    }

    fn checked_command<T: clap::Parser>(
        &self,
        parsed: link_spec::Parsed<T>,
    ) -> Result<T, clap::Error> {
        if !parsed.fields.is_empty() {
            return Err(self.apply(T::command()).error(
                clap::error::ErrorKind::UnknownArgument,
                format!(
                    "lnk {} doesn't accept configuration fields through this command",
                    parsed.words.join(" ")
                ),
            ));
        }
        Ok(parsed.command)
    }

    /// [`Help::parse_adjusted`], with the field flags of the spec the
    /// command binds.
    pub fn parse_with_fields<T: clap::Parser>(
        &self,
        args: impl IntoIterator<Item = OsString>,
        adjust: impl Fn(clap::Command) -> clap::Command,
    ) -> link_spec::Parsed<T> {
        link_spec::run::<T>(self, args, adjust)
    }

    /// Each command's declared Nexts, by the command a person types (`lnk
    /// box create`): Next's graph, as this program answers `nexts
    /// --json`. A command declaring none is there too, with none.
    pub fn nexts(&self) -> std::collections::BTreeMap<String, link_format::nexts::Declared> {
        let Ok(actions) = link_component::actions::parse(self.toml) else {
            return Default::default();
        };
        actions
            .iter()
            .filter(|(path, a)| *path != "root" && !a.hidden())
            .map(|(path, a)| {
                let declared = link_format::nexts::Declared::new(a.next.clone(), a.fail.clone());
                (self.typed(path), declared)
            })
            .collect()
    }

    /// `lnk box create`, for the action at `path` (`box create`).
    fn typed(&self, path: &str) -> String {
        match self.under {
            "" => format!("lnk {path}"),
            under => format!("{under} {path}"),
        }
    }

    /// The hidden `nexts --json`: [`Help::nexts`] on stdout, and this
    /// program's end.
    pub fn answer_nexts(&self) -> ! {
        let nexts = self.nexts();
        link_format::select_json(true);
        let _ = link_settings::output::run(|out| {
            out.reply(&nexts);
            Ok(link_format::Done::shown())
        });
        std::process::exit(0)
    }

    /// What the command `matches` names declares, for the check that a
    /// debug build gives no Next it doesn't.
    pub fn declare(&self, matches: &clap::ArgMatches) {
        let mut words = Vec::new();
        let mut at = matches;
        while let Some((name, sub)) = at.subcommand() {
            words.push(name.to_string());
            at = sub;
        }
        if let Some(declared) = self.nexts().remove(&self.typed(&words.join(" "))) {
            link_format::nexts::declare(declared);
        }
    }

    /// A program with no clap: its `--help`, from its own entry's `about`
    /// and each command's (`usage` in place of its name, when it has one).
    pub fn usage(&self) -> String {
        let file = self.file().unwrap_or_default();
        let name = self.own.unwrap_or_default();
        let Some(own) = table_at(&file, name) else {
            return String::new();
        };
        let mut text = String::new();
        if let Some(about) = own.get("about").and_then(|a| a.as_str()) {
            for paragraph in paragraphs(about) {
                text += &wrapped(&paragraph, 0);
                text += "\n";
            }
        }
        text += &format!("Usage: {name} <COMMAND>\n\nCommands:\n");
        let commands: Vec<(String, String)> = own
            .iter()
            .filter_map(|(command, entry)| {
                let entry = entry.as_table()?;
                let usage = entry.get("usage").and_then(|u| u.as_str());
                let about = entry.get("about").and_then(|a| a.as_str())?;
                Some((
                    usage.unwrap_or(command).to_string(),
                    short(&paragraphs(about)),
                ))
            })
            .collect();
        // With a usage wider than this, each has its line of its own,
        // its help under it, as clap lays out long ones; every help
        // wrapped to [`LINE`].
        const WIDE: usize = 32;
        let width = commands.iter().map(|(c, _)| c.len()).max().unwrap_or(0);
        for (command, about) in commands {
            if width <= WIDE {
                text += &format!("  {command:<width$}  ");
                text += &wrapped(&about, width + 4);
            } else {
                text += &format!("  {command}\n          ");
                text += &wrapped(&about, 10);
            }
        }
        text
    }

    /// The commands a program with no clap answers, by its entry.
    pub fn commands(&self) -> Vec<String> {
        let file = self.file().unwrap_or_default();
        self.own
            .and_then(|own| table_at(&file, own))
            .map(|own| {
                own.iter()
                    .filter(|(_, entry)| entry.is_table())
                    .map(|(command, _)| command.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The short help of the command at `key` (`agent.start`), as `--help`
    /// lists it.
    pub fn about(&self, key: &str) -> String {
        let file = self.file().unwrap_or_default();
        table_at(&file, key)
            .and_then(|entry| entry.get("about"))
            .and_then(|about| about.as_str())
            .map(|about| short(&paragraphs(about)))
            .unwrap_or_default()
    }
}

/// The table at the dotted `key`; a part is a bare key, or quoted
/// (`agent."<harness>"`).
fn table_at<'a>(file: &'a toml::Table, key: &str) -> Option<&'a toml::Table> {
    parts(key)
        .iter()
        .try_fold(file, |table, part| table.get(part)?.as_table())
}

fn parts(key: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut quoted = false;
    for c in key.chars() {
        match c {
            '"' => quoted = !quoted,
            '.' if !quoted => parts.push(std::mem::take(&mut part)),
            c => part.push(c),
        }
    }
    parts.push(part);
    parts
}

/// A text's paragraphs, each one line, as clap makes a doc comment's.
fn paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n")
        .map(|p| {
            p.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|p| !p.is_empty())
        .collect()
}

/// The first paragraph without its last period, as clap's short help.
fn short(paragraphs: &[String]) -> String {
    let first = paragraphs.first().cloned().unwrap_or_default();
    match first.strip_suffix('.') {
        Some(cut) if !first.ends_with("..") => cut.to_string(),
        _ => first,
    }
}

/// The widest a line of [`Help::usage`] is.
const LINE: usize = 80;

/// `text` from column `indent` on, its words wrapped to [`LINE`], each
/// line after the first indented as far; ended by a newline.
fn wrapped(text: &str, indent: usize) -> String {
    let mut out = String::new();
    let mut column = indent;
    for (i, word) in text.split_whitespace().enumerate() {
        if i > 0 && column + 1 + word.len() > LINE {
            out += "\n";
            out += &" ".repeat(indent);
            column = indent;
        } else if i > 0 {
            out += " ";
            column += 1;
        }
        out += word;
        column += word.len();
    }
    out + "\n"
}

/// `text` as (short help, long help when it has more than one paragraph).
fn split(text: &str) -> (String, Option<String>) {
    let paragraphs = paragraphs(text);
    let long = (paragraphs.len() > 1).then(|| paragraphs.join("\n\n"));
    (short(&paragraphs), long)
}

/// The command's own `about` and `after`, and its arguments'.
fn describe(command: clap::Command, entry: &toml::Table) -> clap::Command {
    describe_under(command, entry, &[])
}

/// [`describe`], with `globals` the global arguments of the commands
/// above it.
fn describe_under(
    mut command: clap::Command,
    entry: &toml::Table,
    globals: &[clap::Arg],
) -> clap::Command {
    if let Some(about) = entry.get("about").and_then(|a| a.as_str()) {
        let (short, long) = split(about);
        command = command.about(short);
        if let Some(long) = long {
            command = command.long_about(long);
        }
    }
    if let Some(after) = entry.get("after").and_then(|a| a.as_str()) {
        command = command.after_help(paragraphs(after).join("\n\n"));
    }
    args_under(command, entry, globals)
}

/// The key of a command's entry listing the arguments it declares hidden
/// (`OptionDeclaration::hidden`): no command or flag is spelled so.
const HIDDEN: &str = "<hidden>";

/// Whether `entry` declares the argument spelled `key` hidden.
fn hidden(entry: &toml::Table, key: &str) -> bool {
    entry
        .get(HIDDEN)
        .and_then(toml::Value::as_array)
        .is_some_and(|keys| keys.iter().any(|k| k.as_str() == Some(key)))
}

fn args(command: clap::Command, entry: &toml::Table) -> clap::Command {
    args_under(command, entry, &[])
}

/// [`args`], with `globals` the global arguments of the commands above:
/// one declared hidden here is given a hidden copy of its own, since a
/// global reaches a subcommand only once the whole command is built.
fn args_under(
    mut command: clap::Command,
    entry: &toml::Table,
    globals: &[clap::Arg],
) -> clap::Command {
    for global in globals {
        let own = command
            .get_arguments()
            .any(|a| a.get_id() == global.get_id());
        if !own && hidden(entry, &arg_key(global)) {
            command = command.arg(global.clone().global(false).hide(true));
        }
    }
    let keys: Vec<(String, String)> = command
        .get_arguments()
        .map(|a| (a.get_id().to_string(), arg_key(a)))
        .collect();
    for (id, key) in &keys {
        if hidden(entry, key) {
            command = command.mut_arg(id, |arg| arg.hide(true));
        }
    }
    for (id, key) in keys {
        if let Some(text) = entry.get(&key).and_then(|t| t.as_str()) {
            let (short, long) = split(text);
            command = command.mut_arg(id, |arg| match long {
                Some(long) => arg.help(short).long_help(long),
                None => arg.help(short),
            });
        }
    }
    command
}

fn children(command: clap::Command, entry: &toml::Table) -> clap::Command {
    children_under(command, entry, &[])
}

/// [`children`], with `globals` the global arguments of the commands
/// above it.
fn children_under(
    mut command: clap::Command,
    entry: &toml::Table,
    globals: &[clap::Arg],
) -> clap::Command {
    let mut globals = globals.to_vec();
    globals.extend(
        command
            .get_arguments()
            .filter(|a| a.is_global_set())
            .cloned(),
    );
    let names: Vec<String> = command
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    for name in names {
        if let Some(sub) = entry.get(&name).and_then(|s| s.as_table()) {
            command = command.mut_subcommand(&name, |c| {
                children_under(describe_under(c, sub, &globals), sub, &globals)
            });
        }
    }
    command
}

/// An argument's key in its command's entry: `--<long>`, `-<short>`, or
/// `<id>`.
fn arg_key(arg: &clap::Arg) -> String {
    match (arg.get_long(), arg.get_short()) {
        (Some(long), _) => format!("--{long}"),
        (None, Some(short)) => format!("-{short}"),
        (None, None) => format!("<{}>", arg.get_id()),
    }
}

/// For a test: every problem with the programs' help in `toml`, the one
/// file they share. Each is its command line as clap makes it from the
/// code, with its [`Help`]. A command or an argument with no entry, help
/// left in the code, and an entry naming nothing are each a problem.
pub fn check(toml: &str, programs: &[(clap::Command, Help)]) -> Vec<String> {
    let file: toml::Table = match toml.parse() {
        Ok(file) => file,
        Err(err) => return vec![format!("help.toml doesn't parse: {err}")],
    };
    let file = match programs
        .first()
        .map(|(_, help)| help.project_file(file.clone()))
        .unwrap_or(Some(file))
    {
        Some(file) => file,
        None => return vec!["command metadata cannot be projected as help".into()],
    };
    let mut problems = Vec::new();
    let mut used = BTreeSet::new();
    for (command, help) in programs {
        match help.own {
            Some(own) => check_entry(command, own, &file, &mut used, &mut problems),
            None => check_args(command, help.under, &file, &mut used, &mut problems),
        }
        let globals = globals(command, &[]);
        for sub in command.get_subcommands() {
            check_tree(
                sub,
                &join(help.under, sub.get_name()),
                &file,
                &globals,
                &mut used,
                &mut problems,
            );
        }
        // Help in the code is help the file doesn't say.
        left_in_code(command, help.own.unwrap_or(help.under), &mut problems);
    }
    // A table holding only others' entries (`[lnk-example.connect]`'s).
    for key in used.clone() {
        let parts = parts(&key);
        for i in 1..parts.len() {
            used.insert(parts[..i].iter().fold(String::new(), |k, p| join(&k, p)));
        }
    }
    let mut unused: Vec<String> = Vec::new();
    for key in entries(&file, "") {
        // Inside a table naming nothing: said of the table.
        if !used.contains(&key) && !unused.iter().any(|u| key.starts_with(&format!("{u}."))) {
            unused.push(key.clone());
            problems.push(format!("help.toml: {key} names no command or argument"));
        }
    }
    problems
}

fn join(under: &str, name: &str) -> String {
    let name = match name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        true => name.to_string(),
        false => format!("\"{name}\""),
    };
    match under {
        "" => name,
        under => format!("{under}.{name}"),
    }
}

fn check_tree(
    command: &clap::Command,
    key: &str,
    file: &toml::Table,
    inherited: &[String],
    used: &mut BTreeSet<String>,
    problems: &mut Vec<String>,
) {
    check_entry(command, key, file, used, problems);
    // A global argument of a command above is this one's too, and its
    // entry here may say so; its help is the command's that declares it.
    for name in inherited {
        used.insert(format!("{key}.\"{name}\""));
    }
    let inherited = globals(command, inherited);
    for sub in command.get_subcommands() {
        check_tree(
            sub,
            &join(key, sub.get_name()),
            file,
            &inherited,
            used,
            problems,
        );
    }
}

/// `inherited` and `command`'s own global arguments, by their keys.
fn globals(command: &clap::Command, inherited: &[String]) -> Vec<String> {
    let mut all = inherited.to_vec();
    all.extend(
        command
            .get_arguments()
            .filter(|arg| arg.is_global_set())
            .map(arg_key),
    );
    all
}

/// The command at `key`: its entry and `about`, and its arguments'.
fn check_entry(
    command: &clap::Command,
    key: &str,
    file: &toml::Table,
    used: &mut BTreeSet<String>,
    problems: &mut Vec<String>,
) {
    used.insert(key.to_string());
    match table_at(file, key) {
        None => problems.push(format!("help.toml: no [{key}], the help of {key}")),
        Some(entry) => {
            if entry.get("about").and_then(|a| a.as_str()).is_none() {
                problems.push(format!("help.toml: [{key}] has no about"));
            }
            for word in ["about", "after", HIDDEN] {
                if entry.contains_key(word) {
                    used.insert(join(key, word));
                }
            }
        }
    }
    check_args(command, key, file, used, problems);
}

fn check_args(
    command: &clap::Command,
    key: &str,
    file: &toml::Table,
    used: &mut BTreeSet<String>,
    problems: &mut Vec<String>,
) {
    let entry = match key {
        "" => Some(file),
        key => table_at(file, key),
    };
    for arg in command.get_arguments().filter(|arg| !standard_action(arg)) {
        let name = arg_key(arg);
        let full = format!("{key}.\"{name}\"");
        used.insert(full.clone());
        if entry
            .and_then(|e| e.get(&name))
            .and_then(|t| t.as_str())
            .is_none()
        {
            problems.push(format!("help.toml: no {full}, the argument's help"));
        }
    }
}

fn left_in_code(command: &clap::Command, key: &str, problems: &mut Vec<String>) {
    if command.get_about().is_some()
        || command.get_long_about().is_some()
        || command.get_after_help().is_some()
    {
        problems.push(format!(
            "{key}: help in the code (a doc comment?), not help.toml"
        ));
    }
    for arg in command.get_arguments().filter(|arg| !standard_action(arg)) {
        if arg.get_help().is_some() || arg.get_long_help().is_some() {
            problems.push(format!(
                "{key} {}: help in the code (a doc comment?), not help.toml",
                arg_key(arg)
            ));
        }
    }
    for sub in command.get_subcommands() {
        left_in_code(sub, &join(key, sub.get_name()), problems);
    }
}

/// Clap's help and version actions carry its own generated descriptions.
fn standard_action(arg: &clap::Arg) -> bool {
    matches!(
        arg.get_action(),
        clap::ArgAction::Help
            | clap::ArgAction::HelpShort
            | clap::ArgAction::HelpLong
            | clap::ArgAction::Version
    )
}

/// Every entry in `table` at `key`, as [`check`] names them: a command's
/// table, its `about` and `after`, and its arguments.
fn entries(table: &toml::Table, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (name, value) in table {
        match value {
            toml::Value::Table(sub) => {
                let at = join(key, name);
                out.push(at.clone());
                out.extend(entries(sub, &at));
            }
            _ if name == "about" || name == "after" => out.push(format!("{key}.{name}")),
            _ => out.push(format!("{key}.\"{name}\"")),
        }
    }
    out
}

/// For a test of a program with no clap: the commands its `source`'s
/// function `function` answers, as the string patterns of its match
/// arms (`"install" =>`, `Some("serve") =>`), but `--version`, `-V`,
/// `--help`, `-h` and `help`.
pub fn answered(source: &str, function: &str) -> Vec<String> {
    let start = source
        .find(&format!("fn {function}("))
        .unwrap_or(source.len());
    let body = &source[start..];
    let end = body.find("\n}\n").map_or(body.len(), |e| e + 1);
    let mut commands = BTreeSet::new();
    for line in body[..end].lines() {
        let line = line.trim();
        let Some((pattern, _)) = line.split_once("=>") else {
            continue;
        };
        for word in pattern.split('"').skip(1).step_by(2) {
            if !matches!(word, "--version" | "-V" | "--help" | "-h" | "help")
                && word.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && !word.starts_with('-')
            {
                commands.insert(word.to_string());
            }
        }
    }
    commands.into_iter().collect()
}

/// Whether `args` ask for the hidden `nexts --json`, with or without the
/// program's group first.
pub fn asks_nexts(args: &[OsString]) -> bool {
    let words: Vec<&OsString> = args.iter().skip(1).collect();
    match words.as_slice() {
        [n, j] | [_, n, j] => *n == "nexts" && *j == "--json",
        _ => false,
    }
}

#[cfg(test)]
mod tests;

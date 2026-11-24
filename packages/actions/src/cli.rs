//! What a generated command-line parser calls: each declared runtime
//! argument as a clap argument, and a command's matches read back into
//! its typed arguments, checked as a file would be.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::value::Validate;

/// How a runtime argument's value is typed on the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Given alone, it's true.
    Switch,
    Text,
    Integer,
    Number,
    /// Repeatable: a list of text.
    Texts,
    Integers,
    /// Every word after the others, unparsed.
    Rest,
}

/// One runtime argument, as its declaration says.
#[derive(Clone, Copy, Debug)]
pub struct Arg {
    pub name: &'static str,
    pub kind: Kind,
    pub help: &'static str,
    pub required: bool,
    pub position: Option<usize>,
    pub short: Option<char>,
    pub env: Option<&'static str>,
    /// A closed choice's words.
    pub values: &'static [&'static str],
    /// The value its absence means.
    pub default: Option<&'static str>,
    /// A rest taken only after `--`.
    pub last: bool,
    /// A whole number's least and most, refused by the parser naming the
    /// flag before its type's width is.
    pub range: (Option<i64>, Option<i64>),
    /// What its flag alone means, its value left out: an optional value.
    pub default_missing: Option<&'static str>,
    /// The arguments it can't be given with.
    pub conflicts: &'static [&'static str],
    /// The arguments it's only given with.
    pub requires: &'static [&'static str],
    /// Kept out of help.
    pub hidden: bool,
    /// Every command of its program takes it, before or after the
    /// command's words: declared once in the module's `[args]`.
    pub global: bool,
    /// The kind of Link ID it takes (`format = "link-id"`, `x-link-kind`):
    /// a name a person types for it is looked up by the kind's [`Lookup`]
    /// and given as the ID, so no action receives a name.
    pub id_kind: Option<&'static str>,
}

/// How a program looks up a name a person typed for a kind of Link ID:
/// the ID it names, `<kind>:<id>`, or why there's none, in a sentence.
pub type Lookup = fn(&str) -> Result<String, String>;

static LOOKUPS: std::sync::RwLock<Vec<(&'static str, Lookup)>> = std::sync::RwLock::new(Vec::new());

/// Names of `kind` typed on this program's command line are looked up
/// with `lookup`: the one place a person's names become IDs, before any
/// command reads its arguments.
pub fn names(kind: &'static str, lookup: Lookup) {
    let mut held = LOOKUPS.write().unwrap_or_else(|e| e.into_inner());
    held.retain(|(k, _)| *k != kind);
    held.push((kind, lookup));
}

/// `text`, given for an ID of `kind`: itself when it's one, else the ID
/// the kind's lookup finds for it as a name. Left as given where no
/// lookup is set, for the argument's check to refuse.
fn as_id(kind: &str, text: &str) -> Result<String, String> {
    if text
        .strip_prefix(kind)
        .is_some_and(|rest| rest.starts_with(':'))
    {
        return Ok(text.to_string());
    }
    let lookup = LOOKUPS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, lookup)| *lookup);
    match lookup {
        Some(lookup) => lookup(text),
        None => Ok(text.to_string()),
    }
}

/// One action's command, as its declaration says.
#[derive(Clone, Copy, Debug)]
pub struct Action {
    pub word: &'static str,
    pub description: &'static str,
    pub hidden: bool,
    pub args: &'static [Arg],
}

/// The clap command for `action`.
pub fn command(action: &Action) -> clap::Command {
    let summary = action.description.split("\n\n").next().unwrap_or_default();
    let mut command = clap::Command::new(action.word)
        .about(summary.replace('\n', " "))
        .long_about(action.description.to_string())
        .hide(action.hidden);
    let after = action
        .args
        .iter()
        .filter_map(|a| a.position)
        .max()
        .unwrap_or(0)
        + 1;
    for arg in ordered(action.args).into_iter().filter(|a| !a.global) {
        command = command.arg(placed(arg, clap_arg(arg, after), action.args));
    }
    command
}

/// `root`, a program that renders its own help, with clap's `--help`
/// given back to every command under it: disabling it at the root
/// disables it below too.
pub fn nested_help(root: clap::Command) -> clap::Command {
    fn nested(command: clap::Command, help: &clap::Arg) -> clap::Command {
        // Its own, so a command built alone, as for a usage error,
        // adds no second.
        command
            .disable_help_flag(true)
            .arg(help.clone())
            .mut_subcommands(|command| nested(command, help))
    }
    let mut defaults = clap::Command::new("defaults");
    defaults.build();
    let Some(help) = defaults
        .get_arguments()
        .find(|a| a.get_id() == "help")
        .cloned()
    else {
        return root;
    };
    root.mut_subcommands(|command| nested(command, &help))
}

/// `root` reading its variables from `env` rather than this process's
/// environment: an argument that falls back to a variable takes its
/// value from `env`, as the line would if it ran with `env`. A program
/// that reads several lines in one process, each meant to run with
/// its own environment, reads each one this way.
pub fn under(root: clap::Command, env: &dyn Fn(&str) -> Option<String>) -> clap::Command {
    fn each(command: clap::Command, env: &dyn Fn(&str) -> Option<String>) -> clap::Command {
        command
            .mut_args(|arg| {
                let Some(name) = arg.get_env().map(|n| n.to_string_lossy().into_owned()) else {
                    return arg;
                };
                let arg = arg.env(None::<&'static str>);
                let Some(value) = env(&name) else {
                    return arg;
                };
                // A switch reads its variable as clap does: "", 0, false,
                // no or off (any case) is off, anything else on.
                let value = match arg.get_action() {
                    clap::ArgAction::SetTrue => {
                        let off = ["", "0", "false", "no", "off"]
                            .iter()
                            .any(|word| value.eq_ignore_ascii_case(word));
                        (!off).to_string()
                    }
                    _ => value,
                };
                // Given by its variable, it's no longer missing: clap
                // refuses a required argument that has a default.
                arg.required(false).default_value(value)
            })
            .mut_subcommands(|command| each(command, env))
    }
    each(root, env)
}

/// `args` as clap arguments, for a group or a program's root: every
/// command's own arguments, its program's flags and a default's.
pub fn args(args: &[Arg]) -> Vec<clap::Arg> {
    let after = args.iter().filter_map(|a| a.position).max().unwrap_or(0) + 1;
    ordered(args)
        .into_iter()
        .filter(|a| !a.global)
        .map(|arg| placed(arg, clap_arg(arg, after), args))
        .collect()
}

/// `args` as clap reads positionals, in the order they're added: the
/// positionals by place, then the rest and the flags.
fn ordered(args: &[Arg]) -> Vec<&Arg> {
    let mut ordered: Vec<&Arg> = args.iter().collect();
    ordered.sort_by_key(|a| a.position.unwrap_or(usize::MAX));
    ordered
}

/// `out`, `arg`'s clap argument, as `args` place it: a positional list
/// takes one or more, and a rest after one is taken only after `--`.
fn placed(arg: &Arg, out: clap::Arg, args: &[Arg]) -> clap::Arg {
    let listed = |a: &Arg| a.position.is_some() && matches!(a.kind, Kind::Texts | Kind::Integers);
    if listed(arg) {
        out.num_args(1..)
    } else if arg.kind == Kind::Rest
        && arg.position.is_none()
        && (arg.last || args.iter().any(listed))
    {
        out.last(true).trailing_var_arg(false).num_args(1..)
    } else {
        out
    }
}

/// The arguments every command of a program takes, for its root, which
/// clap gives each command under it.
/// Hidden stand-ins for the module-wide arguments `names` an action
/// leaves out: clap gives no command a global it defines itself, so the
/// action's command shows none of them, and [`left_out`] refuses one given.
pub fn stand_ins(every: &[Arg], names: &[&str]) -> Vec<clap::Arg> {
    every
        .iter()
        .filter(|arg| names.contains(&arg.name))
        .map(|arg| {
            let stand_in = Arg {
                global: false,
                hidden: true,
                required: false,
                env: None,
                default: None,
                ..*arg
            };
            clap_arg(&stand_in, 1)
        })
        .collect()
}

/// A usage error where the command line gives one of the module-wide
/// arguments `names` its command leaves out, before or after its command.
pub fn left_out(
    command: &clap::Command,
    matches: &[&clap::ArgMatches],
    names: &[&str],
) -> Result<(), clap::Error> {
    for name in names {
        let given = matches.iter().any(|m| {
            m.try_get_raw(name).is_ok_and(|raw| raw.is_some())
                && m.value_source(name) == Some(clap::parser::ValueSource::CommandLine)
        });
        if given {
            return Err(command.clone().error(
                clap::error::ErrorKind::UnknownArgument,
                format!("--{name} isn't one of this command's arguments"),
            ));
        }
    }
    Ok(())
}

pub fn globals(args: &[Arg]) -> Vec<clap::Arg> {
    args.iter()
        .filter(|a| a.global)
        .map(|arg| clap_arg(arg, 1))
        .collect()
}

fn clap_arg(arg: &Arg, rest: usize) -> clap::Arg {
    use clap::ArgAction;
    let mut out = clap::Arg::new(arg.name)
        .help(arg.help.to_string())
        .required(arg.required)
        .hide(arg.hidden)
        .global(arg.global)
        .conflicts_with_all(arg.conflicts);
    for other in arg.requires {
        out = out.requires(*other);
    }
    out = match arg.position {
        Some(index) => out.index(index).value_name(arg.name),
        None if arg.kind == Kind::Rest => out.index(rest).value_name("ARGS"),
        None => out.long(arg.name),
    };
    if let Some(short) = arg.short {
        out = out.short(short);
    }
    if let Some(env) = arg.env {
        out = out.env(env);
    }
    if !arg.values.is_empty() {
        out = out.value_parser(clap::builder::PossibleValuesParser::new(
            arg.values.iter().copied(),
        ));
    }
    if let Some(default) = arg.default {
        out = out.default_value(default);
    }
    if let Some(missing) = arg.default_missing {
        out = out
            .num_args(0..=1)
            .require_equals(false)
            .default_missing_value(missing);
    }
    match arg.kind {
        Kind::Switch => out.action(ArgAction::SetTrue),
        Kind::Text => out.action(ArgAction::Set),
        Kind::Integer => out.action(ArgAction::Set).value_parser(whole(arg.range)),
        Kind::Number => out
            .action(ArgAction::Set)
            .value_parser(clap::value_parser!(f64)),
        Kind::Texts => out.action(ArgAction::Append),
        Kind::Integers => out.action(ArgAction::Append).value_parser(whole(arg.range)),
        Kind::Rest => out
            .action(ArgAction::Append)
            .num_args(0..)
            .trailing_var_arg(true)
            .allow_hyphen_values(true),
    }
}

/// A whole number's parser, within its declared bounds.
fn whole(range: (Option<i64>, Option<i64>)) -> clap::builder::RangedI64ValueParser<i64> {
    let parser = clap::value_parser!(i64);
    match range {
        (Some(min), Some(max)) => parser.range(min..=max),
        (Some(min), None) => parser.range(min..),
        (None, Some(max)) => parser.range(..=max),
        (None, None) => parser.range(..),
    }
}

/// `matches` read into `T`, a command's typed arguments, and checked:
/// what a bound or a pattern refuses is a usage error naming the flag.
pub fn read<T: DeserializeOwned + Validate>(
    command: &clap::Command,
    matches: &clap::ArgMatches,
    args: &[Arg],
) -> Result<T, clap::Error> {
    let usage = |message: String| {
        command
            .clone()
            .error(clap::error::ErrorKind::ValueValidation, message)
    };
    let flag_of = |arg: &Arg| match arg.position {
        Some(_) => format!("<{}>", arg.name.to_uppercase()),
        None => format!("--{}", arg.name),
    };
    let mut object = Map::new();
    for arg in args {
        let id = arg.name;
        let value = match arg.kind {
            Kind::Switch => matches.get_flag(id).then_some(Value::Bool(true)),
            Kind::Text => match matches.get_one::<String>(id) {
                Some(text) => Some(Value::String(match arg.id_kind {
                    Some(kind) => as_id(kind, text)
                        .map_err(|why| usage(format!("{}: {why}", flag_of(arg))))?,
                    None => text.clone(),
                })),
                None => None,
            },
            Kind::Integer => matches.get_one::<i64>(id).map(|n| Value::from(*n)),
            Kind::Number => matches
                .get_one::<f64>(id)
                .and_then(|n| serde_json::Number::from_f64(*n))
                .map(Value::Number),
            Kind::Texts | Kind::Rest => matches
                .get_many::<String>(id)
                .map(|v| Value::Array(v.map(|s| Value::String(s.clone())).collect())),
            Kind::Integers => matches
                .get_many::<i64>(id)
                .map(|v| Value::Array(v.map(|n| Value::from(*n)).collect())),
        };
        if let Some(value) = value {
            object.insert(id.to_string(), value);
        }
    }
    let typed: T =
        serde_json::from_value(Value::Object(object)).map_err(|e| usage(e.to_string()))?;
    typed.validate("").map_err(|refusal| {
        // Where the refusal is, `ports[1]`, spelled as the flag it was given by.
        let name_end = refusal.at.find(['[', '.']).unwrap_or(refusal.at.len());
        let (name, rest) = refusal.at.split_at(name_end);
        let flag = args
            .iter()
            .find(|a| a.name == name)
            .map_or_else(|| refusal.at.clone(), |a| format!("{}{rest}", flag_of(a)));
        match flag.is_empty() {
            true => usage(refusal.why),
            false => usage(format!("{flag}: {}", refusal.why)),
        }
    })?;
    Ok(typed)
}

/// A command line refused, as its parser says it.
pub type Error = clap::Error;

/// A line [`argv`] wrote, its `words` first, read back into its typed
/// arguments by the same table: what a caller holding an earlier line
/// reads, never by hand.
pub fn from_argv<T: DeserializeOwned + Validate>(
    words: &[&'static str],
    args: &'static [Arg],
    line: &[String],
) -> Result<T, clap::Error> {
    let action = Action {
        word: words.last().copied().unwrap_or("line"),
        description: "",
        hidden: true,
        args,
    };
    // A line alone takes the module-wide arguments its program's root
    // would, as its own.
    let command = args
        .iter()
        .filter(|a| a.global)
        .fold(command(&action), |command, arg| {
            command.arg(clap_arg(arg, 0).global(false))
        });
    let given = words.len().saturating_sub(1);
    if line.len() < words.len() || line.iter().zip(words).any(|(given, word)| given != word) {
        return Err(command.clone().error(
            clap::error::ErrorKind::InvalidSubcommand,
            format!("{line:?} isn't a line of {:?}", words.join(" ")),
        ));
    }
    let matches = command
        .clone()
        .try_get_matches_from(line.iter().skip(given))?;
    read(&command, &matches, args)
}

/// The command line that gives `value`, an action's runtime arguments
/// as JSON, to the program that reads them: `words`, its command, then
/// its positionals in order, its flags, and what takes the rest after
/// `--`. The inverse of [`read`], so a caller building a command line and
/// the program parsing it share one declaration.
pub fn argv(words: &[&str], args: &[Arg], value: &Value) -> Vec<String> {
    let mut out: Vec<String> = words.iter().map(|w| w.to_string()).collect();
    let given = |arg: &Arg| value.get(arg.name).filter(|v| !v.is_null());
    let text = |value: &Value| match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let mut positionals: Vec<&Arg> = args.iter().filter(|a| a.position.is_some()).collect();
    positionals.sort_by_key(|a| a.position);
    for arg in positionals {
        match given(arg) {
            Some(Value::Array(items)) => out.extend(items.iter().map(text)),
            Some(value) => out.push(text(value)),
            None => {}
        }
    }
    let mut rest = Vec::new();
    for arg in args.iter().filter(|a| a.position.is_none()) {
        let Some(value) = given(arg) else { continue };
        let flag = format!("--{}", arg.name);
        match (arg.kind, value) {
            (Kind::Rest, Value::Array(items)) => rest.extend(items.iter().map(text)),
            (Kind::Switch, Value::Bool(on)) => {
                if *on {
                    out.push(flag);
                }
            }
            (_, Value::Array(items)) => {
                for item in items {
                    out.push(flag.clone());
                    out.push(text(item));
                }
            }
            (_, value) => {
                out.push(flag);
                out.push(text(value));
            }
        }
    }
    if !rest.is_empty() {
        out.push("--".into());
        out.extend(rest);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn arg(name: &'static str, kind: Kind, position: Option<usize>) -> Arg {
        Arg {
            name,
            kind,
            help: "",
            required: false,
            position,
            short: None,
            env: None,
            values: &[],
            default: None,
            range: (None, None),
            last: false,
            default_missing: None,
            conflicts: &[],
            requires: &[],
            hidden: false,
            global: false,
            id_kind: None,
        }
    }

    /// A positional list, then a rest only after `--`, declared rest
    /// first, as names sort: `lnk storage push a b -- --x`.
    #[test]
    fn a_rest_after_a_positional_list_is_taken_after_the_dashes() {
        const ARGS: &[Arg] = &[
            arg("flags", Kind::Rest, None),
            arg("paths", Kind::Texts, Some(1)),
            arg("yes", Kind::Switch, None),
        ];
        let action = Action {
            word: "push",
            description: "Push",
            hidden: false,
            args: ARGS,
        };
        let command = command(&action);
        command.clone().debug_assert();
        let matches = command
            .try_get_matches_from(["push", "a", "b", "--yes", "--", "--x"])
            .unwrap();
        let words = |id: &str| -> Vec<&String> { matches.get_many(id).unwrap().collect() };
        assert_eq!(words("paths"), ["a", "b"]);
        assert_eq!(words("flags"), ["--x"]);
        assert!(matches.get_flag("yes"));
    }

    /// An argument that falls back to a variable reads it from the
    /// environment it's given, never this process's: set, empty or
    /// unset, and a flag given still wins.
    #[test]
    fn a_line_reads_its_variables_from_the_environment_it_is_given() {
        const ARGS: &[Arg] = &[Arg {
            env: Some("UNDER_URL"),
            ..arg("url", Kind::Text, None)
        }];
        let action = Action {
            word: "serve",
            description: "Serve",
            hidden: false,
            args: ARGS,
        };
        let read = |argv: &[&str], value: Option<&str>| -> Option<String> {
            let env = |name: &str| {
                (name == "UNDER_URL")
                    .then_some(value)
                    .flatten()
                    .map(String::from)
            };
            under(command(&action), &env)
                .try_get_matches_from(argv)
                .unwrap()
                .get_one::<String>("url")
                .cloned()
        };
        assert_eq!(
            read(&["serve"], Some("http://a")).as_deref(),
            Some("http://a")
        );
        assert_eq!(read(&["serve"], Some("")).as_deref(), Some(""));
        assert_eq!(read(&["serve"], None), None);
        assert_eq!(
            read(&["serve", "--url", "http://b"], Some("http://a")).as_deref(),
            Some("http://b")
        );
    }

    /// A name typed for an ID is the ID its kind's lookup finds, once,
    /// before the arguments are read; an ID is taken as given; and a name
    /// the lookup can't find is a usage error naming the flag.
    #[test]
    fn a_name_typed_for_an_id_is_sent_as_the_id_its_lookup_finds() {
        #[derive(serde::Deserialize)]
        struct Given {
            thing: Option<String>,
        }
        impl Validate for Given {
            fn validate(&self, _: &str) -> Result<(), crate::value::Refusal> {
                Ok(())
            }
        }
        const ARGS: &[Arg] = &[Arg {
            id_kind: Some("thing"),
            ..arg("thing", Kind::Text, None)
        }];
        names("thing", |name| match name {
            "alpha" => Ok("thing:k7m2p9q4r8s3".into()),
            other => Err(format!("you have no thing named {other}")),
        });
        let action = Action {
            word: "show",
            description: "Show",
            hidden: false,
            args: ARGS,
        };
        let command = command(&action);
        let given = |line: &[&str]| {
            let matches = command.clone().try_get_matches_from(line).unwrap();
            read::<Given>(&command, &matches, ARGS)
        };
        let named = given(&["show", "--thing", "alpha"]).unwrap();
        assert_eq!(named.thing.as_deref(), Some("thing:k7m2p9q4r8s3"));
        let id = given(&["show", "--thing", "thing:abcdefghijkl"]).unwrap();
        assert_eq!(id.thing.as_deref(), Some("thing:abcdefghijkl"));
        let unknown = given(&["show", "--thing", "beta"])
            .err()
            .unwrap()
            .to_string();
        assert!(
            unknown.contains("--thing: you have no thing named beta"),
            "{unknown}"
        );
    }

    /// A switch takes its variable by clap's rule, and a required argument
    /// given by its variable needs no flag, while missing both is refused.
    #[test]
    fn a_switch_and_a_required_argument_read_their_variables_as_clap_does() {
        const ARGS: &[Arg] = &[
            Arg {
                env: Some("UNDER_ON"),
                ..arg("on", Kind::Switch, None)
            },
            Arg {
                env: Some("UNDER_URL"),
                required: true,
                ..arg("url", Kind::Text, None)
            },
        ];
        let action = Action {
            word: "serve",
            description: "Serve",
            hidden: false,
            args: ARGS,
        };
        let parse = |argv: &[&str], on: Option<&str>, url: Option<&str>| {
            let env = |name: &str| match name {
                "UNDER_ON" => on.map(String::from),
                "UNDER_URL" => url.map(String::from),
                _ => None,
            };
            under(command(&action), &env).try_get_matches_from(argv)
        };
        let switch = |on: Option<&str>| {
            parse(&["serve", "--url", "u"], on, None)
                .unwrap()
                .get_flag("on")
        };
        assert!(switch(Some("1")));
        assert!(switch(Some("yes")));
        assert!(!switch(Some("")));
        assert!(!switch(Some("off")));
        assert!(!switch(Some("FALSE")));
        assert!(!switch(None));
        let url = |argv: &[&str], value: Option<&str>| {
            parse(argv, None, value).map(|m| m.get_one::<String>("url").cloned())
        };
        assert_eq!(
            url(&["serve"], Some("http://a")).unwrap().as_deref(),
            Some("http://a")
        );
        assert_eq!(
            url(&["serve", "--url", "http://b"], None)
                .unwrap()
                .as_deref(),
            Some("http://b")
        );
        assert!(url(&["serve"], None).is_err());
    }
}

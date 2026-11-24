//! Actual parser validation: values on all references, complete argv on runnable examples.

use std::path::Path;
use std::sync::LazyLock;

use clap::{Arg, Command, builder::StringValueParser, error::ErrorKind};
use link_docs_check::{Finding, Tree, fail};

use link_docs_check::examples;

fn template(value: &str) -> bool {
    value.contains('$') || (value.contains('<') && value.contains('>')) || value == "..."
}

fn prepare(command: Command) -> Command {
    command
        .mut_args(|arg| {
            // A documentation check never inherits the session's environment.
            // Provided literal values are checked with the original parser below.
            let arg = arg.env(None::<&str>);
            if arg.get_action().takes_values() {
                arg.value_parser(StringValueParser::new())
            } else {
                arg
            }
        })
        .mut_subcommands(prepare)
}

fn parser_values(arg: &Arg, value: &str) -> Result<(), String> {
    if template(value) {
        return Ok(());
    }
    let parser = arg.get_value_parser().clone();
    Command::new("documented value")
        .arg(
            Arg::new("value")
                .required(true)
                .allow_hyphen_values(true)
                .value_parser(parser),
        )
        .try_get_matches_from(["documented value", value])
        .map(|_| ())
        .map_err(|error| format!("{}: {}", arg.get_id(), error))
}

fn supplied(arg: &Arg, attached: Option<&str>, argv: &[String], at: &mut usize) -> Vec<String> {
    if !arg.get_action().takes_values() {
        return attached
            .map(|_| format!("{} does not take a value", arg.get_id()))
            .into_iter()
            .collect();
    }
    let mut wrong = Vec::new();
    let maximum = arg.get_num_args().map_or(1, |range| range.max_values());
    let mut count = 0;
    let mut attached = attached;
    while count < maximum {
        let value = if let Some(value) = attached.take() {
            value
        } else if arg.is_require_equals_set() {
            break;
        } else if let Some(value) = argv.get(*at).filter(|value| {
            value.as_str() != "--"
                && (arg.is_allow_hyphen_values_set()
                    || !value.starts_with('-')
                    || value.parse::<f64>().is_ok())
        }) {
            *at += 1;
            value
        } else {
            break;
        };
        for value in value.split(|ch| Some(ch) == arg.get_value_delimiter()) {
            if let Err(error) = parser_values(arg, value) {
                wrong.push(error);
            }
        }
        count += 1;
    }
    wrong
}

fn flag<'a>(command: &'a Command, word: &str) -> Option<&'a Arg> {
    command
        .get_arguments()
        .find(|arg| match word.strip_prefix("--") {
            Some(name) => {
                arg.get_long() == Some(name)
                    || arg
                        .get_all_aliases()
                        .is_some_and(|aliases| aliases.contains(&name))
            }
            None => word
                .strip_prefix('-')
                .filter(|name| name.chars().count() == 1)
                .is_some_and(|name| {
                    let letter = name.chars().next().unwrap();
                    arg.get_short() == Some(letter)
                        || arg
                            .get_all_short_aliases()
                            .is_some_and(|aliases| aliases.contains(&letter))
                }),
        })
}

fn values(program: &Command, argv: &[String]) -> Vec<String> {
    let mut command = program;
    let mut position = 0;
    let mut at = 1;
    let mut wrong = Vec::new();
    while let Some(word) = argv.get(at) {
        at += 1;
        if word == "--" {
            break;
        }
        if let Some(child) = command.find_subcommand(word) {
            command = child;
            position = 0;
            continue;
        }
        if word.starts_with("--") {
            let (name, attached) = word
                .split_once('=')
                .map_or((word.as_str(), None), |(name, value)| (name, Some(value)));
            if let Some(arg) = flag(command, name) {
                wrong.extend(supplied(arg, attached, argv, &mut at));
            }
            continue;
        }
        if let Some(shorts) = word
            .strip_prefix('-')
            .filter(|shorts| !shorts.is_empty() && word.parse::<f64>().is_err())
        {
            for (index, letter) in shorts.char_indices() {
                let Some(arg) = flag(command, &format!("-{letter}")) else {
                    // Unknown names are also checked against the declarations.
                    break;
                };
                let rest = &shorts[index + letter.len_utf8()..];
                if arg.get_action().takes_values() {
                    let attached =
                        (!rest.is_empty()).then(|| rest.strip_prefix('=').unwrap_or(rest));
                    wrong.extend(supplied(arg, attached, argv, &mut at));
                    break;
                } else if let Some(value) = rest.strip_prefix('=') {
                    wrong.extend(supplied(arg, Some(value), argv, &mut at));
                    break;
                }
            }
            continue;
        }
        let mut positional: Vec<_> = command
            .get_arguments()
            .filter(|arg| arg.get_index().is_some())
            .collect();
        positional.sort_by_key(|arg| arg.get_index());
        if let Some(arg) = positional.get(position) {
            if let Err(error) = parser_values(arg, word) {
                wrong.push(error);
            }
            if !arg
                .get_num_args()
                .is_some_and(|range| range.max_values() > 1)
            {
                position += 1;
            }
        }
    }
    wrong
}

fn core_query(example: &examples::Example, maximum: usize) -> Vec<String> {
    let invalid = example.argv.len() > maximum;
    if example.complete && invalid && !example.reject {
        vec!["core query does not take trailing arguments".into()]
    } else if example.complete && !invalid && example.reject {
        vec!["example is marked as rejected but the core accepts it".into()]
    } else {
        Vec::new()
    }
}

/// `argv` without the field flags of the spec its command binds, as the
/// entry splits them (`link_spec::split`); unchanged where it binds none
/// or a flag doesn't split.
fn field_flags_taken(program: &Command, argv: Vec<String>) -> Vec<String> {
    let mut words = Vec::new();
    let mut at = program;
    for word in argv.iter().skip(1) {
        match at.find_subcommand(word) {
            Some(sub) => {
                words.push(word.clone());
                at = sub;
            }
            None => break,
        }
    }
    let path = words.join(" ");
    let Some(kind) = link_cli::registry()
        .values()
        .find_map(|component| component.commands.get(&path).and_then(|c| c.spec.clone()))
    else {
        return argv;
    };
    let Some(binding) = crate::references::kinds::kinds()
        .into_iter()
        .find(|k| k.kind.kind == kind)
        .map(|k| (k.binding)())
    else {
        return argv;
    };
    match link_spec::split(
        argv.iter().map(std::ffi::OsString::from),
        Some(&binding),
        None,
    ) {
        Ok((_, rest)) => rest
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
        Err(_) => argv,
    }
}

fn invocation(programs: &[Command], example: &examples::Example) -> Vec<String> {
    let mut argv = example.argv.clone();
    let delegated = argv.get(1).is_some_and(|word| word == "help");
    if delegated
        && argv
            .get(2)
            .is_none_or(|word| matches!(word.as_str(), "help" | "--help" | "-h"))
    {
        return core_query(example, if argv.len() > 2 { 3 } else { 2 });
    }
    if delegated {
        argv.remove(1);
        argv.push("--help".into());
    }
    let Some(group) = argv.get(1) else {
        return Vec::new();
    };
    if template(group) {
        return Vec::new();
    }
    if !delegated
        && (matches!(
            group.as_str(),
            "--help" | "-h" | "version" | "--version" | "-V" | "nexts"
        ) || [link_cli::REGISTRY_FLAG, link_cli::DOCUMENTATION_FLAG].contains(&group.as_str()))
    {
        return core_query(example, 2);
    }
    let Some(program) = programs.iter().find(|program| {
        program
            .find_subcommand(group)
            .is_some_and(|c| !c.is_hide_set())
    }) else {
        let known = link_cli::registry().values().any(|component| {
            component
                .commands
                .keys()
                .any(|path| path.split_whitespace().next() == Some(group.as_str()))
        });
        return if known
            || link_cli::cli::command().find_subcommand(group).is_some()
            || group.starts_with('-')
            || delegated
        {
            vec![format!("no registered argument parser for {group:?}")]
        } else {
            // Unknown command names and external modules have their own check.
            Vec::new()
        };
    };
    // A command a spec binds takes that spec's field flags, which the
    // entry takes out before clap reads the rest.
    let argv = field_flags_taken(program, argv);
    let mut wrong = values(program, &argv);
    if example.complete {
        let error = match prepare(program.clone()).try_get_matches_from(&argv) {
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                ) =>
            {
                None
            }
            Err(error) => Some(format!("runnable example is not valid: {error}")),
            Ok(_) => None,
        };
        if example.reject {
            return if wrong.is_empty() && error.is_none() {
                vec!["example is marked as rejected but the parser accepts it".into()]
            } else {
                Vec::new()
            };
        }
        if let Some(error) = error {
            wrong.push(error);
        }
    }
    wrong
}

fn programs() -> Vec<Command> {
    crate::parsers::commands()
        .into_iter()
        .map(|mut program| {
            program.build();
            program
        })
        .collect()
}

/// A complete shell example must parse; an inline fragment still checks every literal value it supplies.
pub fn check(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !tree.is_doc(file) {
        return Vec::new();
    }
    static PROGRAMS: LazyLock<Vec<Command>> = LazyLock::new(programs);
    match examples::read(text) {
        Ok(examples) => examples
            .iter()
            .flat_map(|example| {
                invocation(&PROGRAMS, example).into_iter().map(|why| {
                    fail(format!(
                        "{}: argv {:?}: {why}",
                        tree.shown(file),
                        example.argv
                    ))
                })
            })
            .collect(),
        Err(why) => vec![fail(format!("{}: {why}", tree.shown(file)))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{ArgAction, value_parser};

    fn fixture() -> Vec<Command> {
        let mut program = Command::new("lnk").subcommand(
            Command::new("test").alias("try").subcommand(
                Command::new("run")
                    .alias("execute")
                    .arg(Arg::new("target").required(true))
                    .arg(
                        Arg::new("port")
                            .long("port")
                            .short('p')
                            .alias("listen")
                            .short_alias('P')
                            .value_parser(value_parser!(u16).range(1..)),
                    )
                    .arg(Arg::new("json").long("json").action(ArgAction::SetTrue))
                    .arg(Arg::new("verbose").short('v').action(ArgAction::Count))
                    .arg(Arg::new("mode").long("mode").value_parser(["safe", "fast"]))
                    .arg(
                        Arg::new("public")
                            .long("public")
                            .action(ArgAction::SetTrue)
                            .conflicts_with("private"),
                    )
                    .arg(
                        Arg::new("private")
                            .long("private")
                            .action(ArgAction::SetTrue),
                    )
                    .arg(Arg::new("key").long("key").requires("private")),
            ),
        );
        program.build();
        vec![program]
    }

    fn failures(text: &str) -> Vec<String> {
        examples::read(text)
            .unwrap()
            .iter()
            .flat_map(|example| invocation(&fixture(), example))
            .collect()
    }

    #[test]
    fn literal_values_use_the_real_type_enum_and_range() {
        for command in [
            "lnk test run x --port zero",
            "lnk test run x --port 70000",
            "lnk test run x --port 0",
            "lnk test run x --mode quick",
            "lnk test run x -p70000",
            "lnk test run x -vp70000",
            "lnk test run x -vvP=70000",
            "lnk try execute x --listen=70000",
            "lnk test run x --json=true",
        ] {
            assert!(!failures(&format!("`{command}`")).is_empty(), "{command}");
        }
        assert!(failures("`lnk test run x --port 443 --mode safe`").is_empty());
        assert!(failures("`lnk test run <target> --port <port>`").is_empty());
        assert!(failures("`lnk try execute x -vvp443 --json`").is_empty());
        assert!(failures("`lnk test run x -p=443 --listen 443`").is_empty());
    }

    #[test]
    fn runnable_examples_require_arguments_and_obey_conflicts_and_dependencies() {
        for command in [
            "lnk test run",
            "lnk test run x --public --private",
            "lnk test run x --key file",
        ] {
            assert!(
                !failures(&format!("```sh\n{command}\n```\n")).is_empty(),
                "{command}"
            );
        }
        assert!(
            failures("`lnk test run`").is_empty(),
            "inline reference can omit arguments"
        );
        assert!(failures("```sh\nlnk test run <target> --port $PORT\n```\n").is_empty());
        assert!(
            failures("```sh expect=error\nlnk test run x --public --private\n```\n").is_empty()
        );
        assert!(!failures("```sh expect=error\nlnk test run x\n```\n").is_empty());
        assert!(failures("```sh expect=error\nlnk test run x -p70000\n```\n").is_empty());
    }

    #[test]
    fn help_delegates_to_the_owning_parser_and_core_queries_are_valid() {
        let programs = programs();
        for command in [
            "lnk help storage",
            "lnk help storage list",
            "lnk help module",
            "lnk --link-registry",
            "lnk --link-documentation",
            "lnk --version",
        ] {
            for example in examples::read(&format!("```sh\n{command}\n```\n")).unwrap() {
                let problems = invocation(&programs, &example);
                assert!(problems.is_empty(), "{command}: {problems:?}");
            }
        }
        for command in ["lnk storage list", "lnk up"] {
            let example = examples::read(&format!("`{command}`")).unwrap().remove(0);
            assert!(!invocation(&[], &example).is_empty(), "{command}");
        }
        let example = examples::read("`lnk <group> <command>`").unwrap().remove(0);
        assert!(invocation(&[], &example).is_empty());
        for command in [
            "lnk --link-registry extra",
            "lnk --help extra",
            "lnk version extra",
            "lnk help help extra",
            "lnk help no-such-group",
            "lnk help bucket no-such-command",
            "lnk help version",
        ] {
            let example = examples::read(&format!("```sh\n{command}\n```\n"))
                .unwrap()
                .remove(0);
            assert!(!invocation(&programs, &example).is_empty(), "{command}");
        }
    }

    #[test]
    fn runnable_redirections_do_not_change_the_program_arguments() {
        for suffix in [">file", "> file", "2>&1", "< file"] {
            assert!(failures(&format!("```sh\nlnk test run <target> {suffix}\n```\n")).is_empty());
            assert!(
                !failures(&format!("```sh\nlnk test run x -p70000 {suffix}\n```\n")).is_empty()
            );
        }
    }
}

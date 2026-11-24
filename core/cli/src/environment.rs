//! The declared command inputs, never the current process's values.

use std::collections::BTreeMap;

use link_component::{Command, OptionDeclaration};
use link_format::{Done, Out, Report, Table, WithinExt, sentence};
use serde::Serialize;

use crate::{cli, registry::REGISTRY};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct Variable {
    name: String,
    command: String,
    about: String,
}

fn variables<'a>(
    commands: impl IntoIterator<Item = (&'a String, &'a Command)>,
    root: &str,
    selected: &str,
) -> Vec<Variable> {
    let mut rows = Vec::new();
    for (path, command) in commands {
        let path = if path == "root" { root } else { path };
        if !selected.is_empty()
            && path != selected
            && !path.starts_with(&format!("{selected} "))
            && !selected.starts_with(&format!("{path} "))
            && !path.is_empty()
        {
            continue;
        }
        let mut declared: BTreeMap<&str, &OptionDeclaration> = command
            .environment
            .iter()
            .map(|(name, spec)| (name.as_str(), spec))
            .collect();
        for option in command.options.values() {
            if let Some(name) = &option.environment {
                declared.entry(name).or_insert(option);
            }
        }
        for (name, spec) in declared {
            rows.push(Variable {
                name: name.to_owned(),
                command: if path.is_empty() {
                    "lnk".into()
                } else {
                    format!("lnk {path}")
                },
                about: spec
                    .help
                    .as_deref()
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            });
        }
    }
    rows
}

pub fn list(out: &mut Out, asked: &cli::VariableListArgs) -> Result<Done, Report> {
    let selected = asked.command.join(" ");
    let actions = link_component::actions::parse(cli::HELP.toml)
        .within("reading core command declarations")?;
    let mut commands = link_component::actions::commands(&actions);
    link_component::actions::add_variables(&mut commands, cli::VARIABLES)
        .within("reading core variable declarations")?;
    let known = commands.contains_key(&selected)
        || REGISTRY.values().any(|module| {
            module.commands.contains_key(&selected)
                && module.commands.iter().any(|(path, c)| {
                    c.route && (selected == *path || selected.starts_with(&format!("{path} ")))
                })
        });
    if !selected.is_empty() && !known {
        return Err(Report::error(sentence!("That command isn't known.")));
    }
    let mut rows = variables(&commands, "", &selected);
    for module in REGISTRY.values() {
        let routes: Vec<_> = module
            .commands
            .iter()
            .filter(|(_, c)| c.route)
            .map(|(path, _)| path.as_str())
            .collect();
        // Program-wide inputs apply to every command routed to that program.
        if routes.is_empty()
            || (!selected.is_empty()
                && !routes
                    .iter()
                    .any(|path| selected == *path || selected.starts_with(&format!("{path} "))))
        {
            continue;
        }
        let root = routes
            .iter()
            .copied()
            .find(|path| {
                selected.is_empty()
                    || selected == *path
                    || selected.starts_with(&format!("{path} "))
            })
            .unwrap_or(&module.component.id);
        let commands = module.commands.iter().filter(|(path, _)| {
            path.as_str() == "root"
                || routes
                    .iter()
                    .any(|route| path.as_str() == *route || path.starts_with(&format!("{route} ")))
        });
        rows.extend(variables(commands, root, &selected));
    }
    // A variable a module reads but another defines takes its definition's
    // words.
    let mut defined: BTreeMap<String, String> = BTreeMap::new();
    for root in REGISTRY
        .values()
        .filter_map(|module| module.commands.get("root"))
        .chain(commands.get("root"))
    {
        for (name, spec) in &root.environment {
            if let Some(help) = &spec.help {
                defined.insert(
                    name.clone(),
                    help.split_whitespace().collect::<Vec<_>>().join(" "),
                );
            }
        }
    }
    for row in rows.iter_mut().filter(|row| row.about.is_empty()) {
        if let Some(about) = defined.get(&row.name) {
            row.about.clone_from(about);
        }
    }
    rows.sort();
    rows.dedup();
    if asked.json {
        out.reply(&rows);
    } else {
        let mut table = Table::new(
            ["Variable", "Command", "About"],
            sentence!("No environment variables are declared for these commands."),
        );
        for row in rows {
            table.row([row.name, row.command, row.about]);
        }
        out.block(table);
    }
    Ok(Done::shown())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_includes_program_inputs_and_option_inputs_once() {
        let mut commands = link_component::actions::commands(
            &toml::from_str(
                r#"
[root]
help = "Program"
role = "secondary"
["x start"]
help = "Start"
role = "secondary"
["x start".args.port]
type = "string"
help = "Port input"
long = "port"
environment = "LNK_PORT"
runtime = true
["x stop"]
help = "Stop"
role = "secondary"
"#,
            )
            .unwrap(),
        );
        link_component::actions::add_variables(
            &mut commands,
            "schema = 1\n[defines.LNK_GLOBAL]\ntype = \"string\"\ndescription = \"Program input\"\n",
        )
        .unwrap();
        let rows = variables(&commands, "x", "x start");
        assert_eq!(
            rows.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            ["LNK_GLOBAL", "LNK_PORT"]
        );
        assert_eq!(rows[0].about, "Program input");
        assert_eq!(rows[0].command, "lnk x");
        assert_eq!(rows[1].command, "lnk x start");
    }
}

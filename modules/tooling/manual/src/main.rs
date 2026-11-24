//! The optional manual consumes the core's registry and installed answers.

use std::{
    collections::{BTreeMap, BTreeSet},
    process::Stdio,
};

use link_component::{Page, Resolved};
use link_format::{
    Done, Exit, Out, Product, Report, Verbatim, WithinExt, cmd, phrase, prose, sentence,
};
use link_manual::cli::{self, Command};

fn main() -> std::process::ExitCode {
    link_secretive::process::guard();
    link_core::describe_component!();
    // It declares it reads no control (its module.toml), so it renders
    // in the default look and keeps no records: reading the settings that
    // say otherwise would be reading control.
    link_format::run_with(
        link_format::Machine::new(Default::default(), None),
        try_main,
    )
}

/// What the core answers `request` with, asked stating this program's
/// description protocol, which the core checks.
fn core_answer(request: &str) -> Result<Vec<u8>, Report> {
    let output = link_component::asking(&mut link_core::lnk())
        .arg(request)
        .stdin(Stdio::null())
        .output()
        .within("asking the core")?;
    if !output.status.success() {
        return Err(
            Report::error(sentence!("The core could not answer {}.", request))
                .why(prose!(
                    "It said: {}.",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
                .next(
                    cmd!("lnk module list"),
                    phrase!("check the installed modules"),
                ),
        );
    }
    Ok(output.stdout)
}

fn topic(value: &str) -> Result<&str, Report> {
    let canonical = !value.starts_with('/')
        && !value.ends_with('/')
        && !value.is_empty()
        && value.split('/').all(|s| {
            !s.is_empty()
                && s != "."
                && s != ".."
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        });
    if !canonical {
        return Err(Report::error(sentence!(
            "A topic is a canonical documentation path, such as services/boxes."
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk manual"), phrase!("list the topics")));
    }
    Ok(value)
}

/// The refusal of a canonical path that names no topic.
fn unknown_topic(path: &str) -> Report {
    Report::error(sentence!("There is no documentation topic {}.", path))
        .exit(Exit::Usage)
        .next(cmd!("lnk manual"), phrase!("list the topics"))
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    let Command::ManualShow(cli) = cli::parse();
    let registry: BTreeMap<String, Resolved> =
        link_component::parse_registry(&core_answer("--link-registry")?)
            .within("reading the core's registry")?;
    let mut local: Vec<Page> =
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/manual.json")))
            .within("reading its own pages")?;
    local.extend(
        serde_json::from_slice::<Vec<Page>>(&core_answer("--link-documentation")?)
            .within("reading the core's answer")?,
    );
    let own = Resolved::parse(include_str!(concat!(env!("OUT_DIR"), "/component.json")))
        .within("reading its own description")?;
    local.extend(own.pages);
    // The reference flags declare that they need a path.
    let Some(path) = cli.path.as_deref() else {
        // The front page, the website's `/`, then every topic.
        if let Some(front) = local.iter().find(|p| p.path.is_empty()) {
            out.data(format!("{}\n", front.markdown).as_bytes());
        }
        let paths: BTreeSet<_> = local
            .iter()
            .map(|p| p.path.as_str())
            .chain(
                registry
                    .values()
                    .flat_map(|d| d.pages.iter().map(|p| p.path.as_str())),
            )
            .filter(|path| !path.is_empty())
            .collect();
        for path in paths {
            out.data(format!("{path}\n").as_bytes());
        }
        return Ok(Done::shown().next(cmd!("lnk manual <topic>"), phrase!("read a topic")));
    };
    let path = topic(path)?;
    let owner = registry.values().find(|d| {
        d.pages.iter().any(|p| p.path == path)
            || d.documentation
                .as_ref()
                .is_some_and(|doc| doc.topic == path)
    });
    if cli.commands || cli.command.is_some() || cli.variables {
        let descriptor = owner.ok_or_else(|| {
            Report::error(sentence!("The topic {} has no component reference.", path))
                .exit(Exit::Usage)
                .next(
                    cmd!("lnk manual {}", path),
                    phrase!("read the topic itself"),
                )
        })?;
        if cli.variables {
            print_variables(out, descriptor);
        } else {
            print_commands(out, descriptor, path, cli.command.as_deref())?;
        }
        return Ok(Done::shown());
    }
    if let Some(page) = local.iter().find(|p| p.path == path) {
        out.data(page.markdown.as_bytes());
        return Ok(Done::shown());
    }
    let descriptor = owner.ok_or_else(|| unknown_topic(path))?;
    if let Some(program) = link_core::find_module(&descriptor.component.id) {
        let output = link_component::asking(&mut link_secretive::process::command(&program))
            .arg("--link-describe")
            .stdin(Stdio::null())
            .output()
            .within("asking the installed module for its pages")?;
        if !output.status.success() {
            return Err(Report::error(sentence!(
                "The module {} could not describe its pages.",
                Product::new(descriptor.component.program.clone())
            ))
            .next(cmd!("lnk upgrade"), phrase!("bring the module up to date")));
        }
        let installed = Resolved::parse(
            std::str::from_utf8(&output.stdout).within("reading the module's description")?,
        )
        .within("reading the module's description")?;
        if installed.component.id != descriptor.component.id {
            return Err(Report::error(sentence!(
                "The installed program described another component."
            ))
            .next(
                cmd!("lnk module add {}", descriptor.component.id.as_str()),
                phrase!("install the right module"),
            ));
        }
        if let Some(page) = installed.pages.iter().find(|p| p.path == path) {
            out.data(page.markdown.as_bytes());
            return Ok(Done::shown());
        }
        return Err(Report::error(sentence!(
            "The installed {} has no {} page.",
            Product::new(descriptor.component.program.clone()),
            path
        ))
        .why(prose!(
            "It is on the website at {}{}.",
            link_core::docs!(""),
            path
        ))
        .next(cmd!("lnk upgrade"), phrase!("bring the module up to date")));
    }
    out.data(
        format!(
            "{}: {}\n",
            descriptor.component.id, descriptor.component.description
        )
        .as_bytes(),
    );
    out.block(
        Verbatim::new()
            .lead(phrase!("Install it"))
            .line(format!("lnk module add {}", descriptor.component.id)),
    );
    out.data(format!("{}{path}\n", link_core::docs!("")).as_bytes());
    Ok(Done::shown().next(
        cmd!("lnk module add {}", descriptor.component.id.as_str()),
        phrase!("install it"),
    ))
}

fn print_commands(
    out: &mut Out,
    d: &Resolved,
    topic: &str,
    asked: Option<&str>,
) -> Result<(), Report> {
    let matches: Vec<_> = d
        .commands
        .iter()
        .filter(|(name, _)| {
            asked.is_none_or(|asked| *name == asked || name.ends_with(&format!(" {asked}")))
        })
        .collect();
    if matches.is_empty() {
        return Err(Report::error(sentence!(
            "{} has no command {}.",
            d.component.id.as_str(),
            asked.unwrap_or_default()
        ))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk manual {} --commands", topic),
            phrase!("list its commands"),
        ));
    }
    if asked.is_some() && matches.len() != 1 {
        return Err(Report::error(sentence!(
            "That command is ambiguous: give its complete name."
        ))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk manual {} --commands", topic),
            phrase!("list its commands"),
        ));
    }
    for (name, command) in matches {
        let invocation =
            if name == "root" {
                d.component.program.clone()
            } else if d.commands.iter().any(|(route, c)| {
                c.route && (name == route || name.starts_with(&format!("{route} ")))
            }) {
                format!("lnk {name}")
            } else {
                format!("{} {name}", d.component.program)
            };
        let mut text = format!("{invocation}\n{}\n", command.help.trim());
        for (flag, help) in &command.flags {
            text.push_str(&format!("  {flag}  {help}\n"));
        }
        text.push('\n');
        out.data(text.as_bytes());
    }
    Ok(())
}

fn print_variables(out: &mut Out, d: &Resolved) {
    let mut variables = BTreeMap::new();
    for command in d.commands.values() {
        variables.extend(command.environment.iter());
        for option in command.options.values() {
            if let Some(name) = &option.environment {
                variables.entry(name).or_insert(option);
            }
        }
    }
    for (name, option) in variables {
        out.data(format!("{name}\n  {}\n", option.help.as_deref().unwrap_or_default()).as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_refusal_lists_commands_at_the_requested_topic() {
        let descriptor: Resolved = serde_json::from_value(serde_json::json!({
            "protocol": 1,
            "component": {
                "id": "example", "cargo_name": "link-example", "program": "lnk-example",
                "kind": "module", "version": "1.0.0", "license": "MIT", "description": "Example"
            },
            "commands": {
                "example show": { "help": "Show the example." },
                "example nested show": { "help": "Show the nested example." }
            }
        }))
        .unwrap();
        for asked in ["missing", "show"] {
            let (mut out, capture) = Out::capture(false, &["lnk", "manual"]);
            let result = print_commands(
                &mut out,
                &descriptor,
                "agents/harnesses/example",
                Some(asked),
            );
            assert_eq!(out.finish(result.map(|()| Done::shown())), 2);
            assert!(capture.stdout().is_empty());
            assert!(
                capture
                    .stderr()
                    .contains("lnk manual agents/harnesses/example --commands"),
                "{}",
                capture.stderr()
            );
        }
    }

    #[test]
    fn paths_are_topics_and_never_command_shorthand_or_traversal() {
        assert_eq!(topic("services/boxes").unwrap(), "services/boxes");
        for bad in [
            "",
            "/services/boxes",
            "services/../x",
            "services//x",
            "box create",
            "services/boxes/",
        ] {
            assert!(topic(bad).is_err(), "{bad}");
        }
    }
}

#[cfg(test)]
link_test_kit::program!(unit);

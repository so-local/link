//! A module's command reference: each command it offers on the command
//! line, grouped by resource, with its usage, its runtime arguments, the
//! spec it reads and what it answers.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
};

use link_actions::{Action, Arg, Catalog, Module, Role, Surface};

use super::{
    folder, kind,
    page::{Page, Section},
    relative, title,
};
use crate::{Files, config::SETTINGS};

/// The module's page, by its path from `root`: every command it offers,
/// under a heading for the resource each acts on when it has several.
pub(super) fn generate(catalog: &Catalog, module: &Module, root: &Path) -> anyhow::Result<Files> {
    let mut files = Files::new();
    let folder = folder(module, root)?;
    let path = folder.join(page(module, &folder));
    let id = &module.identity.id;
    // Each resource's commands, in order, each with its heading and body.
    let mut resources: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for action in module.actions.values() {
        if !action.offered_on(Surface::Cli) || action.role == Role::Plumbing {
            continue;
        }
        let resource = action.id.resource().to_string();
        let held = (
            format!("`{}`", action.id.command()),
            command(catalog, root, &path, action)?,
        );
        match resources.last_mut() {
            Some((last, commands)) if *last == resource => commands.push(held),
            _ => resources.push((resource, vec![held])),
        }
    }
    if resources.is_empty() {
        return Ok(files);
    }
    let sections = match resources.len() {
        // One resource needs no heading of its own: its commands are the
        // page's sections.
        1 => resources
            .into_iter()
            .flat_map(|(_, commands)| commands)
            .map(|(heading, body)| Section { heading, body })
            .collect(),
        _ => resources
            .into_iter()
            .map(|(resource, commands)| Section {
                heading: title(&resource),
                body: commands
                    .iter()
                    .map(|(heading, body)| format!("### {heading}\n\n{}\n", body.trim_end()))
                    .collect::<Vec<_>>()
                    .join("\n"),
            })
            .collect(),
    };
    Page {
        title: format!("{} Commands", title(id)),
        opening: format!(
            "Every command the `{id}` module offers, from its declarations, in order of the \
             resource each acts on. Each command's help says the same."
        ),
        sections,
        merged: None,
        preface: aliases(module),
    }
    .write(&path, &mut files)?;
    Ok(files)
}

/// The module's aliases, from its `[aliases]`, each group's full command
/// first and then its short form: none where it declares none.
fn aliases(module: &Module) -> Vec<(String, String)> {
    if module.aliases.is_empty() {
        return Vec::new();
    }
    let program = &link_actions::core().program;
    let said: Vec<String> = module
        .aliases
        .iter()
        .map(|(group, alias)| {
            let short = match group.rsplit_once(' ') {
                Some((parent, _)) => format!("{parent} {alias}"),
                None => alias.clone(),
            };
            format!("`{program} {group}` is typed short as `{program} {short}`.")
        })
        .collect();
    vec![(
        "Aliases".to_string(),
        format!(
            "A group's alias is a word a person may type in place of its own, \
             declared in the module's `[aliases]`. {}\n",
            said.join(" ")
        ),
    )]
}

/// The page linking every module's command reference, from the workspace's
/// root: the page above them, which introduces each one.
/// The hub, linking each domain's hub and each module with no domain, and
/// each domain's hub, linking its modules' pages: `pages` is each module's
/// ID, domain, and page, and a domain with fewer than `least` modules has
/// no hub, its modules linked from the hub.
pub(super) fn hub(
    pages: &[(String, Option<String>, PathBuf)],
    least: usize,
) -> anyhow::Result<Files> {
    let mut files = Files::new();
    let path = PathBuf::from(&SETTINGS.reference.hub);
    if pages.is_empty() {
        return Ok(files);
    }
    let mut domains: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut links = Vec::new();
    // A domain with fewer modules than a group holds has no hub of its own:
    // its modules are linked from the hub, as a module with no domain is.
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, domain, _) in pages {
        if let Some(domain) = domain {
            *counts.entry(domain).or_default() += 1;
        }
    }
    for (id, domain, page) in pages {
        let domain = domain.as_deref().filter(|d| counts[d] >= least);
        match domain {
            Some(domain) => {
                let at = domain_hub(domain);
                domains.entry(domain).or_default().push(format!(
                    "[{}]({})",
                    title(id),
                    relative(&at, page)
                ));
            }
            None => links.push(format!("[{}]({})", title(id), relative(&path, page))),
        }
    }
    for (domain, modules) in &domains {
        let at = domain_hub(domain);
        links.push(format!("[{}]({})", title(domain), relative(&path, &at)));
        files.insert(
            &at,
            format!(
                "<!-- {} -->\n# {} Modules\n\n> {}\n\nThe commands of each {domain} module, \
                 from its declarations, with their usage, runtime arguments, specifications, \
                 and answers: {}.\n",
                crate::marker(),
                title(domain),
                SETTINGS.reference.domain_hook,
                super::sentence(modules)
            ),
        )?;
    }
    links.sort();
    files.insert(
        &path,
        format!(
            "<!-- {} -->\n# Module Commands\n\n> {}\n\nEach module's commands, from its \
             declarations, with their usage, runtime arguments, specifications, and answers, on its \
             own page or on its domain's where a domain has several: {}.\n",
            crate::marker(),
            SETTINGS.reference.hub_hook,
            super::sentence(&links)
        ),
    )?;
    // Its domains' hubs are its own, so one with no module left is removed.
    files.own(path.with_extension(""));
    Ok(files)
}

/// The hub of `domain`'s modules, beside the hub.
fn domain_hub(domain: &str) -> PathBuf {
    PathBuf::from(&SETTINGS.reference.hub)
        .with_extension("")
        .join(format!("{domain}.md"))
}

/// The domain of the module in `folder`, from the root: the folder after
/// `modules` or `adapters`, if it's in one.
pub(super) fn domain(folder: &Path) -> Option<String> {
    let mut parts = folder.iter().filter_map(|p| p.to_str());
    let first = parts.next()?;
    let domain = parts.next()?;
    (SETTINGS.reference.domains.iter().any(|d| d == first) && parts.next().is_some())
        .then(|| domain.to_string())
}

/// The module's page, from its folder: under its domain's, if it has one.
pub(super) fn page(module: &Module, folder: &Path) -> PathBuf {
    let commands = PathBuf::from(&SETTINGS.reference.commands);
    let commands = match domain(folder) {
        Some(domain) => commands.join(domain),
        None => commands,
    };
    commands.join(format!("{}.md", module.identity.id))
}

fn command(catalog: &Catalog, root: &Path, page: &Path, action: &Action) -> anyhow::Result<String> {
    // The description's first paragraph: the rest is its help's.
    let description = action.description.trim();
    let first = description.split("\n\n").next().unwrap_or(description);
    let mut out = format!(
        "{}\n\n",
        super::code(&first.split_whitespace().collect::<Vec<_>>().join(" "))
    );
    // A synopsis, not a command to run: its fence is not a terminal's.
    writeln!(
        out,
        "```{}\n{}\n```\n",
        SETTINGS.reference.synopsis,
        usage(action)
    )?;
    // One paragraph closes the section: its arguments when they're too few
    // for a list, the spec it reads, and what it answers.
    let mut closing = Vec::new();
    let arguments = arguments(action);
    if arguments.len() >= 3 {
        for arg in &arguments {
            writeln!(out, "- {arg}")?;
        }
        out.push('\n');
    } else {
        closing.extend(arguments.iter().map(|arg| format!("It takes {arg}.")));
    }
    if let Some(spec) = &action.spec {
        let declared = catalog.shape(spec);
        let kind = declared.and_then(|d| d.spec.clone());
        closing.push(match (kind, catalog.module(spec.module())) {
            (Some(kind), Some(owner)) => {
                let to = folder(owner, root)?.join(super::specs::page(&kind));
                format!(
                    "It reads the [`{kind}` specification]({}), from a file or its field flags.",
                    relative(page, &to)
                )
            }
            _ => format!("It reads the specification `{spec}`."),
        });
    }
    closing.push(format!("It answers {}.", kind(&action.output)));
    writeln!(out, "{}", closing.join(" "))?;
    Ok(out)
}

/// The command as typed, its positionals in order and its flags after.
fn usage(action: &Action) -> String {
    let mut words = vec![action.id.command()];
    for (name, arg) in action.positionals() {
        words.push(if arg.required {
            format!("<{name}>")
        } else {
            format!("[{name}]")
        });
    }
    for (name, arg) in &action.args {
        if arg.cli.position.is_some() || arg.cli.hidden {
            continue;
        }
        if arg.cli.rest {
            words.push(format!("[-- <{name}>...]"));
            continue;
        }
        let flag = flag(name, arg);
        words.push(if arg.required {
            flag
        } else {
            format!("[{flag}]")
        });
    }
    // A bound action's spec file, written once: its own declared `spec`
    // above where it declares one, as its parser takes it.
    if action.spec.is_some() || action.held.is_some() {
        match action.args.contains_key(&link_actions::core().spec) {
            true => words.push("[field flags]".into()),
            false => words.push("[--spec <file>] [field flags]".into()),
        }
    }
    words.join(" ")
}

fn flag(name: &str, arg: &Arg) -> String {
    match arg.schema.node {
        link_actions::schema::Node::Boolean => format!("--{name}"),
        _ => format!("--{name} <{name}>"),
    }
}

/// A line per runtime argument: how it's typed, its type and its meaning.
/// One kept out of help is kept out here too.
fn arguments(action: &Action) -> Vec<String> {
    action
        .args
        .iter()
        .filter(|(_, arg)| !arg.cli.hidden)
        .map(|(name, arg)| {
            let mut parts = vec![match (arg.cli.position, arg.cli.rest) {
                (Some(position), _) => format!("`{name}`, position {position}"),
                (None, true) => format!("`{name}`, after `--`"),
                (None, false) => format!("`--{name}`"),
            }];
            if let Some(short) = arg.cli.short {
                parts.push(format!("`-{short}`"));
            }
            parts.push(kind(&arg.schema));
            if arg.required {
                parts.push("required".into());
            }
            if let Some(default) = &arg.schema.meta.default {
                parts.push(format!("default `{}`", super::plain(default)));
            }
            if let Some(variable) = &arg.cli.env {
                parts.push(format!("else `{variable}`"));
            }
            let description = arg
                .schema
                .meta
                .description
                .as_deref()
                .map(|d| {
                    let d = d.split_whitespace().collect::<Vec<_>>().join(" ");
                    format!(": {}", super::code(d.trim_end_matches('.')))
                })
                .unwrap_or_default();
            format!("{}{description}", parts.join(", "))
        })
        .collect()
}

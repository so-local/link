//! Each spec kind's reference page and its JSON Schema, in the one layout
//! `link_spec::reference` writes: its title, how to use it, Every Option
//! as a checked TOML example, then an H2 per top-level key and an H3 per
//! field, each opening with a quoted line of its type, values, default
//! and flag, and its description below.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use link_actions::{Catalog, Module, TypeName};
use link_spec::reference::{Composed, every_option_of, key_sections};

use super::{
    folder,
    page::{Page, Section},
    title,
};
use crate::{Files, config::SETTINGS};

/// Each spec kind the module declares: its page and its JSON Schema.
pub(super) fn generate(catalog: &Catalog, module: &Module, root: &Path) -> anyhow::Result<Files> {
    let mut files = Files::new();
    let folder = folder(module, root)?;
    for (name, declared) in &module.types {
        let Some(kind) = &declared.spec else {
            continue;
        };
        let name = TypeName::new(&module.identity.id, name)?;
        let path = folder.join(page(kind));
        let schema = catalog.json_schema(&name)?;
        let version = module
            .schemas
            .get(kind)
            .map(|versions| versions.format.current)
            .ok_or_else(|| anyhow::anyhow!("spec kind {kind} has no schema.toml"))?;
        let every = every_option_of(kind, version, &schema);
        let others = held(catalog, &name, &path, root)?;
        let composed: Vec<Composed<'_>> = others
            .iter()
            .map(|(shape, kind, page)| Composed { shape, kind, page })
            .collect();
        let sections = key_sections(&schema, &composed)
            .into_iter()
            .map(|(key, body)| Section {
                heading: format!("`{key}`"),
                body,
            })
            .collect();
        files.insert(
            path.with_extension("schema.json"),
            crate::shapes::json_text(crate::files::stamp(schema))?,
        )?;
        let description = declared
            .schema
            .meta
            .description
            .as_deref()
            .map(|d| format!("{}. ", d.trim().trim_end_matches('.')))
            .unwrap_or_default();
        let mut preface: Vec<(String, String)> =
            use_it(catalog, &name, kind, root).into_iter().collect();
        preface.push((
            "Every Option".into(),
            format!("Every field the reader takes, with an example value.\n\n```toml\n{every}```"),
        ));
        Page {
            title: format!("{} Specification", title(kind)),
            opening: format!(
                "{description}The `{kind}` specification is TOML, given to a command that reads it as a \
                 file, and a field flag sets the field it names, overriding the file. Its JSON \
                 Schema is [`{kind}.schema.json`]({kind}.schema.json)."
            ),
            sections,
            merged: None,
            preface,
        }
        .write(&path, &mut files)?;
    }
    Ok(files)
}

/// Every other spec kind, which a field of this one may hold whole: its
/// JSON Schema, its kind and its page as a link from `here`, so such a
/// field links that page rather than writing its fields out again.
fn held(
    catalog: &Catalog,
    name: &TypeName,
    here: &Path,
    root: &Path,
) -> anyhow::Result<Vec<(serde_json::Value, String, String)>> {
    let mut out = Vec::new();
    for module in catalog.modules() {
        for (other, declared) in &module.types {
            let Some(kind) = &declared.spec else {
                continue;
            };
            let other = TypeName::new(&module.identity.id, other)?;
            if &other == name || !module.directory.starts_with(root) {
                continue;
            }
            let target = folder(module, root)?.join(page(kind));
            out.push((
                catalog.json_schema(&other)?,
                kind.clone(),
                super::relative(here, &target),
            ));
        }
    }
    Ok(out)
}

/// The section saying how a command takes the kind, under the heading
/// core's spec commands link: the commands reading it, and its owner's
/// `spec show`. A kind a package owns, the package owning no command, is
/// shown by each module that binds it, and one no command reads yet has
/// no such section.
fn use_it(catalog: &Catalog, name: &TypeName, kind: &str, root: &Path) -> Option<(String, String)> {
    let core = link_actions::core();
    let anchor = core.spec_docs.rsplit('#').next().unwrap_or_default();
    // The commands a person types, of the workspace the page is in: no
    // plumbing, no action off the command line, and none a page here
    // can't name.
    let readers: Vec<String> = crate::offered(catalog, link_actions::Surface::Cli)
        .filter(|action| action.role != link_actions::Role::Plumbing)
        .filter(|action| {
            catalog
                .module(action.id.namespace())
                .is_some_and(|module| module.directory.starts_with(root))
        })
        .filter(|action| action.spec.as_ref() == Some(name))
        .map(|action| format!("`{}`", action.id.command()))
        .collect();
    // A module's second kind is shown under its own word (`agent team spec
    // show`), as the view names it. A package's kind, its owner having no
    // command, is shown by each module that binds it.
    let shown_by: Vec<&str> = match link_actions::package_kind(catalog, name) {
        Some(_) => catalog
            .actions()
            .filter(|action| action.spec.as_ref() == Some(name))
            .map(|action| action.id.namespace())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        None => catalog
            .module(name.module())
            .filter(|owner| !owner.actions.is_empty())
            .map(|_| name.module())
            .into_iter()
            .collect(),
    };
    let shows: Vec<String> = shown_by
        .iter()
        .map(|module| {
            format!(
                "`{} {module} {}`",
                core.program,
                link_actions::spec_show(module, kind).join(" ")
            )
        })
        .collect();
    let show = (!shows.is_empty()).then(|| {
        let verb = if shows.len() == 1 {
            "prints"
        } else {
            "each print"
        };
        format!("{} {verb}", super::sentence(&shows))
    });
    let given = format!(
        "Give {} a `{kind}` specification as a file, and set any field below with its field flag, \
         which overrides the file.",
        super::sentence(&readers)
    );
    let body = match (readers.is_empty(), show) {
        (true, None) => return None,
        (true, Some(show)) => format!("{show} a `{kind}` specification as it is read."),
        (false, None) => given,
        (false, Some(show)) => format!("{given} {show} the specification as it is read."),
    };
    Some((title(anchor), body))
}

/// A spec kind's page, from its module's folder.
pub(super) fn page(kind: &str) -> PathBuf {
    PathBuf::from(&SETTINGS.reference.specs).join(format!("{kind}.md"))
}

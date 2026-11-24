//! The variable index: every environment variable the workspace's crates
//! define, modules, packages and contracts alike, by the crate defining
//! it, with its type, its default and its meaning, and each namespace a
//! crate computes names in.

use std::{fmt::Write as _, path::Path};

use link_actions::{Catalog, Crates, Scalar};

use super::{
    page::{Page, Section},
    title,
};
use crate::{Files, config::SETTINGS};

/// The variable index for the crates in `root`, each crate's
/// `variables.toml` its one source.
pub(super) fn generate(catalog: &Catalog, root: &Path) -> anyhow::Result<Files> {
    let mut files = Files::new();
    let mut crates = Crates::load(&[root])?;
    // A module's declarations are read whether or not a crate is beside
    // them, as the catalog reads them.
    for module in catalog.modules().filter(|m| m.directory.starts_with(root)) {
        let here = module.directory.canonicalize().ok();
        let known = crates
            .crates
            .keys()
            .any(|directory| directory.canonicalize().ok() == here);
        if !known {
            crates
                .crates
                .insert(module.directory.clone(), module.variables.clone());
        }
    }
    let mut sections = Vec::new();
    for (directory, variables) in &crates.crates {
        let owner = owner(catalog, directory)?;
        let mut body = String::new();
        for (name, definition) in &variables.defines {
            let mut parts = vec![
                format!("`{name}`"),
                match definition.kind {
                    Scalar::String => "string",
                    Scalar::Boolean => "boolean",
                    Scalar::Integer => "integer",
                }
                .to_string(),
            ];
            if let Some(default) = &definition.default {
                parts.push(format!("default `{}`", toml_plain(default)));
            }
            if let Some(carries) = &definition.carries {
                parts.push(format!("carries the secret `{}`", carries.secret));
            }
            parts.push(format!("from `{owner}`"));
            write!(
                body,
                "- {}: {}",
                parts.join(", "),
                definition.description.trim()
            )?;
            if let Some(suffix) = &definition.suffix {
                write!(body, " Its `*`: {}", suffix.trim())?;
            }
            body.push('\n');
        }
        for (pattern, description) in &variables.namespaces {
            writeln!(
                body,
                "- `{pattern}`, a namespace, from `{owner}`: {}",
                description.trim()
            )?;
        }
        if !body.is_empty() {
            sections.push(Section {
                heading: title(&owner),
                body,
            });
        }
    }
    if sections.is_empty() {
        return Ok(files);
    }
    Page {
        title: "Variables".into(),
        opening: "Every environment variable the workspace's crates define, modules, packages \
                  and contracts alike, from their `variables.toml`, by the crate defining it, \
                  and each namespace a crate computes names in. A runtime flag may name one to \
                  read when it is absent, as its command's reference says, and a specification \
                  field never reads one."
            .into(),
        sections,
        merged: Some((
            "Defined".into(),
            "Each variable with its type, its default, and the crate defining it.".into(),
        )),
        preface: Vec::new(),
    }
    .write(Path::new(&SETTINGS.reference.variables), &mut files)?;
    Ok(files)
}

/// The crate in `directory` as the index names it: its module's ID, or
/// its package's name.
fn owner(catalog: &Catalog, directory: &Path) -> anyhow::Result<String> {
    let here = directory.canonicalize().ok();
    if let Some(module) = catalog
        .modules()
        .find(|m| m.directory.canonicalize().ok() == here)
    {
        return Ok(module.identity.id.clone());
    }
    let manifest = directory.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)?;
    let value: toml::Value = toml::from_str(&text)?;
    value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("{} names no package", manifest.display()))
}

fn toml_plain(value: &toml::Value) -> String {
    match value {
        toml::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

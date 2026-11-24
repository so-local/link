//! The spec kinds installed modules export, found from their
//! descriptions: each kind's owner, the actions consuming it and its
//! owner's history.

use anyhow::{Context, bail};
use link_component::Resolved;
use link_spec::Exported;
use link_spec::history::Gone;

/// A spec kind and what's known of it from its owner's description.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Owned {
    /// The module that owns it.
    pub module: String,
    pub kind: Exported,
    /// The actions that consume it, by their words.
    pub consumers: Vec<String>,
    /// Its owner's retired spellings.
    pub history: Vec<Gone>,
}

/// Every kind `descriptions` export, by name, and their consumers among
/// every module's actions. Two modules exporting one kind is refused.
pub fn owned(descriptions: &[Resolved]) -> anyhow::Result<Vec<Owned>> {
    let mut owned: Vec<Owned> = Vec::new();
    for description in descriptions {
        for kind in &description.kinds {
            if let Some(other) = owned.iter().find(|o| o.kind.kind == kind.kind) {
                bail!(
                    "the {} spec kind is exported by both {} and {}",
                    kind.kind,
                    other.module,
                    description.component.id
                );
            }
            owned.push(Owned {
                module: description.component.id.clone(),
                kind: kind.clone(),
                consumers: Vec::new(),
                history: description.history.gone.clone(),
            });
        }
    }
    for description in descriptions {
        for (path, command) in &description.commands {
            if let Some(bound) = &command.spec
                && let Some(kind) = owned.iter_mut().find(|o| o.kind.kind == *bound)
            {
                kind.consumers.push(format!("lnk {path}"));
            }
        }
    }
    for kind in &mut owned {
        kind.consumers.sort();
    }
    owned.sort_by(|a, b| a.kind.kind.cmp(&b.kind.kind));
    Ok(owned)
}

/// The kind a spec file names, by its top-level `kind`.
pub fn kind_of(text: &str) -> anyhow::Result<Option<String>> {
    let table: toml::Table = toml::from_str(text).context("reading the spec as TOML")?;
    match table.get("kind") {
        Some(toml::Value::String(kind)) => Ok(Some(kind.clone())),
        Some(_) => bail!("its kind is a spec kind's name, as kind = \"agent\""),
        None => Ok(None),
    }
}

/// What a file naming no kind is told: its kind is never guessed from
/// what's in it, so it names one, from those installed.
pub fn no_kind(owned: &[Owned]) -> String {
    let names: Vec<&str> = owned.iter().map(|o| o.kind.kind.as_str()).collect();
    format!(
        "it names no kind: add its kind's name at its top, as kind = \"{}\", one of {}",
        names.first().copied().unwrap_or("agent"),
        if names.is_empty() {
            "none installed".to_string()
        } else {
            names.join(", ")
        }
    )
}

#[cfg(test)]
mod tests;

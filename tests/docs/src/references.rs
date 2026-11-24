//! The workspace's generated references: each module's command reference
//! and spec pages, the variable index and `lnk`'s shell completion, written by
//! `link-actions-surfaces` from the modules' declarations and held fresh
//! here. `lnk-docs-tree --write` writes each absent or stale one.

use std::path::Path;

use link_actions::Catalog;
use link_actions_surfaces::{Files, completion, modules, reference};
use serde::Deserialize;

use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[Rule {
    id: "references",
    scope: Scope::Site,
    fix: "a generated reference is absent, stale, or edited by hand: change the module's \
          declarations, then run lnk-docs-tree --write",
    check: Check::Whole(check),
}];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Held {
    modules: Vec<String>,
}

/// The workspace's modules still held, by their paths from it.
pub fn held() -> anyhow::Result<Vec<String>> {
    let held: Held = toml::from_str(include_str!("../held.toml"))?;
    Ok(held.modules)
}

/// The workspace's modules but those held, loaded.
pub fn catalog(root: &Path) -> anyhow::Result<Catalog> {
    let folders = modules::discover(root, &held()?)?;
    let folders: Vec<&Path> = folders.iter().map(|f| f.as_path()).collect();
    Catalog::load(&folders)
}

/// Every reference page and completion script the workspace at `root`
/// has, generated: none
/// where `root` is no Cargo workspace, such as a checkout of workspaces.
pub fn files(root: &Path) -> anyhow::Result<Files> {
    if !root.join("Cargo.toml").is_file() {
        return Ok(Files::new());
    }
    let catalog = catalog(root)?;
    let mut files = reference::generate(&catalog, root, crate::Limits::read().of("group-pages"))?;
    files.extend(Path::new(""), completion::generate(&catalog, root)?)?;
    Ok(files)
}

fn check(tree: &Tree) -> Vec<Finding> {
    match files(&tree.root).and_then(|files| files.problems(&tree.root)) {
        Ok(problems) => problems.iter().map(|p| fail(p.to_string())).collect(),
        Err(error) => vec![fail(format!(
            "the references can't be generated: {error:#}"
        ))],
    }
}

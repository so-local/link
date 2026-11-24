//! The suggested command structure: each verb a module's commands start
//! with (`lnk agent move`, `lnk box create`) has a page or folder of its
//! own in its module's `docs/`. A suggestion for clarity, never a
//! reorganization, so it warns and fails nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{Check, Finding, Rule, Scope, Tree, warn};

pub const RULES: &[Rule] = &[Rule {
    id: "verb-pages",
    scope: Scope::Site,
    fix: "a suggestion, never a failure: give each verb of a module's public commands a page \
          or folder of its own in its docs/, named for the verb, when that reads clearer; a \
          good page shared by two commands stays",
    check: Check::Whole(check),
}];

fn check(tree: &Tree) -> Vec<Finding> {
    let mut found = Vec::new();
    for (path, text) in &tree.files {
        if !path.ends_with("actions.toml") || !path.starts_with(&tree.root) {
            continue;
        }
        let Ok(actions) = toml::from_str::<toml::Table>(text) else {
            continue;
        };
        let dir = path.parent().expect("a folder");
        let missing: Vec<String> = verbs(&actions)
            .into_iter()
            .filter(|verb| !has_page(tree, dir, verb))
            .collect();
        if !missing.is_empty() {
            found.push(warn(format!(
                "{}: no page of its own for {}",
                tree.shown(dir),
                missing.join(", ")
            )));
        }
    }
    found
}

/// The verbs a module's public commands start with: each command under
/// one of its groups (a command it dispatches), but its plumbing.
fn verbs(actions: &toml::Table) -> BTreeSet<String> {
    let groups: BTreeSet<&str> = actions
        .iter()
        .filter(|(_, v)| v.get("dispatch").and_then(toml::Value::as_bool) == Some(true))
        .map(|(k, _)| k.as_str())
        .collect();
    let public: BTreeMap<&str, &toml::Value> = actions
        .iter()
        .filter(|(_, v)| v.get("role").and_then(toml::Value::as_str) != Some("plumbing"))
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    public
        .keys()
        .filter_map(|command| {
            let mut words = command.split(' ');
            let group = words.next()?;
            let verb = words.next()?;
            groups.contains(group).then(|| verb.to_owned())
        })
        .collect()
}

/// Whether the module in `dir` has a page (`<verb>.md`) or a folder
/// (`<verb>/`) named for `verb` anywhere in its `docs/`.
fn has_page(tree: &Tree, dir: &Path, verb: &str) -> bool {
    let docs = dir.join("docs");
    tree.files.keys().filter(|f| f.starts_with(&docs)).any(|f| {
        f.file_stem().is_some_and(|s| s == verb)
            || f.strip_prefix(&docs)
                .is_ok_and(|rel| rel.components().any(|c| c.as_os_str() == verb))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_verb_is_the_word_after_a_group_and_plumbing_is_left_out() {
        let actions: toml::Table = toml::from_str(
            r#"
            [box]
            dispatch = true
            role = "secondary"
            ["box create"]
            role = "primary"
            ["box image list"]
            role = "secondary"
            ["box answer"]
            role = "plumbing"
            ["other run"]
            role = "secondary"
            "#,
        )
        .unwrap();
        let verbs: Vec<String> = super::verbs(&actions).into_iter().collect();
        assert_eq!(verbs, ["create", "image"]);
    }
}

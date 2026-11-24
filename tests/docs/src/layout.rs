//! Repository layout inventories describe folders present in their source
//! tree. Placeholders match existing folders; an absent entry is omitted.

use std::path::Path;

use crate::md::{headings, slug, split};
use crate::site::normal;
use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[Rule {
    id: "repository-layout",
    scope: Scope::Site,
    fix: "repository layout inventories name only folders present in this workspace; \
          omit absent entries, including explanations of their absence",
    check: Check::Each(layout),
}];

fn layout(tree: &Tree, doc: &Path, text: &str) -> Vec<Finding> {
    if !tree.is_doc(doc) {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut section = None;
    for line in split(text).0 {
        if let Some((level, heading)) = headings(&[line]).first() {
            if section.is_none_or(|parent| *level <= parent) {
                section = matches!(
                    slug(heading).as_str(),
                    "layout"
                        | "repository-layout"
                        | "repository-organization"
                        | "repository-structure"
                        | "source-layout"
                        | "source-tree"
                        | "folder-structure"
                )
                .then_some(*level);
            }
            continue;
        }
        if section.is_some()
            && let Some(folder) = entry(line)
            && !present(tree, doc, folder)
        {
            found.push(fail(format!(
                "{}: `{folder}` is not a folder in this source tree; omit the entry",
                tree.shown(doc)
            )));
        }
    }
    found
}

/// The directory in a list or table's leading code span. Absolute,
/// home-relative, and variable-based paths describe runtime locations.
fn entry(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let item = ["- ", "* ", "+ ", "| "]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))?;
    let item = item.strip_prefix('[').unwrap_or(item);
    let (folder, _) = item.strip_prefix('`')?.split_once('`')?;
    (folder.ends_with('/')
        && !folder.starts_with(['/', '~', '$'])
        && !folder.contains(char::is_whitespace))
    .then_some(folder)
}

fn present(tree: &Tree, doc: &Path, folder: &str) -> bool {
    let owner = tree.folder_of(doc);
    std::iter::once(tree.root.as_path())
        .chain(owner.as_deref())
        .chain(doc.parent())
        .any(|base| {
            let pattern = normal(&base.join(folder));
            if !pattern.starts_with(&tree.root) {
                return false;
            }
            let expected: Vec<_> = pattern.components().collect();
            tree.dirs.iter().any(|dir| {
                let actual: Vec<_> = dir.components().collect();
                expected.len() == actual.len()
                    && expected.iter().zip(actual).all(|(part, actual)| {
                        *part == actual
                            || part.as_os_str().to_str().is_some_and(|word| {
                                word == "*" || (word.starts_with('<') && word.ends_with('>'))
                            })
                    })
            })
        })
}

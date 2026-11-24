//! Advisory review of a grouping level that leads to only one child.
//! Architectural layers, packages and documentation bridges keep their
//! boundaries; the check never changes files or requires flattening.

use std::path::{Path, PathBuf};

use crate::{Check, Finding, Rule, Scope, Tree, warn};

pub const RULES: &[Rule] = &[Rule {
    id: "single-child-groups",
    scope: Scope::Every,
    fix: "review whether a grouping with one child hub and no documentation of its own \
          needs a separate navigation level; this is advice, never a failure",
    check: Check::Whole(check),
}];

pub fn check(tree: &Tree) -> Vec<Finding> {
    let packages = crate::shape::packages(tree);
    let readmes: Vec<_> = tree
        .files
        .keys()
        .filter(|path| path.file_name().is_some_and(|n| n == "README.md"))
        .filter(|path| !excluded(tree, &packages, path))
        .collect();
    let mut found = Vec::new();
    for hub in &readmes {
        let dir = hub.parent().unwrap();
        if dir == tree.root
            || dir
                .strip_prefix(&tree.root)
                .is_ok_and(|relative| relative.components().any(|c| c.as_os_str() == "docs"))
            || tree.files.contains_key(&dir.join("Cargo.toml"))
            || tree.files.contains_key(&dir.join("layer.toml"))
        {
            continue;
        }
        let children: Vec<_> = readmes
            .iter()
            .filter(|path| path.parent().and_then(Path::parent) == Some(dir))
            .collect();
        let [child] = children.as_slice() else {
            continue;
        };
        let child_dir = child.parent().unwrap();
        let own_docs = tree.files.keys().any(|path| {
            path.starts_with(dir)
                && path != *hub
                && !path.starts_with(child_dir)
                && path.extension().is_some_and(|e| e == "md")
                && !excluded(tree, &packages, path)
        });
        if !own_docs {
            found.push(warn(format!(
                "{}: only child {}; consider whether this grouping level helps readers",
                tree.shown(hub),
                tree.shown(child)
            )));
        }
    }
    found
}

fn excluded(tree: &Tree, packages: &[PathBuf], path: &Path) -> bool {
    if crate::outlines::metadata(tree, path) {
        return true;
    }
    let Ok(relative) = path.strip_prefix(&tree.root) else {
        return true;
    };
    if relative.iter().any(|part| {
        let name = part.to_string_lossy();
        name.starts_with('.') || matches!(name.as_ref(), "src" | "assets" | "fixtures")
    }) {
        return true;
    }
    if path.file_name().is_some_and(|name| {
        matches!(
            name.to_str(),
            Some("AGENTS.md" | "CLAUDE.md" | "SKILL.md" | "website.md")
        ) || name == concat!("HUMAN", ".md")
    }) {
        return true;
    }
    packages.iter().any(|package| {
        ["tests", "benches", "examples"]
            .iter()
            .any(|folder| path.starts_with(package.join(folder)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn tree(paths: &[(&str, &str)]) -> Tree {
        let root = PathBuf::from("/repository");
        Tree {
            checkout: root.clone(),
            files: paths
                .iter()
                .map(|(path, text)| (root.join(path), (*text).to_owned()))
                .collect(),
            dirs: BTreeSet::new(),
            folders: Vec::new(),
            websites: Vec::new(),
            pages: BTreeMap::new(),
            limits: crate::Limits::read(),
            large: crate::Large::default(),
            root,
        }
    }

    #[test]
    fn one_child_without_own_docs_is_advice_and_never_a_failure() {
        let tree = tree(&[
            ("wrapper/README.md", "# Wrapper\n\nA grouping.\n"),
            ("wrapper/only/README.md", "# Only\n\nThe child.\n"),
            ("wrapper/only/Cargo.toml", "[package]\nname = 'only'\n"),
            ("wrapper/AGENTS.md", "Internal instructions."),
        ]);
        let findings = check(&tree);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].warning);
        assert!(findings[0].text.contains("wrapper/README.md"));
        let report = crate::run(&tree, RULES);
        assert!(report.failures.is_empty());
        assert_eq!(report.warnings["single-child-groups"].len(), 1);
    }

    #[test]
    fn own_docs_and_multiple_children_justify_the_group() {
        for extra in [
            "wrapper/guide.md",
            "wrapper/docs/guide.md",
            "wrapper/other/README.md",
        ] {
            let tree = tree(&[
                ("wrapper/README.md", "# Wrapper\n\nA grouping.\n"),
                ("wrapper/only/README.md", "# Only\n\nThe child.\n"),
                (extra, "# More\n\nAnother subject.\n"),
            ]);
            assert!(check(&tree).is_empty(), "{extra}");
        }
    }

    #[test]
    fn packages_layers_doc_bridges_and_source_folders_are_exempt() {
        for (prefix, extra, text) in [
            (
                "package",
                "package/Cargo.toml",
                "[package]\nname = 'package'\n",
            ),
            (
                "workspace",
                "workspace/Cargo.toml",
                "[workspace]\nmembers = []\n",
            ),
            ("layer", "layer/layer.toml", "depends_on = []\n"),
            ("docs", "docs/guide.md", "A document."),
            (
                "package/src",
                "package/Cargo.toml",
                "[package]\nname = 'package'\n",
            ),
            (
                "package/tests",
                "package/Cargo.toml",
                "[package]\nname = 'package'\n",
            ),
            ("fixtures/group", "fixtures/input.txt", "Input."),
            ("assets/group", "assets/input.txt", "Asset."),
            (".agents/group", ".agents/input.txt", "Internal."),
        ] {
            let hub = format!("{prefix}/README.md");
            let child = format!("{prefix}/only/README.md");
            let tree = tree(&[(&hub, "# Group\n"), (&child, "# Child\n"), (extra, text)]);
            assert!(check(&tree).is_empty(), "{prefix}");
        }
        assert!(
            check(&tree(&[
                ("README.md", "# Root\n"),
                ("only/README.md", "# Only\n")
            ]))
            .is_empty()
        );
    }
}

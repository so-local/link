//! Configuration references follow their typed catalogue's logical home.
//! The code-backed inventory check independently verifies the catalogue entries.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::{Tree, md};

pub(super) fn homes(tree: &Tree) -> BTreeMap<PathBuf, BTreeSet<String>> {
    let packages = super::packages(tree);
    let addresses = super::addresses(tree);
    let mut homes: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    for (index, text) in &tree.files {
        for kind in ["specs", "spec-parts"] {
            let start = format!("<!-- inventory:{kind}:start -->");
            let end = format!("<!-- inventory:{kind}:end -->");
            let Some((_, after)) = text.split_once(&start) else {
                continue;
            };
            let Some((block, _)) = after.split_once(&end) else {
                continue;
            };
            for target in md::links(&block.lines().collect::<Vec<_>>()) {
                if target.contains(':') || target.starts_with('#') {
                    continue;
                }
                let file =
                    crate::site::normal(&index.parent().unwrap().join(md::link_path(target)));
                let Some(address) = addresses.get(&file) else {
                    continue;
                };
                let Some(parent) = super::parent_address(address) else {
                    continue;
                };
                let Some(package) = packages
                    .iter()
                    .filter(|package| file.starts_with(package))
                    .max_by_key(|p| p.as_os_str().len())
                else {
                    continue;
                };
                homes
                    .entry(package.clone())
                    .or_default()
                    .insert(parent.to_string());
            }
        }
    }
    homes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bound_reference_has_a_logical_home_without_exempting_unrelated_pages() {
        let root = link_test_kit::TempDir::new("spec-doc-home");
        let write = |name: &str, text: &str| {
            let file = root.join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        };
        write("large.toml", "[lines]\n");
        write(
            "scripts/documentation-index.json",
            r#"{"sites":[{"name":"Docs","url":"https://docs.example","directory":"web","roots":true}],"roots":[{"directory":"."},{"directory":"parent"},{"directory":"parent/child"}]}"#,
        );
        write(
            "parent/Cargo.toml",
            "[package]\nname='parent'\nversion='0.1.0'\n",
        );
        write(
            "parent/child/Cargo.toml",
            "[package]\nname='child'\nversion='0.1.0'\n",
        );
        write(
            "parent/docs/agents/interface.md",
            "# Interface\n\nThe runtime interface.\n",
        );
        write(
            "parent/child/docs/config/specs/child.md",
            "# Child Spec\n\nIts configuration fields.\n",
        );
        write(
            "parent/child/docs/config/specs/guide.md",
            "# Child Guide\n\nConfigure the child.\n",
        );
        write(
            "docs/config/specs/README.md",
            "# Specs\n\n<!-- inventory:specs:start -->\n- [Child](../../../parent/child/docs/config/specs/child.md)\n<!-- inventory:specs:end -->\n",
        );
        let tree = || Tree::read(&root, false, &root.join("large.toml"));
        assert!(super::super::follows_code(&tree()).is_empty());
        write(
            "parent/child/docs/unrelated.md",
            "# Unrelated\n\nAn unrelated address.\n",
        );
        let findings = super::super::follows_code(&tree());
        assert_eq!(findings.len(), 1);
        assert!(findings[0].text.contains("/unrelated"));
        assert!(!findings[0].text.contains("/config/specs/guide"));
        write(
            "docs/config/specs/README.md",
            "# Specs\n\nNo bound reference.\n",
        );
        assert!(
            super::super::follows_code(&tree())[0]
                .text
                .contains("/config/specs/child")
        );
    }
}

//! Split a site's curated outline into independently maintained top-level sections.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::navigation::Update;
use crate::site::normal;
use crate::{Tree, md::link_path};

use super::{Node, nodes, relative, render, structure};

pub(super) struct Layout {
    pub nodes: Vec<Node>,
    sections: BTreeMap<String, PathBuf>,
}

fn rebase(nodes: &mut [Node], from: &Path, to: &Path) -> Result<(), String> {
    for node in nodes {
        if node.target.ends_with(" \"include\"") {
            return Err("outline includes must be top-level sections, never nested".into());
        }
        if !node.target.contains(':') {
            let at = node
                .target
                .find(['#', '?', ' '])
                .unwrap_or(node.target.len());
            let file = normal(&from.join(link_path(&node.target[..at])));
            node.target = format!("{}{}", relative(to, &file), &node.target[at..]);
        }
        rebase(&mut node.children, from, to)?;
    }
    Ok(())
}

impl Layout {
    pub fn read(tree: &Tree, path: &Path) -> Result<Self, String> {
        let mut outline = nodes(tree.text(path).unwrap_or("# Website\n\n## Pages\n"))?;
        let directory = path.parent().unwrap();
        let mut sections = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for node in &mut outline {
            let Some(target) = node.target.strip_suffix(" \"include\"") else {
                continue;
            };
            let part = Path::new(target);
            if part.components().count() != 2
                || part.parent() != Some(Path::new("navigation"))
                || part.extension().is_none_or(|ext| ext != "md")
                || !node.children.is_empty()
            {
                return Err(
                    "outline include must name navigation/<section>.md and have no children".into(),
                );
            }
            let file = normal(&directory.join(part));
            if !seen.insert(file.clone()) {
                return Err(format!("duplicate outline include {}", file.display()));
            }
            let text = tree
                .text(&file)
                .ok_or_else(|| format!("missing outline {}", file.display()))?;
            structure(text, false).map_err(|error| format!("{}: {error}", file.display()))?;
            let mut included = nodes(text)?;
            if included.len() != 1 || included[0].label != node.label {
                return Err(format!(
                    "{} must contain one top-level {} section",
                    file.display(),
                    node.label
                ));
            }
            rebase(&mut included, file.parent().unwrap(), directory)?;
            *node = included.remove(0);
            if sections.insert(node.target.clone(), file).is_some() {
                return Err("outline sections repeat a page".into());
            }
        }
        // Also refuse include markers nested in an ordinary section.
        rebase(&mut outline, directory, directory)?;
        if let Some(unused) = tree.files.keys().find(|file| {
            file.parent() == Some(directory.join("navigation").as_path())
                && file.extension().is_some_and(|ext| ext == "md")
                && !seen.contains(*file)
        }) {
            return Err(format!("unreferenced outline {}", unused.display()));
        }
        Ok(Self {
            nodes: outline,
            sections,
        })
    }

    pub fn updates(&self, tree: &Tree, path: &Path, title: &str, outline: &[Node]) -> Vec<Update> {
        let mut root = format!("# {title}\n\n## Pages\n\n");
        let mut updates = Vec::new();
        for node in outline {
            if let Some(file) = self.sections.get(&node.target) {
                root += &format!(
                    "- [{}]({} \"include\")\n",
                    node.label,
                    relative(path.parent().unwrap(), file)
                );
                let mut own = vec![node.clone()];
                rebase(&mut own, path.parent().unwrap(), file.parent().unwrap())
                    .expect("expanded outline");
                let mut text = format!("# {} Navigation\n\n## Pages\n\n", node.label);
                render(&own, 0, &mut text);
                if tree.text(file) != Some(text.as_str()) {
                    updates.push(Update {
                        path: file.clone(),
                        text,
                    });
                }
            } else {
                render(std::slice::from_ref(node), 0, &mut root);
            }
        }
        if tree.text(path) != Some(root.as_str()) {
            updates.push(Update {
                path: path.to_path_buf(),
                text: root,
            });
        }
        updates
    }
}

/// The complete outline, with every target relative to the root navigation file.
pub fn expanded(tree: &Tree, path: &Path) -> Result<Vec<Node>, String> {
    Ok(Layout::read(tree, path)?.nodes)
}

/// Render the expanded structure for checks that accept a Markdown outline.
pub fn expanded_text(tree: &Tree, path: &Path) -> Result<String, String> {
    let mut text = String::from("# Website\n\n## Pages\n\n");
    render(&expanded(tree, path)?, 0, &mut text);
    Ok(text)
}

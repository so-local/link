//! Each page's kind, from `kinds.toml`: the guide, the section's own
//! pages, developer pages, generated references, then the catch-alls.
//! The kinds' order is the order a page's children are read in.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[
    Rule {
        id: "group-pages",
        scope: Scope::Site,
        fix: "a page with pages under it holds at least the limit's: fold the few into it, or \
              move them beside it",
        check: Check::Whole(group_pages),
    },
    Rule {
        id: "kind-leaves",
        scope: Scope::Site,
        fix: "a guide, decisions, security or troubleshooting page holds no pages: what it \
              holds is one page's subject, so move the others beside it",
        check: Check::Whole(kind_leaves),
    },
    Rule {
        id: "module-pages",
        scope: Scope::Site,
        fix: "a module with commands of its own has a guide.md, the page a reader starts \
              from, and a troubleshooting.md in its docs",
        check: Check::Whole(module_pages),
    },
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Kinds {
    kind: Vec<Kind>,
}

/// One kind of page.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Kind {
    pub id: String,
    /// The ends of the paths below `docs/` that make a page this kind.
    #[serde(default)]
    files: Vec<String>,
    /// Whether a generated reference is this kind.
    #[serde(default)]
    generated: bool,
    /// Whether a page of this kind holds no pages.
    #[serde(default)]
    pub leaf: bool,
    /// Whether a page of this kind may name Rust source (`rust-names`).
    #[serde(default)]
    pub names_rust: bool,
    /// The heading a README's index lists it under.
    #[serde(default = "pages")]
    pub index: String,
}

/// The headings a README's index lists pages under.
pub const INDEXES: [&str; 3] = ["Pages", "References", "More"];

fn pages() -> String {
    INDEXES[0].to_string()
}

fn kinds() -> &'static [Kind] {
    static KINDS: OnceLock<Vec<Kind>> = OnceLock::new();
    KINDS.get_or_init(|| {
        let kinds: Kinds = toml::from_str(include_str!("../kinds.toml")).expect("kinds.toml");
        let fallback = kinds
            .kind
            .iter()
            .filter(|k| k.files.is_empty() && !k.generated)
            .count();
        assert_eq!(fallback, 1, "kinds.toml has one kind naming no files");
        for kind in &kinds.kind {
            assert!(
                INDEXES.contains(&kind.index.as_str()),
                "kinds.toml: {}'s index is one of {INDEXES:?}",
                kind.id
            );
        }
        kinds.kind
    })
}

/// The kind of the page at `file`, whose text is `text`, and its place in
/// the reading order.
pub fn of(file: &Path, text: &str) -> (usize, &'static Kind) {
    // Only a page, a file below `docs/`, has a kind its file names: a
    // folder's README elsewhere is a page of the fallback kind.
    let below = file
        .to_string_lossy()
        .rsplit_once("/docs/")
        .map(|(_, rest)| format!("/{rest}"));
    let all = kinds();
    let generated = crate::generated_reference(text);
    let named = all.iter().position(|kind| {
        (kind.generated && generated)
            || (!generated
                && below
                    .as_ref()
                    .is_some_and(|b| kind.files.iter().any(|f| b.ends_with(&format!("/{f}")))))
    });
    let at = named.unwrap_or_else(|| {
        all.iter()
            .position(|k| k.files.is_empty() && !k.generated)
            .expect("a fallback kind")
    });
    (at, &all[at])
}

/// Each page of the websites' outlines with its file, if it's a page,
/// and its children's files.
fn outline(tree: &Tree) -> Vec<(PathBuf, Vec<Option<PathBuf>>)> {
    fn visit(
        nodes: &[crate::outlines::Node],
        at: &Path,
        out: &mut Vec<(PathBuf, Vec<Option<PathBuf>>)>,
    ) {
        let file = |node: &crate::outlines::Node| {
            let target = node
                .target
                .split(['#', '?', ' '])
                .next()
                .unwrap_or_default();
            (!node.target.contains(':'))
                .then(|| crate::site::normal(&at.join(crate::md::link_path(target))))
        };
        for node in nodes {
            if let Some(page) = file(node)
                && !node.children.is_empty()
            {
                out.push((page, node.children.iter().map(file).collect()));
            }
            visit(&node.children, at, out);
        }
    }
    let mut out = Vec::new();
    for web in tree.web() {
        let path = web.app.join("docs/navigation.md");
        if let Ok(nodes) = crate::outlines::expanded(tree, &path) {
            visit(&nodes, path.parent().unwrap(), &mut out);
        }
    }
    out
}

/// A page with pages under it holds at least the limit's: fewer is a
/// level the reader opens for one or two pages.
fn group_pages(tree: &Tree) -> Vec<Finding> {
    let least = tree.limits.of("group-pages");
    outline(tree)
        .into_iter()
        .filter(|(page, children)| {
            children.len() < least && !tree.large.holds("group-pages", &tree.shown(page))
        })
        .map(|(page, children)| {
            fail(format!(
                "{}: {} pages under it, fewer than {least}",
                tree.shown(&page),
                children.len()
            ))
        })
        .collect()
}

/// A guide or a catch-all page holds no pages.
fn kind_leaves(tree: &Tree) -> Vec<Finding> {
    outline(tree)
        .into_iter()
        .filter(|(page, _)| {
            of(page, tree.text(page).unwrap_or_default()).1.leaf
                && !tree.large.holds("kind-leaves", &tree.shown(page))
        })
        .map(|(page, children)| {
            fail(format!(
                "{}: a {} page holding {} pages",
                tree.shown(&page),
                of(&page, tree.text(&page).unwrap_or_default()).1.id,
                children.len()
            ))
        })
        .collect()
}

/// A module with commands of its own, so a generated command reference
/// in its `docs/`, has a guide and a troubleshooting page there too.
fn module_pages(tree: &Tree) -> Vec<Finding> {
    let mut found = Vec::new();
    for path in tree.files.keys() {
        let Some(module) = path
            .file_name()
            .is_some_and(|n| n == "module.toml")
            .then(|| path.parent())
            .flatten()
        else {
            continue;
        };
        let docs = module.join("docs");
        let own = |name: &str| {
            tree.files.iter().any(|(file, text)| {
                file.starts_with(&docs)
                    && file.file_name().is_some_and(|n| n == name)
                    && !crate::generated_reference(text)
            })
        };
        let commands = tree.files.iter().any(|(file, text)| {
            file.starts_with(docs.join("more/reference/modules"))
                && crate::generated_reference(text)
        });
        let shown = tree.shown(module);
        if !commands || tree.large.holds("module-pages", &shown) {
            continue;
        }
        let missing: Vec<&str> = ["guide.md", "troubleshooting.md"]
            .into_iter()
            .filter(|name| !own(name))
            .collect();
        if !missing.is_empty() {
            found.push(fail(format!("{shown}: no {}", missing.join(" or "))));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn a_page_is_the_kind_its_file_names_and_a_page_otherwise() {
        let id = |file: &str, text: &str| super::of(Path::new(file), text).1.id.as_str();
        assert_eq!(id("m/docs/a/guide.md", "# A"), "guide");
        assert_eq!(id("m/docs/a/b.md", "# B"), "page");
        assert_eq!(id("m/docs/a/more/README.md", "# More"), "developer");
        assert_eq!(
            id("m/docs/a/protocol/README.md", "# Interface"),
            "developer"
        );
        assert_eq!(id("m/docs/a/troubleshooting.md", "# T"), "troubleshooting");
        assert_eq!(id("interop/README.md", "# Adapters"), "page");
        let marker = format!("<!-- {} -->\n# X", link_actions_surfaces::marker());
        assert_eq!(id("m/docs/config/specs/box.md", &marker), "generated");
        let order = |file| super::of(Path::new(file), "# P").0;
        assert!(order("m/docs/d/guide.md") < order("m/docs/d/page.md"));
        assert!(order("m/docs/d/page.md") < order("m/docs/d/developing.md"));
        assert!(order("m/docs/d/decisions.md") < order("m/docs/d/security.md"));
        assert!(order("m/docs/d/security.md") < order("m/docs/d/troubleshooting.md"));
        let index = |file: &str, text: &str| super::of(Path::new(file), text).1.index.as_str();
        assert_eq!(index("m/docs/d/guide.md", "# G"), "Pages");
        assert_eq!(index("m/docs/d/decisions.md", "# D"), "More");
        assert_eq!(index("m/docs/config/specs/box.md", &marker), "References");
    }
}

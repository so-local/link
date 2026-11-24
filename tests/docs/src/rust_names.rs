//! The docs describe behavior, never the Rust that does it: a page names
//! no Rust source in a code span, neither a `.rs` file, a `src/` path,
//! nor a Rust item path (`link_files::schema`, `Question::new`). Only the
//! pages a maintainer reads may name it, each declared as data: the kinds
//! `kinds.toml` marks `names_rust` (decisions, security and
//! troubleshooting), the folders at any depth below a member's `docs/`
//! that `limits.toml`'s `[rust-names]` names (its `more/` developer and
//! library references), and the folders a workspace's `rust-pages.toml`
//! lists. A page is a Markdown file below a `docs/` folder that is no
//! package of its own: a package's README keeps its `## Code` list of
//! entry points. Code blocks, HTML comments and generated blocks aren't
//! read, and an address such as `[::1]` or a test filter ending in `::`
//! is no item path.

use std::path::Path;

use crate::{Check, Finding, Rule, Scope, Tree};

pub const RULES: &[Rule] = &[Rule {
    id: "rust-names",
    scope: Scope::Every,
    fix: "a page names Rust source, a .rs file, a src/ path or a Rust item path: say what \
          it does in prose, or move a fact only maintainers need to the decisions, security, \
          troubleshooting or docs/more/ page that owns it",
    check: Check::Each(check),
}];

fn check(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    if !page(tree, path)
        || tree.is_data(path)
        || crate::generated_reference(text)
        || crate::kinds::of(path, text).1.names_rust
        || maintained(tree, path)
    {
        return Vec::new();
    }
    named(text)
        .into_iter()
        .map(|(line, code)| crate::fail(format!("{}:{line}: `{code}`", tree.shown(path))))
        .collect()
}

/// Whether `path` is a page: Markdown below a `docs/` folder that holds
/// no package manifest `limits.toml`'s `[rust-names]` names, as the docs
/// check's `tests/docs` holds one, whose README keeps its `## Code` list.
fn page(tree: &Tree, path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "md")
        && path.ancestors().skip(1).any(|folder| {
            folder.file_name().is_some_and(|name| name == "docs")
                && folder.starts_with(&tree.root)
                && !tree
                    .limits
                    .rust_names
                    .manifests
                    .iter()
                    .any(|manifest| tree.files.contains_key(&folder.join(manifest)))
        })
}

/// Whether `path` is in a folder a maintainer reads: one below a `docs/`
/// folder, at any depth, named as `limits.toml`'s `[rust-names]` names,
/// or one the workspace's `rust-pages.toml` lists.
fn maintained(tree: &Tree, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(&tree.root) else {
        return false;
    };
    let folders: Vec<_> = relative
        .parent()
        .into_iter()
        .flat_map(Path::iter)
        .filter_map(|part| part.to_str())
        .collect();
    let below_docs = folders
        .iter()
        .position(|part| *part == "docs")
        .map_or(&[][..], |docs| &folders[docs + 1..]);
    below_docs
        .iter()
        .any(|part| tree.limits.rust_names.within_docs.iter().any(|f| f == part))
        || tree
            .large
            .rust
            .iter()
            .any(|folder| relative.starts_with(folder))
}

/// Each code span in `text`'s prose naming Rust source, with its line.
pub fn named(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (first, paragraph) in crate::asides::paragraphs(text) {
        let mut at = 0;
        while let Some(open) = paragraph[at..].find(['`', '<']).map(|o| at + o) {
            if paragraph[open..].starts_with("<!--") {
                at = paragraph[open..]
                    .find("-->")
                    .map_or(paragraph.len(), |end| open + end + 3);
                continue;
            }
            if !paragraph[open..].starts_with('`') {
                at = open + 1;
                continue;
            }
            let Some(end) = crate::asides::code_span(&paragraph, open) else {
                break;
            };
            let ticks = paragraph[open..].len() - paragraph[open..].trim_start_matches('`').len();
            let code = paragraph[open + ticks..end - ticks].trim();
            if rust(code) {
                let line = first + paragraph[..open].matches('\n').count();
                found.push((line, code.split_whitespace().collect::<Vec<_>>().join(" ")));
            }
            at = end;
        }
    }
    found
}

/// Whether a code span names Rust source: a word that is a `.rs` file
/// (with a line or two after it), holds a `src/` folder, or is an item
/// path of identifiers joined by `::`.
pub fn rust(code: &str) -> bool {
    code.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ',' | '"' | '\''))
        .any(|word| {
            let file = word.split([':', '#']).next().unwrap_or(word);
            file.ends_with(".rs") && file.len() > 3
                || word.starts_with("src/")
                || word.contains("/src/")
                || (!word.contains("://") && item_path(word))
        })
}

/// Whether a word holds two identifiers joined by `::`, as `a::b`.
fn item_path(word: &str) -> bool {
    let identifier = |part: &str| {
        part.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    word.split("::").collect::<Vec<_>>().windows(2).any(|pair| {
        let before = pair[0].rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'));
        let after = pair[1].split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'));
        before.into_iter().next().is_some_and(identifier)
            && after.into_iter().next().is_some_and(identifier)
    })
}

#[cfg(test)]
mod tests {
    use super::{named, rust};

    /// A `.rs` file, a `src/` path and an item path are Rust source.
    #[test]
    fn rust_source_is_named() {
        for code in [
            "store.rs",
            "src/lib.rs",
            "modules/agents/agent/src/teams.rs",
            "process.rs:248-335",
            "src/native/products.toml",
            "link_files::schema",
            "Question::new(\"storage.folders\")",
            "link-box::sandbox::a_session_runs_in_the_sandbox",
            "cargo test -p link-repository-check agents::ids::an_id",
        ] {
            assert!(rust(code), "{code}");
        }
    }

    /// A command, a path elsewhere, an address and a test filter are not.
    #[test]
    fn other_code_is_not_rust_source() {
        for code in [
            "lnk agent start",
            "~/.lnk/safe/agents/policy.toml",
            "[::1]",
            "::1",
            "cargo test -p link-repository-check shapes::",
            "Cargo.toml",
            ".rs",
            "docs/more/sources.md",
            "https://example.com/a::b",
        ] {
            assert!(!rust(code), "{code}");
        }
    }

    /// Only prose's code spans count, each on the line it opens on.
    #[test]
    fn only_prose_code_is_read() {
        let text = "# Page\n\nIt runs `lnk up`.\n\n```\nsrc/lib.rs\n```\n\n<!-- `x.rs` -->\nThen\n`store.rs` keeps it.\n";
        assert_eq!(named(text), [(11, "store.rs".to_string())]);
        let generated = "<!-- inventory:x:start -->\n`a.rs`\n<!-- inventory:x:end -->\n";
        assert!(named(generated).is_empty());
    }
}

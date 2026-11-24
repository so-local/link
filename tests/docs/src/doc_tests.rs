//! A library's doc tests run where it has doc examples, and nowhere else:
//! each library's doc tests cost a build of their own, so one without
//! examples turns them off, and the first example added turns them on.

use std::path::{Path, PathBuf};

use crate::shape::packages;
use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[Rule {
    id: "doc-tests",
    scope: Scope::Every,
    fix: "a library with a Rust example in its docs runs its doc tests: take `doctest = false` \
          out of its [lib]. One with none sets it, since every library's doc tests are a build \
          of their own. A block that isn't Rust says its language: text, sh, toml",
    check: Check::Whole(doc_tests),
}];

fn doc_tests(tree: &Tree) -> Vec<Finding> {
    let packages = packages(tree);
    let mut found = Vec::new();
    for package in &packages {
        let manifest = package.join("Cargo.toml");
        let Some(table) = tree
            .text(&manifest)
            .and_then(|text| toml::from_str::<toml::Table>(text).ok())
        else {
            continue;
        };
        let lib = table.get("lib").and_then(|lib| lib.as_table());
        let root = match lib.and_then(|lib| lib.get("path")).and_then(|p| p.as_str()) {
            Some(path) => package.join(path),
            None => package.join("src/lib.rs"),
        };
        if lib.is_none() && tree.text(&root).is_none() {
            continue;
        }
        let off = lib
            .and_then(|lib| lib.get("doctest"))
            .and_then(|d| d.as_bool())
            == Some(false);
        let src = root.parent().unwrap_or(package);
        let example = tree
            .files
            .iter()
            .filter(|(path, _)| {
                path.starts_with(src) && path.extension().is_some_and(|e| e == "rs")
            })
            .filter(|(path, _)| owner(&packages, path) == Some(package))
            .find_map(|(path, text)| example(text).map(|line| (path, line)));
        match (off, example) {
            (true, Some((path, line))) => found.push(fail(format!(
                "{} sets doctest = false, but {}:{line} is a doc example it would run",
                tree.shown(&manifest),
                tree.shown(path)
            ))),
            (false, None) => found.push(fail(format!(
                "{} builds doc tests for a library with no doc example",
                tree.shown(&manifest)
            ))),
            _ => {}
        }
    }
    found
}

/// The package a file is in: the deepest that holds it.
fn owner<'a>(packages: &'a [PathBuf], path: &Path) -> Option<&'a PathBuf> {
    packages
        .iter()
        .filter(|p| path.starts_with(p))
        .max_by_key(|p| p.components().count())
}

/// The line of the first doc example rustdoc runs in a Rust file: a
/// fence in a `///` or `//!` comment that names no other language.
/// `ignore` builds nothing, so it isn't one.
pub fn example(text: &str) -> Option<usize> {
    let mut open = false;
    for (n, line) in text.lines().enumerate() {
        let doc = line.trim_start();
        let Some(doc) = doc.strip_prefix("///").or_else(|| doc.strip_prefix("//!")) else {
            open = false;
            continue;
        };
        let Some(info) = doc.trim().strip_prefix("```") else {
            continue;
        };
        if open {
            open = false;
            continue;
        }
        open = true;
        let runs = info
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .all(|tag| {
                matches!(
                    tag,
                    "rust"
                        | "no_run"
                        | "compile_fail"
                        | "should_panic"
                        | "test_harness"
                        | "standalone_crate"
                ) || tag.starts_with("edition")
                    || (tag.len() == 5 && tag.starts_with('E'))
            });
        if runs {
            return Some(n + 1);
        }
    }
    None
}

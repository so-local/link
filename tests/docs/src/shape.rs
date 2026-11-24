//! The rules of every workspace's shape: no file past its line limit,
//! a README hub in every package and every folder of packages, and links
//! that go through those hubs.

use std::path::{Component, Path, PathBuf};

use crate::md::{links, split, words};
use crate::site::normal;
use crate::{Check, Finding, Rule, Scope, Tree, fail, warn};

pub const RULES: &[Rule] = &[
    Rule {
        id: "crate-page",
        scope: Scope::Every,
        fix: "every Cargo package has a page in its own docs/ saying what its code is and \
              does, linked from its README's ## Docs; large-files.toml's undocumented lists \
              only the packages written before this, which may shrink, never grow",
        check: Check::Whole(crate_page),
    },
    Rule {
        id: "hub-words",
        scope: Scope::Every,
        fix: "a non-root README outside docs/ is a map of its folder: a sentence, its parts a few words \
              each, and links. Move how it works to a page in docs/ and link to it",
        check: Check::Each(hub_words),
    },
    Rule {
        id: "readme-shape",
        scope: Scope::Every,
        fix: "a README opens with a paragraph on what its folder holds; its `## Docs` lists \
              its pages and folders' READMEs alphabetically; a package's `## Code` comes after and \
              links its code, three entry points or so, each with a few words, alphabetically by \
              name: at least one, never past the limit, and never a page",
        check: Check::Each(readme_shape),
    },
    Rule {
        id: "markdown-lines",
        scope: Scope::Every,
        fix: "a Markdown file past its limit: split it by subject. One over it already may \
              shrink, never grow",
        check: Check::Each(markdown_lines),
    },
    Rule {
        id: "rust-lines",
        scope: Scope::Every,
        fix: "a Rust file past its limit: split it where a responsibility ends. One over it \
              already may shrink, never grow",
        check: Check::Each(rust_lines),
    },
    Rule {
        id: "large-files",
        scope: Scope::Every,
        fix: "large-files.toml lists only files over their limit: take out these",
        check: Check::Whole(large_files),
    },
    Rule {
        id: "readmes",
        scope: Scope::Every,
        fix: "every package, and every folder grouping two or more files or folders, has \
              a README.md hub: a sentence, and links one level down to its docs and its \
              children; large-files.toml's data names the folders a program reads or writes \
              whole, which need none",
        check: Check::Whole(readmes),
    },
    Rule {
        id: "docs-files",
        scope: Scope::Every,
        fix: "a docs/ folder holds only what people read: Markdown, the shell a person types, \
              the variables and domains a person pastes (.env.example, domains.txt), and the data or pictures a page \
              shows. A program, its inputs and its output go \
              beside the code that runs them",
        check: Check::Whole(docs_files),
    },
    Rule {
        id: "through-readmes",
        scope: Scope::Every,
        fix: "a folder hub links one level down; the workspace README can link directly \
              to feature documentation, while source links still follow folder hubs",
        check: Check::Each(through_readmes),
    },
];

/// Whether a path is in a `docs/` folder of the workspace, whose
/// Markdown is pages, not hubs.
fn in_docs(tree: &Tree, path: &Path) -> bool {
    path.strip_prefix(&tree.root)
        .is_ok_and(|rel| rel.components().any(|c| c.as_os_str() == "docs"))
}

/// A hub: a `README.md` outside `docs/`, or a page of a `.github/`, but
/// its issue templates and its pull request's.
fn is_hub(tree: &Tree, path: &Path) -> bool {
    let github = path.components().any(|c| c.as_os_str() == ".github");
    if github {
        return path.extension().is_some_and(|e| e == "md")
            && !path.components().any(|c| c.as_os_str() == "ISSUE_TEMPLATE")
            && !path.ends_with("PULL_REQUEST_TEMPLATE.md");
    }
    path.file_name().is_some_and(|n| n == "README.md") && !in_docs(tree, path)
}

fn hub_words(tree: &Tree, hub: &Path, text: &str) -> Vec<Finding> {
    // The workspace's landing page introduces the product, its features
    // and its mission; subordinate READMEs remain short folder maps.
    if hub == tree.root.join("README.md") {
        return Vec::new();
    }
    let most = tree.limits.of("hub-words");
    // A README's `## Docs` is generated from what each page and folder
    // says of itself, so its words are not the README's own.
    let own: String = text
        .split("\n## ")
        .filter(|section| {
            let heading = section.lines().next().unwrap_or_default();
            !crate::navigation::is_docs_heading(&format!("## {heading}"))
        })
        .collect::<Vec<_>>()
        .join("\n## ");
    let n = words(&own);
    match is_hub(tree, hub) && n > most {
        true => vec![fail(format!("{}: {n} words", tree.shown(hub)))],
        false => Vec::new(),
    }
}

/// A README's shape: a paragraph on what its folder holds, then its
/// `## Docs`, then its `## Code`, a few entry points into its code, so a
/// reader goes from a page to the code.
fn readme_shape(tree: &Tree, hub: &Path, text: &str) -> Vec<Finding> {
    if !is_hub(tree, hub) || hub.file_name().is_none_or(|n| n != "README.md") {
        return Vec::new();
    }
    let most = tree.limits.of("readme-shape");
    let prose = split(text).0;
    let mut found = Vec::new();
    // Under the title, before any list or section.
    let opening = prose
        .iter()
        .skip_while(|l| !l.starts_with("# "))
        .skip(1)
        .find(|l| !l.trim().is_empty());
    if opening.is_none_or(|l| l.starts_with('#') || l.starts_with("- ") || words(l) == 0) {
        found.push(fail(format!(
            "{}: no paragraph under its title",
            tree.shown(hub)
        )));
    }
    let at = |name: &str| {
        prose.iter().position(|l| match name {
            "Docs" => crate::navigation::is_docs_heading(l),
            _ => l.trim_end() == format!("## {name}"),
        })
    };
    // A package's README links into its code.
    let package = hub
        .parent()
        .and_then(|dir| tree.text(&dir.join("Cargo.toml")))
        .is_some_and(|text| {
            toml::from_str::<toml::Table>(text).is_ok_and(|t| t.contains_key("package"))
        });
    if package && at("Code").is_none() {
        found.push(fail(format!(
            "{}: a package's README has no ## Code",
            tree.shown(hub)
        )));
    }
    let section = |at: usize| -> Vec<&str> {
        prose[at + 1..]
            .iter()
            .take_while(|l| !l.starts_with("## "))
            .copied()
            .collect()
    };
    let Some(code) = at("Code") else {
        return found;
    };
    if at("Docs").is_some_and(|docs| docs > code) {
        found.push(fail(format!("{}: ## Code before ## Docs", tree.shown(hub))));
    }
    // A file of this folder's own is named and said in a few words.
    let code = section(code);
    for line in &code {
        let Some((name, target, rest)) = item(line) else {
            continue;
        };
        let path = target.split_once('#').map_or(target, |(path, _)| path);
        if path.ends_with(".md") || target.contains("://") {
            found.push(fail(format!(
                "{}: {target} under ## Code (it links code; a page goes under ## Docs)",
                tree.shown(hub)
            )));
        } else if rest.strip_prefix(':').is_none_or(|r| words(r) == 0) {
            found.push(fail(format!(
                "{}: [{name}]({target}) under ## Code says nothing (`: what it is`)",
                tree.shown(hub)
            )));
        }
    }
    let names: Vec<_> = code
        .iter()
        .filter_map(|line| item(line))
        .map(|(name, _, _)| name.replace('`', "").to_lowercase())
        .collect();
    if !names.is_sorted() {
        found.push(fail(format!(
            "{}: ## Code isn't alphabetical by name",
            tree.shown(hub)
        )));
    }
    let n = links(&code).len();
    if n == 0 || n > most {
        found.push(fail(format!(
            "{}: {n} links under ## Code, not 1 to {most}",
            tree.shown(hub)
        )));
    }
    found
}

/// A list item that is one link: its text, its target and what follows.
fn item(line: &str) -> Option<(&str, &str, &str)> {
    let rest = line.strip_prefix("- [")?;
    let (name, rest) = rest.split_once("](")?;
    let (target, rest) = rest.split_once(')')?;
    Some((name, target, rest.trim()))
}

/// A file's lines against `id`'s limit, for files ending in `extension`:
/// a file over it fails unless `large-files.toml` lists it at least as
/// long, and is a warning while it stays over.
fn lines(tree: &Tree, path: &Path, text: &str, id: &str, extension: &str) -> Vec<Finding> {
    let shown = tree.shown(path);
    if path.extension().is_none_or(|e| e != extension)
        || !path.starts_with(&tree.root)
        || tree.limits.generated.contains(&shown)
    {
        return Vec::new();
    }
    let most = tree.limits.of(id);
    let n = text.lines().count();
    if n <= most {
        return Vec::new();
    }
    match tree.large.lines.get(&shown) {
        None => vec![fail(format!("{shown}: {n} lines, over {most}"))],
        Some(was) if n > *was => vec![fail(format!(
            "{shown}: {n} lines, over {most}, and it had {was}"
        ))],
        Some(was) => vec![warn(format!(
            "{shown}: {n} lines, over {most} (large-files.toml says {was})"
        ))],
    }
}

/// The pages behind a feature's page, which may run as long as they
/// need: their sections and subsections organize them.
pub const REFERENCE: &[&str] = &["decisions.md", "security.md", "troubleshooting.md"];

fn markdown_lines(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    if path
        .file_name()
        .is_some_and(|n| REFERENCE.iter().any(|r| n == *r))
        || tree.generated_exempt("markdown-lines", path, text)
    {
        return Vec::new();
    }
    let text = if path.file_name().is_some_and(|n| n == "README.md") {
        crate::navigation::authored(text)
    } else {
        text.to_owned()
    };
    lines(tree, path, &text, "markdown-lines", "md")
}

fn rust_lines(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    lines(tree, path, text, "rust-lines", "rs")
}

fn large_files(tree: &Tree) -> Vec<Finding> {
    let mut found = Vec::new();
    for (shown, was) in &tree.large.lines {
        let path = tree.root.join(shown);
        let id = match path.extension().and_then(|e| e.to_str()) {
            Some("md") => "markdown-lines",
            Some("rs") => "rust-lines",
            _ => {
                found.push(fail(format!("{shown}: neither Markdown nor Rust")));
                continue;
            }
        };
        match tree.text(&path).map(|t| {
            if path.file_name().is_some_and(|n| n == "README.md") {
                crate::navigation::authored(t).lines().count()
            } else {
                t.lines().count()
            }
        }) {
            None => found.push(fail(format!("{shown}: no such file"))),
            Some(n) if n <= tree.limits.of(id) => {
                found.push(fail(format!("{shown}: {n} lines, within its limit")))
            }
            Some(n) if n < *was => found.push(warn(format!(
                "{shown}: {n} lines, down from {was}: lower it in large-files.toml"
            ))),
            Some(_) => {}
        }
    }
    found
}

/// The Cargo packages of the workspace: each folder whose `Cargo.toml`
/// has a `[package]`.
pub(crate) fn packages(tree: &Tree) -> Vec<PathBuf> {
    tree.files
        .iter()
        .filter(|(path, _)| path.ends_with("Cargo.toml") && path.starts_with(&tree.root))
        .filter(|(_, text)| {
            toml::from_str::<toml::Table>(text).is_ok_and(|t| t.contains_key("package"))
        })
        .filter_map(|(path, _)| path.parent().map(Path::to_path_buf))
        .collect()
}

/// Folders that need no README of their own, by kind: a package's
/// `src/` and `tests/`, and fixtures and assets anywhere, with what's in
/// them.
fn exempt(packages: &[PathBuf], root: &Path, dir: &Path) -> bool {
    let mut at = root.to_path_buf();
    let Ok(rel) = dir.strip_prefix(root) else {
        return true;
    };
    for part in rel.components() {
        let Component::Normal(name) = part else {
            continue;
        };
        let in_package = packages.contains(&at);
        at.push(name);
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), "fixtures" | "assets")
            || (in_package && matches!(name.as_ref(), "src" | "tests" | "benches" | "examples"))
        {
            return true;
        }
    }
    false
}

fn readmes(tree: &Tree) -> Vec<Finding> {
    let packages = packages(tree);
    let mut need: Vec<&Path> = packages.iter().map(PathBuf::as_path).collect();
    for dir in &tree.dirs {
        let below = packages
            .iter()
            .filter(|p| p.starts_with(dir) && *p != dir)
            .count();
        if below >= 2 && dir.starts_with(&tree.root) {
            need.push(dir);
        }
    }
    need.extend(groups(tree, &packages));
    need.sort();
    need.dedup();
    need.into_iter()
        .filter(|dir| !exempt(&packages, &tree.root, dir))
        .filter(|dir| !tree.files.contains_key(&dir.join("README.md")))
        .map(|dir| fail(format!("{}/README.md is missing", tree.shown(dir))))
        .collect()
}

/// The folders that group two or more files or folders, outside a
/// package (a Rust or an npm one), a `docs/` tree, a hidden folder and
/// the folders of data `large-files.toml` names: each needs a README saying what
/// its group is.
fn groups<'a>(tree: &'a Tree, packages: &[PathBuf]) -> Vec<&'a Path> {
    let npm: Vec<&Path> = tree
        .files
        .iter()
        .filter(|(f, text)| f.ends_with("package.json") && !text.contains("\"workspaces\""))
        .filter_map(|(f, _)| f.parent())
        .collect();
    let inside = |dir: &Path| {
        packages.iter().any(|p| dir.starts_with(p) && dir != p)
            || npm.iter().any(|p| dir.starts_with(p) && dir != *p)
    };
    let entries = |dir: &Path| {
        let files = tree
            .files
            .keys()
            .filter(|f| f.parent() == Some(dir))
            .count();
        let dirs = tree.dirs.iter().filter(|d| d.parent() == Some(dir)).count();
        files + dirs
    };
    tree.dirs
        .iter()
        .filter(|dir| *dir != &tree.root)
        .filter(|dir| {
            dir.strip_prefix(&tree.root).is_ok_and(|rel| {
                !rel.components().any(|c| {
                    let name = c.as_os_str().to_string_lossy();
                    name.starts_with('.') || name == "docs"
                }) && !tree.is_data(dir)
            })
        })
        .filter(|dir| !inside(dir) && entries(dir) >= 2)
        .map(PathBuf::as_path)
        .collect()
}

/// A folder with a hub of its own, outside `docs/`.
fn has_hub(tree: &Tree, dir: &Path) -> bool {
    !in_docs(tree, dir) && tree.files.contains_key(&dir.join("README.md"))
}

fn through_readmes(tree: &Tree, hub: &Path, text: &str) -> Vec<Finding> {
    if !is_hub(tree, hub) || hub.file_name().is_none_or(|n| n != "README.md") {
        return Vec::new();
    }
    let from = hub.parent().unwrap();
    let mut found = Vec::new();
    for target in links(&split(text).0) {
        if target.contains("://") || target.starts_with("mailto:") || target.starts_with('#') {
            continue;
        }
        let path = target.split_once('#').map_or(target, |(path, _)| path);
        let to = normal(&from.join(crate::md::link_path(path)));
        if !to.starts_with(from) || to == from {
            continue;
        }
        // The repository's entry page introduces its products and links
        // their documentation directly. Source navigation still follows
        // the folder hubs, as every other README does.
        if hub == tree.root.join("README.md") && in_docs(tree, &to) {
            continue;
        }
        // The folder the link lands in, and whether it's that folder's hub.
        let (folder, its_hub) = if tree.dirs.contains(&to) {
            (to.clone(), true)
        } else if to.file_name().is_some_and(|n| n == "README.md") {
            (to.parent().unwrap().to_path_buf(), true)
        } else {
            (to.parent().unwrap().to_path_buf(), false)
        };
        let mut skipped = None;
        let mut at = from.to_path_buf();
        for part in folder.strip_prefix(from).unwrap().components() {
            at.push(part);
            let last = at == folder;
            if has_hub(tree, &at) && !(last && its_hub) && skipped.is_none() {
                skipped = Some(at.clone());
            }
        }
        if let Some(skipped) = skipped {
            found.push(fail(format!(
                "{}: {target} (link {}/README.md)",
                tree.shown(hub),
                tree.shown(&skipped)
            )));
        }
    }
    found
}

/// Every package has a page of its own, but a test's fixture: the
/// packages without one when the rule came in are `undocumented`, and
/// one leaves once it has a page.
fn crate_page(tree: &Tree) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut packages = std::collections::BTreeSet::new();
    for (path, text) in &tree.files {
        if !path.ends_with("Cargo.toml")
            || !path.starts_with(&tree.root)
            || !toml::from_str::<toml::Table>(text).is_ok_and(|t| t.contains_key("package"))
        {
            continue;
        }
        let dir = path.parent().expect("a folder");
        let shown = tree.shown(dir);
        if shown.starts_with("tests/fixtures/") || shown.contains("/fixtures/") {
            continue;
        }
        packages.insert(shown.clone());
        let docs = dir.join("docs");
        let has = tree
            .files
            .keys()
            .any(|f| f.starts_with(&docs) && f.extension().is_some_and(|e| e == "md"));
        match (has, tree.large.undocumented.contains(&shown)) {
            (false, false) => found.push(fail(format!("{shown}: no page in docs/"))),
            (true, true) => found.push(fail(format!(
                "{shown}: has a page now; take it out of undocumented"
            ))),
            _ => {}
        }
    }
    for listed in &tree.large.undocumented {
        if !packages.contains(listed) {
            found.push(fail(format!(
                "{listed}: undocumented, but no package is there"
            )));
        }
    }
    found
}

/// What a `docs/` folder may hold: pages, the shell they show, and the
/// data and pictures a page shows.
const DOCS_FILES: &[&str] = &["md", "sh", "yaml", "json", "svg", "png"];
/// Files a person reads by their whole name and pastes into an
/// environment: its variables, each placeholder to fill, and its allowed
/// domains, one per line.
const DOCS_NAMES: &[&str] = &[".env.example", "domains.txt"];

/// Every file in a `docs/` folder is one a person reads. Inside a package
/// (a folder with a `Cargo.toml` or `package.json`) only its own `docs/`
/// is docs, so a package named `docs`, or a test module of that name, is
/// code.
fn docs_files(tree: &Tree) -> Vec<Finding> {
    let package = |dir: &Path| dir.join("Cargo.toml").exists() || dir.join("package.json").exists();
    let in_docs = |dir: &Path| {
        let within = |here: &&Path| here.starts_with(&tree.root);
        let Some(docs) = dir
            .ancestors()
            .take_while(within)
            .find(|here| here.file_name().is_some_and(|name| name == "docs"))
        else {
            return false;
        };
        let owner = docs
            .ancestors()
            .take_while(within)
            .find(|here| package(here));
        owner.is_none_or(|owner| docs.parent() == Some(owner))
    };
    let mut found = Vec::new();
    for dir in tree.dirs.iter().filter(|dir| in_docs(dir)) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for path in entries.flatten().map(|entry| entry.path()) {
            let named = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| DOCS_NAMES.contains(&n));
            let allowed = named
                || path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| DOCS_FILES.contains(&e));
            if !path.is_dir() && !allowed {
                found.push(fail(tree.shown(&path)));
            }
        }
    }
    found
}

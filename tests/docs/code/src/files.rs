//! The repository's file kinds, one typed catalogue (`files.toml`): what
//! each recurring file is, who reads it and whether a generated header
//! says so. The Repository Files reference lists the kinds and the build
//! scripts from it, and the notices are written from it, so the three
//! never disagree.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use link_docs_check::navigation::Update;
use link_docs_check::{Finding, Tree, fail};
use serde::Deserialize;

/// The reference page, from the workspace's root.
pub const REFERENCE: &str = "docs/more/reference/files.md";

/// The first line of every generated header.
const HEAD: &str = "# Local Link repository metadata: ";

/// The last line of every generated header.
const TAIL: &str =
    "# Header maintained by lnk-docs-inventories; the declaration below is authored here.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalogue {
    kind: Vec<Kind>,
}

/// One kind of file: how it's found, what it is and who reads it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kind {
    pub id: String,
    pub title: String,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    segments: Vec<String>,
    #[serde(default)]
    package: bool,
    pub purpose: String,
    pub reader: String,
    #[serde(default)]
    pub notice: Option<String>,
}

/// The catalogue, read once from `files.toml`.
pub fn kinds() -> Vec<Kind> {
    let catalogue: Catalogue =
        toml::from_str(include_str!("../files.toml")).expect("a valid file catalogue");
    catalogue.kind
}

impl Kind {
    /// Whether `rel`, a path from the workspace's root, is of this kind;
    /// `packaged` says whether it's in a Cargo package's folder.
    fn matches(&self, rel: &Path, packaged: bool) -> bool {
        let name = rel.file_name().map(|n| n.to_string_lossy().into_owned());
        let text = rel.to_string_lossy().replace('\\', "/");
        let folders: Vec<_> = rel
            .parent()
            .into_iter()
            .flat_map(Path::components)
            .filter_map(|c| match c {
                Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        self.names.iter().any(|n| Some(n) == name.as_ref())
            || self.paths.iter().any(|p| {
                if p.ends_with('/') {
                    text.starts_with(p.as_str())
                } else {
                    text == *p
                }
            })
            || self.segments.iter().any(|s| folders.contains(s))
            || (self.package && packaged)
    }
}

/// The workspace's tracked text files of the catalogue's concern (TOML
/// and Cargo's lock), each with the kind it is, or none.
fn classified<'a>(tree: &Tree, kinds: &'a [Kind]) -> Vec<(PathBuf, Option<&'a Kind>)> {
    let packages: Vec<PathBuf> = tree
        .files
        .keys()
        .filter(|p| p.file_name().is_some_and(|n| n == "Cargo.toml"))
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect();
    tree.files
        .keys()
        .filter(|p| p.starts_with(&tree.root))
        .filter(|p| {
            p.extension().is_some_and(|e| e == "toml")
                || p.file_name().is_some_and(|n| n == "Cargo.lock")
        })
        .filter(|p| !ignored(tree, p))
        .map(|path| {
            let rel = path.strip_prefix(&tree.root).unwrap();
            let packaged = packages
                .iter()
                .any(|package| *package != tree.root && path.starts_with(package));
            (
                path.clone(),
                kinds.iter().find(|k| k.matches(rel, packaged)),
            )
        })
        .collect()
}

/// Whether `path` is outside what the repository tracks: a build's
/// output, or a folder Git ignores here.
fn ignored(tree: &Tree, path: &Path) -> bool {
    let rel = path.strip_prefix(&tree.root).unwrap_or(path);
    rel.components().any(|c| {
        matches!(c, Component::Normal(part) if part == "target" || part == "node_modules" || part.to_string_lossy().starts_with("target-"))
    })
}

/// The copyright holder and years, from `[workspace.metadata.link.copyright]`.
fn copyright(tree: &Tree) -> Result<(String, String), String> {
    let text = tree
        .text(&tree.root.join("Cargo.toml"))
        .ok_or("the workspace's Cargo.toml")?;
    let value: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let record = &value["workspace"]["metadata"]["link"]["copyright"];
    let field = |name: &str| {
        record
            .get(name)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("[workspace.metadata.link.copyright] {name}"))
    };
    Ok((field("holder")?, field("years")?))
}

/// The license of the package owning `path`, or the workspace's.
fn license(tree: &Tree, path: &Path) -> Result<String, String> {
    let read =
        |manifest: &Path| -> Option<toml::Value> { toml::from_str(tree.text(manifest)?).ok() };
    let workspace = read(&tree.root.join("Cargo.toml")).ok_or("the workspace's Cargo.toml")?;
    let inherited = workspace["workspace"]["package"]["license"]
        .as_str()
        .map(str::to_owned);
    let mut dir = path.parent();
    while let Some(folder) = dir.filter(|d| d.starts_with(&tree.root) && *d != tree.root) {
        if let Some(manifest) = read(&folder.join("Cargo.toml"))
            && let Some(package) = manifest.get("package")
        {
            return match package.get("license") {
                Some(toml::Value::String(license)) => Ok(license.clone()),
                Some(_) | None => inherited.ok_or_else(|| "no workspace license".into()),
            };
        }
        dir = folder.parent();
    }
    inherited.ok_or_else(|| "no workspace license".into())
}

fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut path = PathBuf::new();
    for _ in common..from.len() {
        path.push("..");
    }
    for part in &to[common..] {
        path.push(part.as_os_str());
    }
    path.to_string_lossy().replace('\\', "/")
}

/// The generated header for `path`, a file of `kind`.
fn header(tree: &Tree, path: &Path, kind: &Kind, notice: &str) -> Result<String, String> {
    let (holder, years) = copyright(tree)?;
    // A workspace without the page points to the public's, `link-public/`
    // beside it or beside a folder it is in.
    let page = Some(tree.root.join(REFERENCE))
        .filter(|page| tree.files.contains_key(page))
        .or_else(|| {
            tree.root
                .ancestors()
                .skip(1)
                .map(|folder| folder.join("link-public").join(REFERENCE))
                .find(|page| page.is_file())
        })
        .unwrap_or_else(|| tree.root.join("../link-public").join(REFERENCE));
    let reference = relative(path.parent().unwrap(), &page);
    Ok(format!(
        "{HEAD}{notice}.\n# Author: {holder}\n# SPDX-FileCopyrightText: {years} {holder}\n\
         # SPDX-License-Identifier: {}\n# Reference: {reference}#{}\n{TAIL}\n",
        license(tree, path)?,
        kind.id
    ))
}

/// `text` without its generated header, if it has one: the authored
/// declaration, byte for byte.
pub fn body(text: &str) -> &str {
    if !text.starts_with(HEAD) {
        return text;
    }
    match text.find(TAIL) {
        Some(at) => {
            let rest = &text[at + TAIL.len()..];
            rest.strip_prefix('\n').unwrap_or(rest)
        }
        None => text,
    }
}

/// The header each noticed file should start with, and what it holds.
fn stamped(tree: &Tree) -> Result<Vec<Update>, String> {
    let kinds = kinds();
    let mut updates = Vec::new();
    for (path, kind) in classified(tree, &kinds) {
        let Some((kind, notice)) = kind.and_then(|k| k.notice.as_deref().map(|n| (k, n))) else {
            continue;
        };
        let text = tree.text(&path).unwrap();
        let wanted = format!("{}{}", header(tree, &path, kind, notice)?, body(text));
        if wanted != text {
            updates.push(Update { path, text: wanted });
        }
    }
    Ok(updates)
}

/// The workspace's packages with a Cargo build script, as Cargo finds
/// one: `package.build` when it names a file, else `build.rs` at the
/// package's root, unless `build = false`. A `build.rs` anywhere else is
/// a module like any other.
fn build_scripts(tree: &Tree) -> BTreeMap<String, PathBuf> {
    tree.files
        .iter()
        .filter(|(p, _)| p.file_name().is_some_and(|n| n == "Cargo.toml") && !ignored(tree, p))
        .filter_map(|(manifest, text)| {
            let value: toml::Value = toml::from_str(text).ok()?;
            let package = value.get("package")?;
            let name = package.get("name")?.as_str()?.to_owned();
            let folder = manifest.parent()?;
            let script = match package.get("build") {
                Some(toml::Value::Boolean(false)) => return None,
                Some(toml::Value::String(file)) => folder.join(file),
                _ => folder.join("build.rs"),
            };
            tree.files.contains_key(&script).then_some((name, script))
        })
        .collect()
}

fn render(tree: &Tree, kinds: &[Kind]) -> (String, String) {
    let page = tree.root.join(REFERENCE);
    let counts: BTreeMap<&str, usize> = classified(tree, kinds)
        .into_iter()
        .filter_map(|(_, kind)| kind.map(|k| k.id.as_str()))
        .fold(BTreeMap::new(), |mut counts, id| {
            *counts.entry(id).or_default() += 1;
            counts
        });
    let mut files = String::from("<!-- inventory:file-kinds:start -->\n\n");
    for kind in kinds {
        files += &format!(
            "- **{}** ({} {}): {}. Read by {}.\n",
            kind.title,
            counts.get(kind.id.as_str()).copied().unwrap_or(0),
            if counts.get(kind.id.as_str()) == Some(&1) {
                "file"
            } else {
                "files"
            },
            kind.purpose,
            kind.reader
        );
    }
    files += "\n<!-- inventory:file-kinds:end -->";
    let mut scripts = String::from("<!-- inventory:build-scripts:start -->\n\n");
    // Each by its folder: a crate's name can read as a product's.
    let mut folders: Vec<_> = build_scripts(tree).into_values().collect();
    folders.sort();
    for script in folders {
        let folder = script.parent().unwrap().strip_prefix(&tree.root).unwrap();
        scripts += &format!(
            "- [`{}`]({})\n",
            folder.display(),
            relative(page.parent().unwrap(), &script)
        );
    }
    scripts += "\n<!-- inventory:build-scripts:end -->";
    (files, scripts)
}

/// Only the named, unique marker pair is writable.
fn replace(text: &str, kind: &str, block: &str) -> Result<String, String> {
    let start = format!("<!-- inventory:{kind}:start -->");
    let end = format!("<!-- inventory:{kind}:end -->");
    match (text.find(&start), text.find(&end)) {
        (Some(a), Some(b)) if b > a && text.matches(&start).count() == 1 => {
            Ok(format!("{}{}{}", &text[..a], block, &text[b + end.len()..]))
        }
        _ => Err(format!(
            "{kind}: expected one ordered inventory marker pair"
        )),
    }
}

/// The reference's generated lists, and every noticed file's header.
pub fn updates(tree: &Tree) -> Result<Vec<Update>, String> {
    let mut updates = stamped(tree)?;
    let page = tree.root.join(REFERENCE);
    if let Some(old) = tree.text(&page) {
        let (files, scripts) = render(tree, &kinds());
        let text = replace(
            &replace(old, "file-kinds", &files)?,
            "build-scripts",
            &scripts,
        )?;
        if text != old {
            updates.push(Update { path: page, text });
        }
    }
    Ok(updates)
}

/// Every TOML file is of a kind, and every generated list and header is
/// as the catalogue writes it.
pub fn check(tree: &Tree) -> Vec<Finding> {
    let kinds = kinds();
    let mut found: Vec<Finding> = classified(tree, &kinds)
        .into_iter()
        .filter(|(_, kind)| kind.is_none())
        .map(|(path, _)| {
            fail(format!(
                "{}: no kind in tests/docs/code/files.toml matches it",
                tree.shown(&path)
            ))
        })
        .collect();
    match updates(tree) {
        Ok(updates) => found.extend(updates.into_iter().map(|update| {
            fail(format!(
                "{}: its generated header or list is missing or stale",
                tree.shown(&update.path)
            ))
        })),
        Err(error) => found.push(fail(error)),
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_comes_off_whole_and_leaves_the_declaration_byte_for_byte() {
        let declaration = "# Authored.\nschema = 1\r\n[module]\nid = \"x\"\n";
        assert_eq!(body(declaration), declaration);
        let stamped = format!("{HEAD}declares it.\n# Author: A\n{TAIL}\n{declaration}");
        assert_eq!(body(&stamped), declaration);
        // A header cut short is left alone, never half removed.
        let cut = format!("{HEAD}declares it.\n# Author: A\n{declaration}");
        assert_eq!(body(&cut), cut);
    }

    #[test]
    fn each_kind_matches_by_its_own_rule_first_kind_first() {
        let kinds = kinds();
        let kind = |path: &str, packaged: bool| {
            kinds
                .iter()
                .find(|k| k.matches(Path::new(path), packaged))
                .map(|k| k.id.as_str())
        };
        assert_eq!(
            kind("modules/compute/box/module.toml", true),
            Some("component")
        );
        assert_eq!(kind("modules/compute/layer.toml", false), Some("layer"));
        assert_eq!(kind("tests/specs/box/a.toml", false), Some("spec-case"));
        assert_eq!(
            kind("tests/repository/test-kit.toml", true),
            Some("repository-policy")
        );
        assert_eq!(
            kind("modules/agents/agent/tests/a.toml", true),
            Some("test-input")
        );
        assert_eq!(
            kind("modules/tooling/measure/defaults.toml", true),
            Some("owned-data")
        );
        assert_eq!(kind("docs/a.toml", false), None);
        // Every kind is reachable and named once.
        let mut ids: Vec<_> = kinds.iter().map(|k| k.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), kinds.len());
    }
}

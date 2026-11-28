//! Every format Link saves says where its files are, so `lnk upgrade`
//! finds and converts them: its `schema.toml` lists its `[[files]]`,
//! declares `holds = "history"` for a ledger of what happened, which an
//! upgrade never rewrites, or `holds = "inside"` for state that isn't a
//! file at a map location (an object in a store, a manifest in an
//! archive, an envelope around another file, a file at a path its
//! caller names, or one under the system's root), which its owner advances as it reads it. A format is a `link_files::specification!`
//! (`crate::specification!` in `link-files` itself) or a
//! `Format::shipped` naming its `schema.toml` with `include_str!`, outside
//! test code. `placed.toml` names the formats still unplaced, each with
//! why, a list that only shrinks.
//!
//! A format of state at a map location (its `[[files]]`, holding neither
//! history nor state inside something else) is converted only by a
//! program that owns it, so its crate makes it a
//! `link_files::upgrade::Saved` for a program's `main` to name with
//! `link_core::saved::own`: `Saved::of::<T>` for a declared type `T`, or
//! `Saved::shipped::<T>` beside a `Format::shipped`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

/// `placed.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    /// Formats whose `schema.toml` neither lists its files nor declares
    /// history, each with why, by its path from the root: the list only
    /// shrinks.
    #[serde(default)]
    pub unplaced: BTreeMap<String, String>,
}

/// The part of a `schema.toml` this check reads.
#[derive(Deserialize)]
struct Placement {
    #[serde(default)]
    holds: Option<String>,
    #[serde(default)]
    files: Vec<toml::Value>,
}

/// The `schema.toml` files, relative to the Rust file's folder, that the
/// Rust source `text` of `file` declares saved formats with: by
/// `link_files::specification!`, `crate::specification!` in `link-files`
/// itself, or `Format::shipped`, each naming its file with `include_str!`.
/// A test module is left out.
pub fn declared(file: &str, text: &str) -> Vec<String> {
    formats(file, text)
        .into_iter()
        .map(|format| format.schema)
        .collect()
}

/// A saved format a Rust file declares.
#[derive(Debug, PartialEq, Eq)]
pub struct Declared {
    /// Its type, as `specification!` names it; none for a
    /// `Format::shipped`.
    pub ty: Option<String>,
    /// Its `schema.toml`, relative to the Rust file's folder.
    pub schema: String,
}

/// The saved formats the Rust source `text` of `file` declares, as
/// [`declared`] finds them, each with its type.
pub fn formats(file: &str, text: &str) -> Vec<Declared> {
    let code: String = crate::saved::outside_tests(text)
        .into_iter()
        .map(|(_, line)| format!("{line}\n"))
        .collect();
    let own = file.starts_with("packages/files/");
    let mut found = Vec::new();
    for opens in [
        "link_files::specification!(",
        "crate::specification!(",
        "Format::shipped(",
    ] {
        for (at, _) in code.match_indices(opens) {
            let before = code[..at].chars().next_back();
            let ours = match opens {
                "crate::specification!(" => own && before != Some('$'),
                _ => before.is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '$'),
            };
            if !ours {
                continue;
            }
            let rest = &code[at + opens.len()..];
            let call = &rest[..rest.find(';').unwrap_or(rest.len())];
            if let Some(path) = call
                .split_once("include_str!(\"")
                .and_then(|(_, path)| path.split_once('"'))
                .map(|(path, _)| path.to_string())
            {
                let ty = (opens != "Format::shipped(").then(|| {
                    call.split(',')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_string()
                });
                found.push(Declared { ty, schema: path });
            }
        }
    }
    found.sort_by_key(|format| format.schema.clone());
    found
}

/// `path` joined to `folder`, its `..` taken back: a path from the root.
fn joined(folder: &Path, path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for part in folder.join(path).components() {
        match part {
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Normal(name) => parts.push(name.to_string_lossy().into()),
            _ => {}
        }
    }
    parts.join("/")
}

/// Whether a `schema.toml`'s text places its format: lists its files, or
/// declares history or state inside something else.
pub fn placed(text: &str) -> Result<bool, String> {
    let declared: Placement = toml::from_str(text).map_err(|err| err.to_string())?;
    Ok(
        matches!(declared.holds.as_deref(), Some("history" | "inside"))
            || !declared.files.is_empty(),
    )
}

/// Whether a `schema.toml`'s text is state at a map location: it lists
/// its files and holds neither history nor state inside something else.
pub fn at_a_location(text: &str) -> Result<bool, String> {
    let declared: Placement = toml::from_str(text).map_err(|err| err.to_string())?;
    Ok(declared.holds.is_none() && !declared.files.is_empty())
}

/// Whether `code`, a crate's Rust outside its tests, makes `format` a
/// `Saved`: `Saved::of::<T>` naming its type by any path, or, for a
/// shipped format, `Saved::shipped::<`.
pub fn made(format: &Declared, code: &str) -> bool {
    let Some(ty) = &format.ty else {
        return code.contains("Saved::shipped::<");
    };
    let name = ty.rsplit("::").next().unwrap_or(ty);
    code.match_indices("Saved::of::<").any(|(at, opens)| {
        let given = &code[at + opens.len()..];
        let given = &given[..given.find('>').unwrap_or(given.len())];
        given.rsplit("::").next() == Some(name)
    })
}

/// The folder of the crate `file` is in: the nearest above it holding a
/// `Cargo.toml`, under `root`.
fn crate_of(root: &Path, file: &str) -> String {
    let mut folder = Path::new(file).parent();
    while let Some(at) = folder {
        if root.join(at).join("Cargo.toml").is_file() {
            return at.to_string_lossy().into();
        }
        folder = at.parent();
    }
    String::new()
}

/// Each problem with the formats under `root`, by the rules in `rules` (a
/// `placed.toml`): none when every format is placed or listed unplaced,
/// and every listed one is still a format that isn't.
pub fn problems(root: &Path, rules: &Path) -> Vec<String> {
    let rules: Rules = match std::fs::read_to_string(rules)
        .map_err(|err| err.to_string())
        .and_then(|text| toml::from_str(&text).map_err(|err| err.to_string()))
    {
        Ok(rules) => rules,
        Err(err) => return vec![format!("{} isn't read: {err}", rules.display())],
    };
    let mut found = Vec::new();
    let mut unplaced = BTreeSet::new();
    let mut code: BTreeMap<String, String> = BTreeMap::new();
    let mut located = Vec::new();
    for file in crate::shapes::sources(root) {
        if crate::shapes::test_code(&file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&file)) else {
            continue;
        };
        code.entry(crate_of(root, &file)).or_default().extend(
            crate::saved::outside_tests(&text)
                .into_iter()
                .map(|(_, line)| format!("{line}\n")),
        );
        let folder = Path::new(&file).parent().unwrap_or(Path::new(""));
        for format in formats(&file, &text) {
            let schema = joined(folder, &format.schema);
            let read = std::fs::read_to_string(root.join(&schema)).map_err(|err| err.to_string());
            if read
                .as_deref()
                .is_ok_and(|text| at_a_location(text) == Ok(true))
            {
                located.push((crate_of(root, &file), schema.clone(), format));
            }
            match read.and_then(|text| placed(&text)) {
                Ok(true) => {}
                Ok(false) => {
                    if !rules.unplaced.contains_key(&schema) {
                        found.push(format!(
                            "{schema}: a saved format with no place: list its [[files]], or declare holds = \"history\" or holds = \"inside\""
                        ));
                    }
                    unplaced.insert(schema);
                }
                Err(err) => found.push(format!("{schema} isn't read: {err}")),
            }
        }
    }
    for (owner, schema, format) in &located {
        if !code.get(owner).is_some_and(|code| made(format, code)) {
            found.push(format!(
                "{schema}: no program can own it: make it a link_files::upgrade::Saved in its crate (Saved::of or Saved::shipped) and name it in its program's link_core::saved::own"
            ));
        }
    }
    for schema in rules.unplaced.keys().filter(|s| !unplaced.contains(*s)) {
        found.push(format!(
            "{schema} is placed now, or no format: take it out of placed.toml's unplaced"
        ));
    }
    found.sort();
    found.dedup();
    found
}

//! Every format Link saves says where its files are, so `lnk upgrade`
//! finds and converts them: its `schema.toml` lists its `[[files]]`, or
//! declares `holds = "history"` for a ledger of what happened, which an
//! upgrade never rewrites. A format is a `link_files::specification!`
//! (`crate::specification!` in `link-files` itself) or a
//! `Format::shipped` naming its `schema.toml` with `include_str!`, outside
//! test code. `placed.toml` names the formats still unplaced, each with
//! why, a list that only shrinks.

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
struct Declared {
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
                found.push(path);
            }
        }
    }
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

/// Whether a `schema.toml`'s text places its format: lists its files or
/// declares history.
pub fn placed(text: &str) -> Result<bool, String> {
    let declared: Declared = toml::from_str(text).map_err(|err| err.to_string())?;
    Ok(declared.holds.as_deref() == Some("history") || !declared.files.is_empty())
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
    for file in crate::shapes::sources(root) {
        if crate::shapes::test_code(&file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&file)) else {
            continue;
        };
        let folder = Path::new(&file).parent().unwrap_or(Path::new(""));
        for schema in declared(&file, &text) {
            let schema = joined(folder, &schema);
            match std::fs::read_to_string(root.join(&schema))
                .map_err(|err| err.to_string())
                .and_then(|text| placed(&text))
            {
                Ok(true) => {}
                Ok(false) => {
                    if !rules.unplaced.contains_key(&schema) {
                        found.push(format!(
                            "{schema}: a saved format with no place: list its [[files]] or declare holds = \"history\""
                        ));
                    }
                    unplaced.insert(schema);
                }
                Err(err) => found.push(format!("{schema} isn't read: {err}")),
            }
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

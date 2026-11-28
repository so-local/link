//! A workspace's root `Cargo.toml` puts what a reader looks for first at
//! the top: its tables in the order `manifest-order.toml` gives, the
//! workspace's version first, its members one per line by path, and its
//! dependencies in one `[workspace.dependencies]` table, one line each,
//! by name. Every `package.json` opens the same way, with its `name` and
//! `version`, and its version is the workspace's. It reads only manifests, so each workspace runs it on its
//! own root.

use std::path::Path;

use serde::Deserialize;

/// `manifest-order.toml`: the places a root manifest's tables go, in order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Order {
    pub places: Vec<Place>,
    pub npm: Npm,
}

/// What every `package.json` opens with: these keys, in this order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Npm {
    pub first: Vec<String>,
}

/// One place: a table by its whole name, or every table under a family's
/// prefix, and the key the table opens with, if any.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    #[serde(default)]
    pub table: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub first: Option<String>,
}

impl Place {
    fn holds(&self, name: &str) -> bool {
        self.table.as_deref() == Some(name)
            || self.family.as_deref().is_some_and(|f| {
                name == f || name.strip_prefix(f).is_some_and(|r| r.starts_with('.'))
            })
    }

    fn label(&self) -> String {
        match (&self.table, &self.family) {
            (Some(table), _) => format!("[{table}]"),
            (_, Some(family)) => format!("[{family}.*]"),
            _ => "[?]".to_string(),
        }
    }
}

/// The order the check holds, from `manifest-order.toml`.
pub fn order() -> Order {
    toml::from_str(include_str!("../manifest-order.toml")).expect("manifest-order.toml")
}

/// The table a line opens, if it is a table's header: `[a.b]` or `[[a.b]]`.
fn header(line: &str) -> Option<String> {
    let line = line.trim();
    let inner = line.strip_prefix('[')?;
    let end = inner.rfind(']')?;
    let name = inner[..end].trim_start_matches('[').trim_end_matches(']');
    Some(
        name.chars()
            .filter(|c| !c.is_whitespace() && *c != '"')
            .collect(),
    )
}

/// The key a line sets, if it opens with one.
fn key(line: &str) -> Option<&str> {
    let (key, _) = line.split_once('=')?;
    let key = key.trim();
    (!key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then_some(key)
}

/// What's out of order in the root manifest `text`, by `order`.
pub fn problems(text: &str, order: &Order) -> Vec<String> {
    let mut problems = Vec::new();
    let mut at = 0;
    let mut section: Option<String> = None;
    let mut first_key_seen = false;
    let mut last_dependency: Option<String> = None;
    for line in text.lines() {
        if let Some(name) = header(line) {
            let Some(place) = order.places.iter().position(|p| p.holds(&name)) else {
                problems.push(match name.strip_prefix("workspace.dependencies.") {
                    Some(dep) => format!(
                        "Cargo.toml has [{name}]: write {dep} as one line in [workspace.dependencies]"
                    ),
                    None => format!("Cargo.toml has [{name}], which manifest-order.toml places nowhere"),
                });
                section = None;
                continue;
            };
            if place < at {
                problems.push(format!(
                    "Cargo.toml has [{name}] after {}: move it up, to its place in manifest-order.toml",
                    order.places[at].label()
                ));
            }
            at = at.max(place);
            section = Some(name);
            first_key_seen = false;
            continue;
        }
        let Some(name) = section.as_deref() else {
            continue;
        };
        let Some(key) = key(line) else { continue };
        let place = order.places.iter().find(|p| p.holds(name));
        if let Some(first) = place.and_then(|p| p.first.as_deref())
            && !first_key_seen
            && key != first
        {
            problems.push(format!(
                "Cargo.toml's [{name}] opens with {key}: open it with {first}"
            ));
        }
        first_key_seen = true;
        if name == "workspace.dependencies" {
            if let Some(last) = &last_dependency
                && key <= last.as_str()
            {
                problems.push(format!(
                    "Cargo.toml's [workspace.dependencies] has {key} after {last}: keep them sorted by name"
                ));
            }
            last_dependency = Some(key.to_string());
        }
    }
    problems.extend(members(text));
    problems
}

/// What's out of order in the members the manifest lists: each follows
/// the one before it by path.
fn members(text: &str) -> Vec<String> {
    let Ok(manifest) = toml::from_str::<toml::Table>(text) else {
        return vec!["Cargo.toml isn't TOML".to_string()];
    };
    let members: Vec<&str> = manifest
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
        .map(|m| m.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default();
    members
        .windows(2)
        .filter(|pair| pair[1] <= pair[0])
        .map(|pair| {
            format!(
                "Cargo.toml lists member {} after {}: keep them sorted by path, each once",
                pair[1], pair[0]
            )
        })
        .collect()
}

/// What's out of order in the root manifest of the workspace at `root`.
pub fn check(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    problems(&text, &order())
}

/// The keys of the JSON object `text`, in the order it writes them: only
/// its own, never those of an object or array inside it.
pub fn top_keys(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut depth = 0usize;
    let mut chars = text.chars();
    let mut expecting_key = false;
    while let Some(c) = chars.next() {
        match c {
            '{' | '[' => {
                depth += 1;
                expecting_key = c == '{' && depth == 1;
            }
            '}' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 1 => expecting_key = true,
            '"' => {
                let mut string = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        c => string.push(c),
                    }
                }
                if depth == 1 && expecting_key {
                    keys.push(string);
                    expecting_key = false;
                }
            }
            _ => {}
        }
    }
    keys
}

/// What's out of order in the `package.json` at `file`, whose text is
/// `text`: it opens with `order`'s keys, in their order.
pub fn npm_problems(file: &str, text: &str, order: &Order) -> Vec<String> {
    let keys = top_keys(text);
    let first = &order.npm.first;
    if keys.iter().take(first.len()).eq(first.iter()) {
        return Vec::new();
    }
    vec![format!(
        "{file} opens with {}: open it with {}",
        keys.iter()
            .take(first.len())
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
        first.join(", ")
    )]
}

/// Whether the `package.json` at `file`, whose text is `text`, carries
/// `version`, the workspace's: npm can't inherit it as a crate does, so
/// the check holds the two together.
pub fn npm_version(file: &str, text: &str, version: &str) -> Vec<String> {
    let found = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|json| {
            json.get("version")
                .and_then(|v| v.as_str())
                .map(String::from)
        });
    match found {
        Some(found) if found == version => Vec::new(),
        Some(found) => vec![format!(
            "{file} is version {found}: set it to the workspace's, {version}"
        )],
        None => vec![format!(
            "{file} has no version: set it to the workspace's, {version}"
        )],
    }
}

/// The version the root manifest of the workspace at `root` gives every
/// package.
pub fn workspace_version(root: &Path) -> String {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    let manifest: toml::Table = toml::from_str(&text).expect("Cargo.toml");
    manifest["workspace"]["package"]["version"]
        .as_str()
        .expect("[workspace.package] version")
        .to_string()
}

/// What's wrong with each `package.json` git tracks under `root`, but
/// those under a folder `skip` names, by its path from `root`: it opens
/// out of order, or carries a version other than `version`.
pub fn npm(root: &Path, skip: &[&str], version: &str) -> Vec<String> {
    let order = order();
    crate::tracked::tracked(root)
        .into_iter()
        .filter(|file| file == "package.json" || file.ends_with("/package.json"))
        .filter(|file| {
            !skip
                .iter()
                .any(|folder| file.starts_with(&format!("{folder}/")))
        })
        .flat_map(|file| {
            let text = std::fs::read_to_string(root.join(&file)).expect("package.json");
            let mut found = npm_problems(&file, &text, &order);
            found.extend(npm_version(&file, &text, version));
            found
        })
        .collect()
}

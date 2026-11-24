//! Every ready-made agent starts blank: its Home holds only headings and
//! empty fields, so the agent never takes an example for its owner's own
//! data. A CSV holds its header and no row; a Markdown file holds no list
//! item, no table row below a header and separator, and no `key: value`
//! line with a value; a TOML file holds only empty strings, zeros, `false`
//! and empty lists. Reference data a skill reads sits beside the skill,
//! and a Home's guides (`ready-mades.toml`) describe its files. Each
//! ready-made agent's tools are the ones `ready-mades.toml` pins.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// `ready-mades.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    /// The archives' folder, from the workspace's root.
    pub archives: String,
    /// Each agent's folder in an archive.
    pub agents: String,
    /// The agent's Home, in its folder.
    pub home: String,
    /// A Home's guides, by file name.
    pub guides: Vec<String>,
    /// Each agent's settings folder in an archive.
    pub settings: String,
    /// Each ready-made agent's tools, by its name.
    pub tools: BTreeMap<String, Tools>,
}

/// A ready-made agent's tools, as its archive holds them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tools {
    /// Those its spec turns on (`[behavior.tool.<name>] enabled = true`).
    pub enabled: Vec<String>,
    /// The programs its sandbox file allows (`[commands] allow`).
    pub commands: Vec<String>,
    /// The tools its sandbox file names (`[tools] names`).
    pub named: Vec<String>,
}

/// `rules` (a `ready-mades.toml`), read.
fn read_rules(rules: &Path) -> Result<Rules, String> {
    std::fs::read_to_string(rules)
        .map_err(|err| err.to_string())
        .and_then(|text| toml::from_str(&text).map_err(|err| err.to_string()))
        .map_err(|err| format!("{} isn't read: {err}", rules.display()))
}

/// Every ready-made agent under the workspace's `root` whose tools aren't
/// the ones `rules` (a `ready-mades.toml`) pins, and every pinned agent
/// that isn't there: none when each holds what it says.
pub fn tool_problems(root: &Path, rules: &Path) -> Vec<String> {
    let rules = match read_rules(rules) {
        Ok(rules) => rules,
        Err(err) => return vec![err],
    };
    let archives = root.join(&rules.archives);
    let mut found = Vec::new();
    let mut seen = Vec::new();
    for archive in folders(&archives) {
        if !archive.join("archive.toml").is_file() {
            continue;
        }
        for folder in folders(&archive.join(&rules.settings)) {
            let name = folder
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let Some(pinned) = rules.tools.get(&name) else {
                found.push(format!(
                    "{name}: its tools aren't pinned in ready-mades.toml"
                ));
                continue;
            };
            seen.push(name.clone());
            match shipped(&folder) {
                Ok(tools) => {
                    for (what, ships, says) in [
                        ("its spec turns on", &tools.enabled, &pinned.enabled),
                        ("its sandbox file allows", &tools.commands, &pinned.commands),
                        ("its sandbox file names", &tools.named, &pinned.named),
                    ] {
                        let mut says = says.clone();
                        says.sort();
                        if *ships != says {
                            found.push(format!(
                                "{name}: {what} {ships:?}, where ready-mades.toml pins {says:?}"
                            ));
                        }
                    }
                }
                Err(err) => found.push(format!("{name}: {err}")),
            }
        }
    }
    for name in rules.tools.keys().filter(|name| !seen.contains(name)) {
        found.push(format!(
            "{name}: pinned in ready-mades.toml, and no ready-made has it"
        ));
    }
    found
}

/// The tools the agent whose settings are in `folder` comes with, each
/// list sorted.
fn shipped(folder: &Path) -> Result<Tools, String> {
    let read = |file: &str| -> Result<toml::Table, String> {
        let path = folder.join(file);
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|err| format!("{file}: {err}")),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
            Err(err) => Err(format!("{file}: {err}")),
        }
    };
    let spec = read("agent.toml")?;
    let sandbox = read("sandbox.toml")?;
    let mut enabled: Vec<String> = spec
        .get("behavior")
        .and_then(|b| b.get("tool"))
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter(|(_, tool)| tool.get("enabled").and_then(toml::Value::as_bool) == Some(true))
        .map(|(name, _)| name.clone())
        .collect();
    enabled.sort();
    let strings = |table: &str, key: &str| -> Vec<String> {
        let mut found: Vec<String> = sandbox
            .get(table)
            .and_then(|t| t.get(key))
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        found.sort();
        found
    };
    Ok(Tools {
        enabled,
        commands: strings("commands", "allow"),
        named: strings("tools", "names"),
    })
}

/// Every Home file under the workspace's `root` that holds an entry, by
/// the rules in `rules` (a `ready-mades.toml`): none when each is blank.
pub fn problems(root: &Path, rules: &Path) -> Vec<String> {
    let rules = match read_rules(rules) {
        Ok(rules) => rules,
        Err(err) => return vec![err],
    };
    let archives = root.join(&rules.archives);
    let mut found = Vec::new();
    let mut homes = 0;
    for archive in folders(&archives) {
        if !archive.join("archive.toml").is_file() {
            continue;
        }
        for agent in folders(&archive.join(&rules.agents)) {
            let home = agent.join(&rules.home);
            if !home.is_dir() {
                continue;
            }
            homes += 1;
            let mut files = Vec::new();
            walk(&home, &mut files);
            for file in files {
                let name = file.file_name().unwrap_or_default().to_string_lossy();
                if rules.guides.iter().any(|guide| *guide == name) {
                    continue;
                }
                let shown = file.strip_prefix(&archives).unwrap_or(&file).display();
                match std::fs::read_to_string(&file) {
                    Ok(text) => found.extend(
                        entries(&name, &text)
                            .into_iter()
                            .map(|problem| format!("{shown}:{problem}")),
                    ),
                    Err(err) => found.push(format!("{shown}: isn't read: {err}")),
                }
            }
        }
    }
    if homes == 0 {
        found.push(format!("{}: no ready-made Home found", archives.display()));
    }
    found
}

/// The entries in `text`, a Home file named `name`, each as
/// `<line>: <what>`, or `: <what>` for the whole file: none when it is
/// blank.
pub fn entries(name: &str, text: &str) -> Vec<String> {
    match Path::new(name).extension().and_then(|e| e.to_str()) {
        Some("csv") => csv(text),
        Some("md") => markdown(text),
        Some("toml") => toml_values(text),
        _ => vec![": a Home file is Markdown, CSV or TOML, which this check reads".into()],
    }
}

/// Every row below a CSV's header.
fn csv(text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .skip(1)
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(n, _)| format!("{}: a row, where a Home holds only the header", n + 1))
        .collect()
}

/// Every list item, table row and filled `key: value` line.
fn markdown(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    // Table lines seen in a row: the header, then its separator.
    let mut table = 0;
    for (n, line) in text.lines().enumerate() {
        let line = without_comments(line);
        let line = line.trim();
        let at = n + 1;
        if line.starts_with('|') {
            table += 1;
            if table > 2 {
                found.push(format!(
                    "{at}: a table row, where a Home holds only its header"
                ));
            }
            continue;
        }
        table = 0;
        if is_list_item(line) {
            found.push(format!(
                "{at}: a list item, where a Home holds only headings"
            ));
        } else if let Some(key) = filled_key(line) {
            found.push(format!(
                "{at}: `{key}` is filled, where a Home's fields are empty"
            ));
        }
    }
    found
}

/// `line` with its HTML comments taken out.
fn without_comments(line: &str) -> String {
    let mut rest = line;
    let mut kept = String::new();
    while let Some(start) = rest.find("<!--") {
        kept.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return kept,
        }
    }
    kept.push_str(rest);
    kept
}

fn is_list_item(line: &str) -> bool {
    if ["- ", "* ", "+ "].iter().any(|mark| line.starts_with(mark)) {
        return true;
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && (line[digits..].starts_with(". ") || line[digits..].starts_with(") "))
}

/// The key of a `key: value` line with a value: a key is lowercase
/// letters, digits and underscores, starting with a letter.
fn filled_key(line: &str) -> Option<&str> {
    let (key, value) = line.split_once(':')?;
    let is_key = key.starts_with(|c: char| c.is_ascii_lowercase())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    (is_key && !value.trim().is_empty()).then_some(key)
}

/// Every TOML value that isn't empty, by its key path.
fn toml_values(text: &str) -> Vec<String> {
    let table: toml::Table = match toml::from_str(text) {
        Ok(table) => table,
        Err(err) => return vec![format!(": isn't TOML: {err}")],
    };
    let mut found = Vec::new();
    filled(&toml::Value::Table(table), "", &mut found);
    found
}

fn filled(value: &toml::Value, path: &str, found: &mut Vec<String>) {
    let empty = match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                let path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                filled(value, &path, found);
            }
            return;
        }
        toml::Value::String(text) => text.is_empty(),
        toml::Value::Integer(number) => *number == 0,
        toml::Value::Float(number) => *number == 0.0,
        toml::Value::Boolean(on) => !on,
        toml::Value::Array(items) => items.is_empty(),
        toml::Value::Datetime(_) => false,
    };
    if !empty {
        found.push(format!(
            " `{path}` is filled, where a Home's fields are empty"
        ));
    }
}

fn folders(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    found.sort();
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    for path in folders(dir) {
        walk(&path, found);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    found.extend(files);
}

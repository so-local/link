//! A command's arguments and a spec's shape are declared in TOML and
//! generated into Rust, never written in Rust: no hand-written clap
//! argument type (`#[derive(Parser)]`, `Args` or `Subcommand`), no
//! clap choice type (`ValueEnum`), whose choices `types.toml` declares, no
//! `link_spec::typed` inventory of them, no JSON Schema derived or
//! written for a Rust type (`schemars::JsonSchema`), and no hand-written
//! `Specification` impl. `shapes.toml` names the generator's own shared
//! shapes, which stay, and the files still held, which may only shrink.
//! A hidden argument's payload part the format can't say yet names its Rust
//! type in its declaration (`x-link-rust`), and every `types.toml` that
//! does is on its own list, `[rust]`, which may only shrink too.
//! Test code and examples are left out: a test may build a parser to
//! test one, and an example, run by hand through Cargo, is no program
//! Link releases or runs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

/// `shapes.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    /// Folders and files whose shapes are the generator's or its
    /// machinery's, each with why: a path from the root, or a folder's.
    pub allowed: BTreeMap<String, String>,
    /// Files that still define a shape in Rust, each with why, until
    /// their module is declared: the list only shrinks.
    #[serde(default)]
    pub held: BTreeMap<String, String>,
    /// Declarations naming a Rust type for a hidden argument's payload part
    /// (`x-link-rust`), each with why: the list only shrinks.
    #[serde(default)]
    pub rust: BTreeMap<String, String>,
}

/// What a line of Rust defines in Rust that a declaration owns, if
/// anything.
pub fn refused(line: &str) -> Option<&'static str> {
    let line = line.trim();
    if line.starts_with("//") {
        return None;
    }
    if let Some(at) = line.find("derive(") {
        let inside = &line[at + "derive(".len()..];
        let inside = inside.split(')').next().unwrap_or_default();
        let parser = inside.split(',').map(str::trim).any(|word| {
            matches!(
                word.trim_start_matches("clap::"),
                "Parser" | "Args" | "Subcommand"
            )
        });
        if parser {
            return Some("a clap argument type: declare its arguments in actions.toml");
        }
        let choices = inside
            .split(',')
            .map(str::trim)
            .any(|word| word.trim_start_matches("clap::") == "ValueEnum");
        if choices {
            return Some("a clap choice type: declare its choices as an enum in types.toml");
        }
        let schema = inside
            .split(',')
            .map(str::trim)
            .any(|word| word.trim_start_matches("schemars::") == "JsonSchema");
        if schema {
            return Some("a JSON Schema derived in Rust: declare the shape in types.toml");
        }
    }
    if line.contains("link_spec::typed") || line.contains("typed::<") {
        return Some("a link_spec::typed inventory: its parser is generated from actions.toml");
    }
    if line.starts_with("impl") && line.contains("ValueEnum for ") {
        return Some(
            "a clap choice type written in Rust: declare its choices as an enum in types.toml",
        );
    }
    if line.starts_with("impl") && line.contains("JsonSchema for ") {
        return Some("a JSON Schema written in Rust: declare the shape in types.toml");
    }
    if line.starts_with("impl")
        && (line.contains(" Specification for ") || line.contains("::Specification for "))
    {
        return Some("a hand-written Specification: declare the spec in types.toml");
    }
    None
}

/// `text`'s lines, each numbered from one, with a `derive(` spread over
/// several lines joined into the line it starts on, so its words are
/// read whole.
pub fn statements(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (i, line) in text.lines().enumerate() {
        let (number, mut joined) = match open.take() {
            Some((number, mut joined)) => {
                joined.push(' ');
                joined.push_str(line.trim());
                (number, joined)
            }
            None => (i + 1, line.to_string()),
        };
        let unclosed = joined
            .find("derive(")
            .is_some_and(|at| !joined[at..].contains(')'));
        if unclosed {
            open = Some((number, std::mem::take(&mut joined)));
        } else {
            out.push((number, joined));
        }
    }
    out.extend(open);
    out
}

/// Whether `file`, a path from the root, is test code, which may build a
/// parser to test one, or an example, which Link never releases or runs.
pub fn test_code(file: &str) -> bool {
    let path = Path::new(file);
    path.components()
        .any(|part| matches!(part.as_os_str().to_str(), Some("tests" | "examples")))
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "tests.rs" || name.ends_with("_tests.rs"))
}

/// Each problem with the Rust under `root`, by the rules in `rules` (a
/// `shapes.toml`): none when every shape is declared, or allowed or held
/// by name, and every held file still holds one.
pub fn problems(root: &Path, rules: &Path) -> Vec<String> {
    let rules: Rules = match std::fs::read_to_string(rules)
        .map_err(|err| err.to_string())
        .and_then(|text| toml::from_str(&text).map_err(|err| err.to_string()))
    {
        Ok(rules) => rules,
        Err(err) => return vec![format!("{} isn't read: {err}", rules.display())],
    };
    let allowed = |file: &str| {
        rules
            .allowed
            .keys()
            .any(|path| file == path || file.starts_with(&format!("{path}/")))
    };
    let mut found = Vec::new();
    let mut defining = BTreeSet::new();
    for file in sources(root) {
        if test_code(&file) || allowed(&file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&file)) else {
            continue;
        };
        let refusals: Vec<(usize, &str)> = statements(&text)
            .into_iter()
            .filter_map(|(line, statement)| refused(&statement).map(|why| (line, why)))
            .collect();
        if refusals.is_empty() {
            continue;
        }
        defining.insert(file.clone());
        if rules.held.contains_key(&file) {
            continue;
        }
        found.extend(
            refusals
                .into_iter()
                .map(|(line, why)| format!("{file}:{line}: {why}")),
        );
    }
    for file in rules.held.keys().filter(|file| !defining.contains(*file)) {
        found.push(format!(
            "{file} defines no shape in Rust now: take it out of shapes.toml's held"
        ));
    }
    let naming: BTreeSet<String> = tracked(root, "*types.toml")
        .into_iter()
        .filter(|file| {
            std::fs::read_to_string(root.join(file)).is_ok_and(|text| text.contains("x-link-rust"))
        })
        .collect();
    for file in naming.iter().filter(|file| !rules.rust.contains_key(*file)) {
        found.push(format!(
            "{file} names a Rust type for a shape (x-link-rust): declare it in TOML, which \
             names no Rust type"
        ));
    }
    for file in rules.rust.keys().filter(|file| !naming.contains(*file)) {
        found.push(format!(
            "{file} names no Rust type now: take it out of shapes.toml's rust"
        ));
    }
    found
}

/// The Rust files git tracks under `root`, by their paths from it.
pub(crate) fn sources(root: &Path) -> Vec<String> {
    tracked(root, "*.rs")
}

/// The files git tracks under `root` matching `pattern`, by their paths
/// from it.
#[expect(clippy::disallowed_methods, reason = "git lists the tracked files")]
fn tracked(root: &Path, pattern: &str) -> Vec<String> {
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z", "--", pattern])
        .current_dir(root)
        .output()
        .expect("git ls-files");
    assert!(listed.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|file| !file.is_empty())
        .map(str::to_string)
        .collect()
}

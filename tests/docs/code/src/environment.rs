//! Environment reads are checked against their owning declarations, not a
//! second list of the variable names expected to exist.

mod collect;
mod resolve;
mod scan;
mod source;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use link_docs_check::{Finding, Tree, fail};

#[derive(Default)]
pub struct Declarations {
    /// Each defined name, with the `variables.toml` defining it, for the
    /// generated inventory.
    pub names: BTreeMap<String, BTreeSet<PathBuf>>,
    /// Dynamic patterns are distinct from static names: a wildcard must never
    /// authorize a newly added literal read.
    pub namespaces: BTreeMap<PathBuf, BTreeSet<String>>,
    pub inputs: BTreeMap<PathBuf, BTreeSet<String>>,
    /// Each crate's variables: what it defines and declares using.
    pub crates: link_actions::Crates,
    pub errors: Vec<Finding>,
}

impl Declarations {
    /// Whether the crate in `owner` may touch `name` with `access` at
    /// `context`: it defines the variable (or its family), whose owner it
    /// is, or declares that use of it, by name or family, as a
    /// `[[uses]]` entry.
    fn allows(
        &self,
        owner: &Path,
        name: &str,
        access: link_actions::Access,
        context: link_actions::When,
    ) -> bool {
        self.crates.crates.get(owner).is_some_and(|variables| {
            variables.defining(name).is_some()
                || variables.uses.iter().any(|one| {
                    named(&one.name, name) && one.context == context && one.access.contains(&access)
                })
        })
    }
}

/// Whether `pattern`, a variable's name or a family's (`LNK_TOOL_*`),
/// names `name`.
fn named(pattern: &str, name: &str) -> bool {
    match link_actions::family(pattern) {
        Some(start) => name.len() > start.len() && name.starts_with(start),
        None => pattern == name,
    }
}

/// The variables Cargo sets for every crate it compiles (`compiled.toml`).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Compiled {
    cargo: Vec<String>,
}

/// Whether Cargo sets `name` for a compile, so reading it as the crate
/// is built needs no declaration.
fn cargo_sets(name: &str) -> bool {
    static COMPILED: std::sync::LazyLock<Compiled> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("../compiled.toml"))
            .expect("compiled.toml is checked by a test")
    });
    COMPILED
        .cargo
        .iter()
        .any(|pattern| match pattern.strip_suffix('*') {
            Some(start) => name.starts_with(start),
            None => pattern == name,
        })
}

/// Every crate's `variables.toml`, the one home of each variable it
/// defines and each it uses. The inventory uses this same collection as
/// the source audit.
pub fn declarations(tree: &Tree) -> Declarations {
    let mut result = Declarations::default();
    for (path, text) in &tree.files {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let owner = path.parent().unwrap().to_path_buf();
        match name {
            "variables.toml" if tree.files.contains_key(&owner.join("Cargo.toml")) => {
                if let Err(error) = result.crates.add(&owner, text) {
                    result
                        .errors
                        .push(fail(format!("{}: {error:#}", tree.shown(path))));
                }
            }
            "Cargo.toml" => {
                let declared = toml::from_str::<toml::Value>(text).ok().and_then(|value| {
                    let link = value.get("package")?.get("metadata")?.get("link")?.clone();
                    link.as_table()?
                        .keys()
                        .find(|key| key.starts_with("environment"))
                        .cloned()
                });
                if let Some(key) = declared {
                    result.errors.push(fail(format!(
                        "{}: [package.metadata.link.{key}] moves to the crate's variables.toml, a variable's one home",
                        tree.shown(path)
                    )));
                }
            }
            _ => {}
        }
    }
    for problem in result.crates.problems() {
        result.errors.push(fail(problem));
    }
    for (directory, variables) in &result.crates.crates {
        let file = directory.join("variables.toml");
        for name in variables.defines.keys() {
            result
                .names
                .entry(name.clone())
                .or_default()
                .insert(file.clone());
        }
        result.namespaces.insert(
            directory.clone(),
            variables.namespaces.keys().cloned().collect(),
        );
        result.inputs.insert(
            directory.clone(),
            variables.readers.keys().cloned().collect(),
        );
    }
    result
}

/// `value`'s word in a `variables.toml` (`read`, `runtime`).
fn words(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Read source text only; this check never reads the running process's inputs.
pub fn check(tree: &Tree) -> Vec<Finding> {
    let mut declared = declarations(tree);
    let (reads, mut findings) = scan::reads(tree);
    findings.extend(std::mem::take(&mut declared.errors));
    for read in reads {
        let missing = match &read.name {
            scan::Name::Static(name)
                if read.context == link_actions::When::Build && cargo_sets(name) =>
            {
                false
            }
            scan::Name::Static(name) => {
                !declared.allows(&read.owner, name, read.access, read.context)
            }
            scan::Name::Pattern(pattern) if pattern.starts_with('*') => {
                !declared.inputs.get(&read.owner).is_some_and(|apis| {
                    apis.contains(read.api.split_once("::").map_or("", |(_, rest)| rest))
                })
            }
            scan::Name::Pattern(pattern) => !declared
                .namespaces
                .get(&read.owner)
                .is_some_and(|patterns| patterns.contains(pattern)),
            scan::Name::Unresolved(_) | scan::Name::Values(_) => true,
            scan::Name::Input => !declared.inputs.get(&read.owner).is_some_and(|apis| {
                apis.contains(read.api.split_once("::").map_or("", |(_, rest)| rest))
            }),
        };
        if missing {
            let (access, context) = (words(&read.access), words(&read.context));
            findings.push(fail(format!("{}: environment {access} of {:?} at {context} in {} has no declaration in its crate; define it in the crate's variables.toml, or declare it there as a [[uses]] entry with access {access} and context {context}, or a dynamic name's function under [readers] or its pattern under [namespaces]", tree.shown(&read.path), read.name, read.api)));
        }
    }
    findings
}

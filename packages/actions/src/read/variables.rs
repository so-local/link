//! `variables.toml`: the variables a crate defines, and each use it makes.

use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

/// A variable's one definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(rename = "type")]
    pub kind: Scalar,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<toml::Value>,
    /// The secret its value is, named instead of holding a default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carries: Option<Carries>,
    /// For a family of variables, `LNK_TOOL_*`, what its `*` names: one
    /// definition for every member, `LNK_TOOL_WEB` among them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
}

/// The family a pattern, `LNK_TOOL_*`, names: its members' start, or none
/// for a variable's own name.
pub fn family(pattern: &str) -> Option<&str> {
    pattern
        .strip_suffix('*')
        .filter(|start| start.ends_with('_') && is_variable(start.trim_end_matches('_')))
}

impl Variables {
    /// The definition of `name`, its own, or its family's.
    pub fn defining(&self, name: &str) -> Option<&Definition> {
        self.defines.get(name).or_else(|| {
            self.defines.iter().find_map(|(pattern, definition)| {
                family(pattern)
                    .filter(|start| name.len() > start.len() && name.starts_with(start))
                    .map(|_| definition)
            })
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scalar {
    String,
    Boolean,
    Integer,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Carries {
    pub secret: String,
}

/// How a crate touches a variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Read,
    Set,
    Unset,
    Forward,
}

/// When it is touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum When {
    Runtime,
    Build,
    Test,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Use {
    pub name: String,
    pub access: Vec<Access>,
    pub context: When,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variables {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defines: BTreeMap<String, Definition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<Use>,
    /// Names the crate computes rather than writes out, each a pattern with
    /// a `*` and what chooses the rest: `LNK_*` read by a cloud's adapter.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub namespaces: BTreeMap<String, String>,
    /// The crate's functions, by their path in the crate, whose callers
    /// name the variables they read, each with what it reads.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub readers: BTreeMap<String, String>,
}

/// A variable's name: letters, digits and `_`, starting with a letter, its
/// letters all capitals, or for an outside program's variable all
/// lowercase, as the system's proxy variables (`https_proxy`) are. Link's
/// own, `LNK_`, are always capitals.
pub fn is_variable(name: &str) -> bool {
    let rest = |c: char| c.is_ascii_digit() || c == '_';
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && (name.chars().all(|c| c.is_ascii_uppercase() || rest(c))
            || (!name.starts_with("lnk_")
                && name.chars().all(|c| c.is_ascii_lowercase() || rest(c))))
}

pub fn read(text: &str) -> anyhow::Result<Variables> {
    let variables: Variables = toml::Value::Table(super::versioned(text)?)
        .try_into()
        .context("variables.toml")?;
    for (name, definition) in &variables.defines {
        match family(name) {
            Some(_) => ensure!(
                definition
                    .suffix
                    .as_ref()
                    .is_some_and(|said| !said.trim().is_empty()),
                "{name}: a family says what its * names, in suffix"
            ),
            None => {
                ensure!(
                    is_variable(name),
                    "{name:?} isn't a variable's name, or a family's, as LNK_TOOL_*"
                );
                ensure!(
                    definition.suffix.is_none(),
                    "{name}: only a family, LNK_TOOL_*, names a suffix"
                );
            }
        }
        ensure!(
            !definition.description.trim().is_empty(),
            "{name}: a definition says what the variable is"
        );
        ensure!(
            definition.default.is_none() || definition.carries.is_none(),
            "{name}: a variable carrying a secret has no default"
        );
    }
    for one in &variables.uses {
        ensure!(
            is_variable(&one.name) || family(&one.name).is_some(),
            "{:?} isn't a variable's name, or a family's",
            one.name
        );
        ensure!(
            !one.access.is_empty(),
            "{}: a use says how it's touched",
            one.name
        );
    }
    for (pattern, description) in &variables.namespaces {
        ensure!(
            pattern.contains('*')
                && !pattern.starts_with('*')
                && is_variable(&pattern.replace('*', "X")),
            "{pattern:?} isn't a namespace: a variable's name, its start written out, with a *"
        );
        ensure!(
            !description.trim().is_empty(),
            "{pattern}: a namespace says what chooses its names"
        );
    }
    for (reader, description) in &variables.readers {
        ensure!(
            !reader.is_empty()
                && reader.split("::").all(|part| !part.is_empty()
                    && part.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())),
            "{reader:?} isn't a function's path in its crate"
        );
        ensure!(
            !description.trim().is_empty(),
            "{reader}: a reader says which variables its callers name"
        );
    }
    Ok(variables)
}

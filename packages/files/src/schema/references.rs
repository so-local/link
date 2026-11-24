//! Names that follow history: a field or map key whose type declares
//! `x-link-ref` names a module, a command or a tool, and is read as the
//! name it has today. A document's steps move its keys; then each
//! reference it holds is followed, an old name becoming its current one
//! and a removed one refused by name. What history says is the
//! follower's ([`Follow`]); where the references are is the shape's
//! ([`Site`]).

use std::sync::OnceLock;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

/// What a reference names, as `x-link-ref` says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Reference {
    /// A module, by its name (`memory-tool`).
    Module,
    /// A command, its module first (`box show`).
    Command,
    /// A tool a model calls, by its own name (`memory`).
    Tool,
}

impl Reference {
    /// The word `x-link-ref` says it with.
    pub fn word(self) -> &'static str {
        match self {
            Reference::Module => "module",
            Reference::Command => "command",
            Reference::Tool => "tool",
        }
    }
}

impl std::str::FromStr for Reference {
    type Err = anyhow::Error;

    fn from_str(word: &str) -> anyhow::Result<Self> {
        [Reference::Module, Reference::Command, Reference::Tool]
            .into_iter()
            .find(|r| r.word() == word)
            .with_context(|| format!("x-link-ref is module, command or tool, not {word:?}"))
    }
}

/// What history says of a name.
pub trait Follow {
    /// The name `name`, a `kind`, has today: itself where history says
    /// nothing of it, and an error naming it where it's removed.
    fn follow(&self, kind: Reference, name: &str) -> anyhow::Result<String>;
}

/// History that says nothing: every name is as written.
struct Unsaid;

impl Follow for Unsaid {
    fn follow(&self, _: Reference, name: &str) -> anyhow::Result<String> {
        Ok(name.to_string())
    }
}

static FOLLOWER: OnceLock<&'static (dyn Follow + Sync)> = OnceLock::new();

/// Says what history this program's reads follow, once, at its start:
/// the release's, as `link-spec` asks for it.
pub fn set_follower(follower: &'static (dyn Follow + Sync)) {
    let _ = FOLLOWER.set(follower);
}

/// What history this program's reads follow: none said, every name as
/// written.
pub fn follower() -> &'static dyn Follow {
    match FOLLOWER.get() {
        Some(follower) => *follower,
        None => &Unsaid,
    }
}

/// A step of a site's path: a key, or any key or list entry.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Segment {
    Key(String),
    Any,
}

/// Where a document holds references: the path to a value (each string
/// of a list there too) or to a table whose keys are names.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Site {
    pub path: Vec<Segment>,
    pub kind: Reference,
    /// Whether the names are the keys of the table at `path`, rather than
    /// the value there.
    pub keys: bool,
}

impl Site {
    /// The path as a person reads it, `tools.names` or `tool.*`.
    pub fn shown(&self) -> String {
        self.path
            .iter()
            .map(|s| match s {
                Segment::Key(key) => key.as_str(),
                Segment::Any => "*",
            })
            .collect::<Vec<_>>()
            .join(".")
    }
}

/// Follows every reference at `sites` in `table`: whether any changed.
/// Two old names becoming one key are refused, never merged.
pub fn follow(table: &mut toml::Table, sites: &[Site], by: &dyn Follow) -> anyhow::Result<bool> {
    let mut changed = false;
    for site in sites {
        changed |=
            at(table, &site.path, site, by).with_context(|| format!("in {}", site.shown()))?;
    }
    Ok(changed)
}

fn at(
    table: &mut toml::Table,
    path: &[Segment],
    site: &Site,
    by: &dyn Follow,
) -> anyhow::Result<bool> {
    let Some((first, rest)) = path.split_first() else {
        return match site.keys {
            true => keys(table, site.kind, by),
            false => Ok(false),
        };
    };
    let mut changed = false;
    match first {
        Segment::Key(key) => {
            if let Some(value) = table.get_mut(key) {
                changed |= value_at(value, rest, site, by)?;
            }
        }
        Segment::Any => {
            for (_, value) in table.iter_mut() {
                changed |= value_at(value, rest, site, by)?;
            }
        }
    }
    Ok(changed)
}

fn value_at(
    value: &mut toml::Value,
    rest: &[Segment],
    site: &Site,
    by: &dyn Follow,
) -> anyhow::Result<bool> {
    match value {
        toml::Value::Table(table) => at(table, rest, site, by),
        // A list's entries each, where the path goes on or ends on it.
        toml::Value::Array(items) => {
            let rest = match rest.split_first() {
                Some((Segment::Any, rest)) => rest,
                _ => rest,
            };
            let mut changed = false;
            for item in items {
                changed |= value_at(item, rest, site, by)?;
            }
            Ok(changed)
        }
        toml::Value::String(name) if rest.is_empty() && !site.keys => {
            let now = by.follow(site.kind, name)?;
            let changed = now != *name;
            *name = now;
            Ok(changed)
        }
        _ => Ok(false),
    }
}

fn keys(table: &mut toml::Table, kind: Reference, by: &dyn Follow) -> anyhow::Result<bool> {
    let mut followed = toml::Table::new();
    let mut changed = false;
    for (name, value) in std::mem::take(table) {
        let now = by.follow(kind, &name)?;
        changed |= now != name;
        ensure!(
            !followed.contains_key(&now),
            "two names are both {now} today; keep one"
        );
        followed.insert(now, value);
    }
    *table = followed;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// History as a test says it: `memory` is `notes` today, `pager` is
    /// removed.
    struct Said;

    impl Follow for Said {
        fn follow(&self, kind: Reference, name: &str) -> anyhow::Result<String> {
            ensure!(name != "pager", "the {} pager is removed", kind.word());
            Ok(match name {
                "memory" => "notes".into(),
                other => other.into(),
            })
        }
    }

    fn site(path: &str, keys: bool) -> Site {
        Site {
            path: path
                .split('.')
                .map(|s| match s {
                    "*" => Segment::Any,
                    key => Segment::Key(key.into()),
                })
                .collect(),
            kind: Reference::Tool,
            keys,
        }
    }

    #[test]
    fn values_and_keys_follow_and_a_removed_name_is_refused() {
        let mut table: toml::Table = toml::from_str(
            "[tools]\nnames = ['memory', 'gh']\n[tools.linux]\nnames = ['memory']\n\
             [tool.memory]\nanswer = '1 KB'\n[tool.web]\n",
        )
        .unwrap();
        let sites = [
            site("tools.names", false),
            site("tools.*.names", false),
            site("tool", true),
        ];
        assert!(follow(&mut table, &sites, &Said).unwrap());
        assert_eq!(
            table["tools"]["names"],
            toml::Value::from(vec!["notes", "gh"])
        );
        assert_eq!(
            table["tools"]["linux"]["names"],
            toml::Value::from(vec!["notes"])
        );
        assert_eq!(table["tool"]["notes"]["answer"].as_str(), Some("1 KB"));
        assert!(table["tool"].get("memory").is_none() && table["tool"].get("web").is_some());
        // Current names change nothing.
        assert!(!follow(&mut table, &sites, &Said).unwrap());
        let mut removed: toml::Table = toml::from_str("[tool.pager]\n").unwrap();
        let error = follow(&mut removed, &[site("tool", true)], &Said).unwrap_err();
        assert!(
            format!("{error:#}").contains("pager is removed"),
            "{error:#}"
        );
    }

    #[test]
    fn two_old_names_becoming_one_key_are_refused() {
        let mut table: toml::Table = toml::from_str("[tool.memory]\n[tool.notes]\n").unwrap();
        assert!(follow(&mut table, &[site("tool", true)], &Said).is_err());
    }

    #[test]
    fn a_reference_is_one_of_three_words() {
        for kind in [Reference::Module, Reference::Command, Reference::Tool] {
            assert_eq!(kind.word().parse::<Reference>().unwrap(), kind);
        }
        assert!("box".parse::<Reference>().is_err());
    }
}

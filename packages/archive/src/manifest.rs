//! `archive.toml`: an archive's own record and the only file every
//! archive holds. Its `schema`, a short `title` and a one-line
//! `description` a person picks it by, where each
//! deleted thing's archive records it was deleted, its `[map]` of
//! location keys to paths in the archive, and a table per thing, keyed by
//! its selection. No IDs, digests or requirements: none is state.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An archive's record.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// What it holds, in a few words, as a list shows it (`Coach`): none
    /// for an archive a person exported.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// What it holds, in one line.
    #[serde(default)]
    pub description: String,
    /// The day the thing it holds was deleted (`2026-11-10`), when a
    /// deletion made it: only such archives expire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted: Option<String>,
    /// Where each location is in the archive, by its key in the map
    /// (`"link.agents" = "Agents"`): the default path a new machine has.
    #[serde(default)]
    pub map: BTreeMap<String, String>,
    /// Each thing it holds, by its selection (`"agent.programmer"`).
    #[serde(flatten, deserialize_with = "things")]
    pub things: BTreeMap<String, Thing>,
}

/// Every other key, each an empty table: a refusal names the key itself,
/// since a flattened map's errors carry no place in the file.
fn things<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, Thing>, D::Error> {
    use serde::de::Error;
    let raw = BTreeMap::<String, toml::Value>::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(key, value)| match value {
            toml::Value::Table(table) => match table.keys().next() {
                None => Ok((key, Thing {})),
                Some(field) => Err(D::Error::custom(format!(
                    "unknown field {field:?} in [{key:?}]: a thing's table is empty"
                ))),
            },
            _ => Err(D::Error::custom(format!(
                "unknown field {key:?}: a thing is a table, keyed by its selection"
            ))),
        })
        .collect()
}

/// A thing's table: empty, since nothing of a thing but its folders is
/// carried.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thing {}

link_files::specification!(Manifest, include_str!("manifest.schema.toml"));

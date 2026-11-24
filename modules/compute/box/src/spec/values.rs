//! A spec's checked words: a box name's start, a cloud's kind and an
//! `lnk` version, each refused unless it is one.

use std::fmt;
use std::str::FromStr;

use super::*;

/// An error unless `text` can start a box's name: what `lnk box create`
/// names boxes after (`web` makes `web-1`).
pub fn check_name(text: &str) -> Result<(), String> {
    // Room for `-<n>` after it.
    let first = format!("{text}-1");
    match crate::state::check_name(&first) {
        Ok(()) if text.len() <= link_machine::machine::NAME_MAX - 3 => Ok(()),
        _ => Err(format!(
            "{:?} can't start a box's name: lowercase letters, digits and dashes, starting \
             with a letter, at most {}",
            link_format::clean(text),
            link_machine::machine::NAME_MAX - 3
        )),
    }
}

/// A cloud's kind, as its account says it (`aws`): what its table in a
/// spec is named, `[machine.<kind>]`, and its adapter, `lnk-<kind>`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CloudKind(String);

impl CloudKind {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CloudKind {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> {
        match link_cloud_contract::check_kind(&text) {
            Ok(()) => Ok(Self(text)),
            Err(err) => Err(format!("[machine.<cloud>]: {err}")),
        }
    }
}

impl FromStr for CloudKind {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        Self::try_from(text.to_string())
    }
}

impl From<CloudKind> for String {
    fn from(kind: CloudKind) -> String {
        kind.0
    }
}

impl std::borrow::Borrow<str> for CloudKind {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CloudKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A release of `lnk` a box may install: a version, [`FLOOR`] or later.
pub(crate) fn lnk_version(text: &str) -> Result<Version, String> {
    let version: Version = text.parse().map_err(|e| format!("{e}"))?;
    if version < FLOOR {
        return Err(format!(
            "lnk {version} has no modules, which a box needs: {FLOOR} or later"
        ));
    }
    Ok(version)
}

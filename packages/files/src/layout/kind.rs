//! Whether a location steers Link, holds what a run may execute or contains
//! granted data, and which owner keeps it.

use serde::Deserialize;

/// The trust boundary of a registered location.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Control,
    Exec,
    Data,
}

/// A path, its trust boundary and its owner, declared together in the
/// catalogue. The owner is a word of `[owners]`, or `link` for Link's own.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub path: String,
    pub kind: Kind,
    pub owner: String,
}

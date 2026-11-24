//! A record of docs reviews: which owner's docs a reviewer read with the
//! review checklist, at which commit, and what came of it. `lnk-docs-survey
//! --reviews <file>` reads one to list the owners never reviewed, and how
//! many commits have touched each reviewed one since.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

/// A record: each owner, by its folder's path from the repository's top
/// (`modules/compute/box`), and its last review.
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Reviews {
    #[serde(default)]
    pub reviewed: BTreeMap<String, Review>,
}

/// One owner's last review.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Review {
    /// The commit its docs were read at.
    pub commit: String,
    /// The day it was reviewed, `2026-11-08`.
    pub date: toml::value::Datetime,
    /// The review checklist's version it followed.
    pub checklist: u32,
    /// What came of it.
    pub outcome: Outcome,
    /// What's left, in a line, for the next review.
    #[serde(default)]
    pub left: Option<String>,
}

/// What a review did.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// Nothing needed changing.
    Clean,
    /// What it found is fixed.
    Fixed,
    /// It found more than it fixed: `left` says what.
    Partial,
}

impl Reviews {
    pub fn read(path: &Path) -> Result<Reviews, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_reads_typed_and_refuses_the_unknown() {
        let record: Reviews = toml::from_str(
            "[reviewed.\"link-public/modules/compute/box\"]\ncommit = \"abc1234\"\ndate = 2026-11-08\nchecklist = 1\noutcome = \"partial\"\nleft = \"next and fail\"\n",
        )
        .unwrap();
        let review = &record.reviewed["link-public/modules/compute/box"];
        assert_eq!(review.outcome, Outcome::Partial);
        assert!(
            toml::from_str::<Reviews>(
                "[reviewed.x]\ncommit = \"a\"\ndate = 2026-11-08\nchecklist = 1\noutcome = \"clean\"\nby = \"me\"\n"
            )
            .is_err()
        );
    }
}

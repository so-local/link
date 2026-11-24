//! Security guidance authenticated by the release's signed checksums.

use std::collections::BTreeSet;

use anyhow::ensure;
use serde::{Deserialize, Serialize};

use crate::{Version, upgrades::Level};

pub const ASSET: &str = "ADVISORIES.toml";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advisories {
    #[serde(default)]
    pub advisory: Vec<Advisory>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advisory {
    pub id: String,
    pub affects: Affects,
    pub fixed: Version,
    pub level: Level,
    pub summary: String,
}

/// A half-open range, using Link's existing semantic version type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Affects {
    pub from: Version,
    pub before: Version,
}

impl Advisory {
    pub fn affects(&self, version: &Version) -> bool {
        *version >= self.affects.from && *version < self.affects.before
    }
}

impl Advisories {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let advisories: Self = toml::from_str(text)?;
        let mut ids = BTreeSet::new();
        for advisory in &advisories.advisory {
            ensure!(
                !advisory.id.is_empty()
                    && advisory
                        .id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                "invalid advisory id"
            );
            ensure!(
                ids.insert(&advisory.id),
                "duplicate advisory {}",
                advisory.id
            );
            ensure!(
                advisory.level != Level::Newer,
                "an advisory is critical or recommended"
            );
            ensure!(
                advisory.affects.from < advisory.affects.before
                    && advisory.fixed >= advisory.affects.before,
                "invalid affected range for {}",
                advisory.id
            );
            ensure!(
                !advisory.summary.trim().is_empty()
                    && !advisory.summary.chars().any(char::is_control),
                "invalid advisory summary"
            );
        }
        Ok(advisories)
    }

    pub fn affecting(&self, version: &Version) -> Vec<Advisory> {
        self.advisory
            .iter()
            .filter(|a| a.affects(version))
            .cloned()
            .collect()
    }
}

impl super::Release {
    /// Missing on releases cut before advisories were published.
    pub async fn advisories(&self) -> anyhow::Result<Advisories> {
        if !self.has(ASSET) {
            return Ok(Advisories::default());
        }
        let bytes = self.get_capped(ASSET, super::MAX_SMALL_FILE).await?;
        Advisories::parse(std::str::from_utf8(&bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "[[advisory]]\nid = 'LNK-1'\naffects = { from = '1.0.0', before = '1.2.0' }\nfixed = '1.2.0'\nlevel = 'critical'\nsummary = 'Update to fix the issue.'\n";

    #[test]
    fn strict_advisories_match_only_affected_versions() {
        let advisories = Advisories::parse(FIXTURE).unwrap();
        for (v, count) in [
            ("0.9.0", 0),
            ("1.0.0", 1),
            ("1.1.9", 1),
            ("1.2.0", 0),
            ("2.0.0", 0),
        ] {
            assert_eq!(advisories.affecting(&v.parse().unwrap()).len(), count);
        }
        for text in [
            FIXTURE.replace("critical", "newer"),
            FIXTURE.replace("fixed = '1.2.0'", "fixed = '1.1.0'"),
            format!("{FIXTURE}\nunknown = true"),
            format!("{FIXTURE}\n{FIXTURE}"),
        ] {
            assert!(Advisories::parse(&text).is_err());
        }
        Advisories::parse(include_str!("../../../../scripts/release/advisories.toml")).unwrap();
    }
}

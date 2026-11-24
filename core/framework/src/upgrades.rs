//! The built-in upgrade policy, shared by the core and its modules.

use std::{collections::BTreeSet, sync::LazyLock};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

/// One source for every configurable upgrade behavior.
pub static POLICY: LazyLock<Policy> = LazyLock::new(|| {
    Policy::parse(include_str!("../upgrades.toml")).expect("the built-in upgrade policy is valid")
});

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub notice: Notice,
}

impl Policy {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let policy: Self = toml::from_str(text)?;
        ensure!(
            policy.notice.check.seconds() > 0,
            "notice.check must be a duration"
        );
        Ok(policy)
    }
}

/// What each release says it changed, as `notes.toml` declares it.
pub static NOTES: LazyLock<Notes> = LazyLock::new(|| {
    Notes::parse(include_str!("../notes.toml")).expect("the built-in release notes are valid")
});

/// Release notes, newest release first.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notes {
    pub release: Vec<ReleaseNotes>,
}

/// One release's notes: what it changed, a sentence each.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseNotes {
    pub version: link_version::Version,
    pub notes: Vec<String>,
}

impl Notes {
    /// The notes in `text`: an error unless every release says at least
    /// one note, each one line, and the releases run newest first, each
    /// once.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let notes: Self = toml::from_str(text)?;
        for release in &notes.release {
            ensure!(
                !release.notes.is_empty(),
                "the notes of {} say nothing",
                release.version
            );
            for note in &release.notes {
                ensure!(
                    !note.trim().is_empty() && !note.contains('\n'),
                    "each note of {} is one line",
                    release.version
                );
            }
        }
        for pair in notes.release.windows(2) {
            ensure!(
                pair[0].version > pair[1].version,
                "release notes run newest first, each release once: {} is listed before {}",
                pair[0].version,
                pair[1].version
            );
        }
        Ok(notes)
    }

    /// The notes of the releases after `from`, up to and including `to`,
    /// oldest first: what upgrading from one to the other changed.
    pub fn between(
        &self,
        from: &link_version::Version,
        to: &link_version::Version,
    ) -> Vec<&ReleaseNotes> {
        self.release
            .iter()
            .rev()
            .filter(|release| release.version > *from && release.version <= *to)
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub check: Cadence,
    pub newer: Cadence,
    pub recommended: Cadence,
    pub critical: Cadence,
    pub quiet: BTreeSet<Level>,
}

impl Notice {
    pub fn cadence(&self, level: Level) -> Cadence {
        match level {
            Level::Newer => self.newer,
            Level::Recommended => self.recommended,
            Level::Critical => self.critical,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Newer,
    Recommended,
    Critical,
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Newer => "newer",
            Self::Recommended => "recommended",
            Self::Critical => "critical",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "String")]
pub struct Cadence(u64);

impl Cadence {
    pub fn seconds(self) -> u64 {
        self.0
    }
    pub fn due(self, last: u64, now: u64) -> bool {
        self.0 == 0 || last == 0 || now.saturating_sub(last) >= self.0
    }
}

impl TryFrom<String> for Cadence {
    type Error = anyhow::Error;
    fn try_from(text: String) -> anyhow::Result<Self> {
        if text == "every command" {
            return Ok(Self(0));
        }
        let (n, unit) = text
            .split_once(' ')
            .context("a cadence needs a duration or every command")?;
        let suffix = match unit {
            "second" | "seconds" => "s",
            "minute" | "minutes" => "m",
            "hour" | "hours" => "h",
            "day" | "days" => "d",
            _ => anyhow::bail!("unknown cadence unit {unit:?}"),
        };
        let millis = link_spec::duration_ms(&format!("{n}{suffix}"))?;
        ensure!(millis > 0, "a cadence must be positive");
        Ok(Self(millis / 1000))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> link_version::Version {
        text.parse().unwrap()
    }

    #[test]
    fn shipped_notes_are_valid() {
        let _ = &*NOTES;
    }

    /// The notes of every release after the one upgraded from, up to the
    /// new one, oldest first; none for the same release or a downgrade.
    #[test]
    fn notes_between_two_releases_are_those_after_the_first() {
        let notes = Notes::parse(
            "[[release]]\nversion = \"0.4.0\"\nnotes = [\"Four.\"]\n\
             [[release]]\nversion = \"0.3.0\"\nnotes = [\"Three.\"]\n\
             [[release]]\nversion = \"0.2.0\"\nnotes = [\"Two.\", \"Also two.\"]\n",
        )
        .unwrap();
        let versions = |from: &str, to: &str| -> Vec<String> {
            notes
                .between(&v(from), &v(to))
                .iter()
                .map(|r| r.version.to_string())
                .collect()
        };
        assert_eq!(versions("0.1.0", "0.3.0"), ["0.2.0", "0.3.0"]);
        assert_eq!(versions("0.2.0", "0.4.0"), ["0.3.0", "0.4.0"]);
        assert_eq!(versions("0.3.0", "0.3.0"), Vec::<String>::new());
        assert_eq!(versions("0.4.0", "0.2.0"), Vec::<String>::new());
        assert_eq!(
            versions("0.3.0-staging.1", "0.3.0"),
            ["0.3.0"],
            "a staging build's upgrade to its release says the release's notes"
        );
    }

    #[test]
    fn notes_are_whole_and_ordered() {
        let release = |version: &str, notes: &str| {
            format!("[[release]]\nversion = \"{version}\"\nnotes = {notes}\n")
        };
        assert!(Notes::parse("release = []").is_ok());
        assert!(Notes::parse("").is_err(), "the list is declared");
        assert!(Notes::parse(&release("0.2.0", "[]")).is_err());
        assert!(Notes::parse(&release("0.2.0", "[\" \"]")).is_err());
        assert!(Notes::parse(&release("0.2.0", "[\"One\\nTwo\"]")).is_err());
        assert!(Notes::parse(&release("two", "[\"Two.\"]")).is_err());
        let ascending = release("0.2.0", "[\"Two.\"]") + &release("0.3.0", "[\"Three.\"]");
        assert!(Notes::parse(&ascending).is_err());
        let twice = release("0.2.0", "[\"Two.\"]") + &release("0.2.0", "[\"Two.\"]");
        assert!(Notes::parse(&twice).is_err());
        assert!(Notes::parse(&(release("0.2.0", "[\"Two.\"]") + "more = 1\n")).is_err());
    }

    #[test]
    fn shipped_policy_is_strict_and_drives_every_cadence() {
        let text = include_str!("../upgrades.toml");
        let policy = Policy::parse(text).unwrap();
        assert!(policy.notice.critical.due(100, 100));
        assert!(!policy.notice.newer.due(100, 101));
        assert!(policy.notice.newer.due(0, 1));
        assert!(Policy::parse(&format!("{text}\nunknown = 2")).is_err());
        assert!(Policy::parse(&text.replace("1 day", "sometimes")).is_err());
        assert!(
            Policy::parse(&text.replace("check = \"1 day\"", "check = \"every command\"")).is_err()
        );
    }
}

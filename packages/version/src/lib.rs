//! A version of `lnk`, as semantic versioning writes and orders them: the
//! one type every place that reads, compares or carries one uses, `lnk
//! upgrade`, a box's install, the relay's notice of a newer `lnk`, a box
//! spec's `[lnk] version`.

use std::{cmp::Ordering, fmt, str::FromStr};

pub mod protocol;
pub use protocol::{Accepted, Protocol, Unread};

use serde::{Deserialize, Serialize};

/// A version: three numbers (`0.0.1`), then perhaps a prerelease
/// (`0.0.1-staging.202610131900`). Read by semantic versioning's rules, at
/// most one `v` before it, and no build metadata (`+...`). Ordered as they
/// are: by its numbers, then a prerelease before the release of the same
/// numbers, and prereleases part by part, numbers by value before words.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub struct Version {
    numbers: [u64; 3],
    prerelease: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Part {
    Number(u64),
    Word(String),
}

impl Version {
    /// A release: `major.minor.patch`, no prerelease.
    pub const fn new(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            numbers: [major, minor, patch],
            prerelease: Vec::new(),
        }
    }

    /// The release this is a build of: its numbers alone, so a staging
    /// build of 0.0.2 (`0.0.2-staging.202611170000`) is of release 0.0.2.
    pub fn release(&self) -> Version {
        let [major, minor, patch] = self.numbers;
        Version::new(major, minor, patch)
    }
}

/// Why a text isn't a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotAVersion(String);

impl fmt::Display for NotAVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Its first 40 characters, quoted with what can't print escaped.
        let shown: String = self.0.chars().take(40).collect();
        write!(
            f,
            "{shown:?} is not a version: three numbers, as 0.0.1, perhaps with a prerelease, \
             as 0.0.1-staging.202610131900"
        )
    }
}

impl std::error::Error for NotAVersion {}

impl FromStr for Version {
    type Err = NotAVersion;

    fn from_str(text: &str) -> Result<Version, NotAVersion> {
        let not = || NotAVersion(text.to_string());
        let bare = text.strip_prefix('v').unwrap_or(text);
        let (numbers, prerelease) = match bare.split_once('-') {
            Some((numbers, prerelease)) => (numbers, Some(prerelease)),
            None => (bare, None),
        };
        let numbers: Vec<u64> = numbers
            .split('.')
            .map(number)
            .collect::<Option<_>>()
            .ok_or_else(not)?;
        let numbers: [u64; 3] = numbers.try_into().map_err(|_| not())?;
        let prerelease = match prerelease {
            None => Vec::new(),
            Some(text) => text
                .split('.')
                .map(|p| {
                    if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) {
                        number(p).map(Part::Number)
                    } else {
                        let word = !p.is_empty()
                            && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
                        word.then(|| Part::Word(p.to_string()))
                    }
                })
                .collect::<Option<_>>()
                .ok_or_else(not)?,
        };
        Ok(Version {
            numbers,
            prerelease,
        })
    }
}

/// Digits alone, no leading zero, within u64.
fn number(text: &str) -> Option<u64> {
    let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    (digits && (text == "0" || !text.starts_with('0')))
        .then(|| text.parse().ok())
        .flatten()
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let [major, minor, patch] = self.numbers;
        write!(f, "{major}.{minor}.{patch}")?;
        for (i, part) in self.prerelease.iter().enumerate() {
            f.write_str(if i == 0 { "-" } else { "." })?;
            match part {
                Part::Number(n) => write!(f, "{n}")?,
                Part::Word(w) => f.write_str(w)?,
            }
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.numbers.cmp(&other.numbers).then_with(|| {
            match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.prerelease.cmp(&other.prerelease),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl TryFrom<String> for Version {
    type Error = NotAVersion;
    fn try_from(text: String) -> Result<Version, NotAVersion> {
        text.parse()
    }
}

impl From<Version> for String {
    fn from(version: Version) -> String {
        version.to_string()
    }
}

/// This `lnk`'s version: the core's, which every module of Link shares.
pub fn version() -> Version {
    env!("CARGO_PKG_VERSION")
        .parse()
        .expect("the workspace's version is a version")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        text.parse().unwrap()
    }

    /// Every pair in `orders.txt`, the one list `install.sh`'s and
    /// `bootstrap.sh`'s `older` are held to as well: a prerelease before
    /// its release, a later staging build after an earlier one, numbers by
    /// value before words, more parts after the same ones.
    #[test]
    fn versions_order_as_semantic_versions_do() {
        assert_eq!(v("v0.1.0"), v("0.1.0"));
        assert!(v("v1.0.0") > v("0.99.0"));
        let orders = include_str!("../orders.txt");
        let pairs: Vec<_> = orders
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .collect();
        assert!(pairs.len() > 10);
        for line in pairs {
            let [a, order, b] = line.split_whitespace().collect::<Vec<_>>()[..] else {
                panic!("orders.txt: {line}");
            };
            let order = match order {
                "<" => std::cmp::Ordering::Less,
                "=" => std::cmp::Ordering::Equal,
                _ => panic!("orders.txt: {line}"),
            };
            assert_eq!(v(a).cmp(&v(b)), order, "{line}");
            assert_eq!(v(b).cmp(&v(a)), order.reverse(), "{line}");
        }
    }

    /// A staging build is of the release its numbers name, which a
    /// release's own version already is.
    #[test]
    fn a_build_is_of_the_release_its_numbers_name() {
        assert_eq!(
            v("0.0.2-staging.202611170000.g1a2b3c").release(),
            v("0.0.2")
        );
        assert_eq!(v("0.0.2").release(), v("0.0.2"));
    }

    #[test]
    fn only_a_semantic_version_is_read() {
        for broken in [
            "",
            "garbage",
            "0.1",
            "0.1.0.0",
            "vv0.1.0",
            "0.1.0+build.1",
            "0.1.0-staging+1",
            "0.1.0-",
            "0.1.0-staging..1",
            "0.1.0-staging 1",
            "0.1.0-staging/1",
            "0.x.0-staging.1",
            "01.1.0",
            "0.01.0",
            "0.1.0-staging.01",
            "0.1.0-staging.99999999999999999999",
            "18446744073709551616.0.0",
        ] {
            assert!(broken.parse::<Version>().is_err(), "{broken}");
        }
    }

    #[test]
    fn a_version_reads_back_as_it_was_written() {
        for text in ["0.0.4", "0.0.4-staging.202610131900", "1.0.0-rc.1-a"] {
            assert_eq!(v(text).to_string(), text);
            let json = serde_json::to_string(&v(text)).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(serde_json::from_str::<Version>(&json).unwrap(), v(text));
        }
        assert_eq!(v("v0.0.4").to_string(), "0.0.4");
        assert!(serde_json::from_str::<Version>("\"0.0\"").is_err());
        assert_eq!(Version::new(1, 2, 3), v("1.2.3"));
    }
}

#[cfg(test)]
link_test_kit::program!(unit);

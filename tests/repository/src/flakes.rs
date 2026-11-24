//! The flaky tests a session met and didn't fix: `flakes.toml` beside each
//! workspace's repository check. A flake is fixed at its cause whenever it
//! can be; one left for later is written here, so the next session finds
//! it, not again by chance. Each entry names a test of a package of its
//! workspace, when it failed, on which commit, how it was run and what it
//! said.

use std::collections::BTreeSet;

use serde::Deserialize;

/// `flakes.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flakes {
    #[serde(default, rename = "flake")]
    pub flakes: Vec<Flake>,
}

/// One flaky test.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flake {
    /// Its package, a member of the workspace.
    pub package: String,
    /// The test's whole path, as `cargo test -- --exact` takes it.
    pub test: String,
    /// The day it failed, `YYYY-MM-DD`.
    pub seen: String,
    /// The commit it failed on.
    pub commit: String,
    /// How it was run when it failed, and alone.
    pub run: String,
    /// The first line it failed with.
    pub said: String,
}

/// What an entry says when its failure wasn't kept: refused, since a
/// session given the list starts from that line, and without it starts
/// a hunt (Nov 17 2026: five entries said only this, and none was fixed).
const UNCAPTURED: &[&str] = &["not captured", "not kept"];

/// What's wrong with `text`, a `flakes.toml`, when `packages` are its
/// workspace's members: none when nothing is.
pub fn problems(text: &str, packages: &BTreeSet<String>) -> Vec<String> {
    let flakes: Flakes = match toml::from_str(text) {
        Ok(flakes) => flakes,
        Err(err) => return vec![format!("flakes.toml isn't read: {err}")],
    };
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for flake in &flakes.flakes {
        let at = format!("{} {}", flake.package, flake.test);
        if !packages.contains(&flake.package) {
            found.push(format!("{at}: no package {} here", flake.package));
        }
        if flake.test.is_empty() || flake.test.contains(char::is_whitespace) {
            found.push(format!("{at}: a test is its whole path, with no spaces"));
        }
        if !is_day(&flake.seen) {
            found.push(format!("{at}: seen is a day, YYYY-MM-DD"));
        }
        if !(7..=40).contains(&flake.commit.len())
            || !flake.commit.bytes().all(|b| b.is_ascii_hexdigit())
        {
            found.push(format!("{at}: commit is a commit's hash"));
        }
        if flake.run.trim().is_empty() || flake.said.trim().is_empty() {
            found.push(format!("{at}: say how it was run and what it said"));
        }
        let said = flake.said.trim().to_lowercase();
        if UNCAPTURED.iter().any(|missing| said.starts_with(missing)) {
            found.push(format!(
                "{at}: said is the first line it failed with: an entry without it is no lead, \
                 so keep the run's output and write it then"
            ));
        }
        if !seen.insert(at.clone()) {
            found.push(format!("{at}: written twice"));
        }
    }
    found
}

/// Whether `text` is `YYYY-MM-DD`.
fn is_day(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    matches!(parts.as_slice(), [y, m, d] if y.len() == 4 && m.len() == 2 && d.len() == 2)
        && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn members() -> BTreeSet<String> {
        BTreeSet::from(["link-agent".to_string()])
    }

    const GOOD: &str = r#"
[[flake]]
package = "link-agent"
test = "foreground_stop::ctrl_c_drains_output"
seen = "2026-11-08"
commit = "7c215bc52"
run = "full cargo test --workspace failed; alone, passed 12 of 12"
said = "stop owner failed: exit status: 101"
"#;

    #[test]
    fn a_flake_written_whole_is_accepted() {
        assert!(problems(GOOD, &members()).is_empty());
        assert!(problems("", &members()).is_empty());
    }

    #[test]
    fn a_flake_of_no_package_or_without_its_facts_is_refused() {
        let wrong = GOOD
            .replace("link-agent", "link-gone")
            .replace("2026-11-08", "yesterday")
            .replace("7c215bc52", "head");
        let found = problems(&wrong, &members());
        assert_eq!(found.len(), 3, "{found:?}");
        let twice = format!("{GOOD}{GOOD}");
        assert_eq!(problems(&twice, &members()).len(), 1);
        assert_eq!(problems("[[flake]]\nfixed = true\n", &members()).len(), 1);
        let blind = GOOD.replace("stop owner failed: exit status: 101", "not captured");
        assert_eq!(problems(&blind, &members()).len(), 1);
    }
}

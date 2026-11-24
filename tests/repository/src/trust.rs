//! Whether each package of the lock files can be trusted: who publishes
//! it, how widely it is used, and the advisories against the version
//! locked. The crates of `Cargo.lock`, and the npm packages of every
//! `package-lock.json` git tracks, development tools included.
//!
//! The evidence is a file the workspace keeps (`trust.toml` beside this
//! package's manifest, or the private workspace's own), read offline by
//! [`trust`] on every test run, and written again from crates.io, npm's
//! registry and OSV by [`refresh`] when asked. A package passes when one
//! of its owners is under its registry's trusted owners
//! (`[workspace.metadata.trusted-owners]` for crates,
//! `[workspace.metadata.trusted-npm-owners]` for npm), or it is widely
//! used ([`Registry::widely_used`]); and in either case when it doesn't
//! call itself unofficial. An npm package locked only in a lock file
//! under `[workspace.metadata.trusted-npm-trees]` passes as part of that
//! tree, trusted whole for its root's sake (a tool nothing Link releases or
//! runs reads), never its authors everywhere; locked anywhere else too,
//! it passes on its own. Every advisory against a locked version, an
//! unmaintained crate's among them, fails unless its id is under
//! `[workspace.metadata.advisories]` with why; a listed one that no
//! longer applies fails too.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{Locked, Locks};

mod refresh;

pub use refresh::{refresh, refresh_in};

/// The words of a package's description that say it isn't its service's
/// own.
const UNOFFICIAL: &[&str] = &[
    "unofficial",
    "reverse-engineered",
    "reverse engineered",
    "not affiliated",
];

/// Where a package is published.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registry {
    Crates,
    Npm,
}

impl Registry {
    const ALL: [Registry; 2] = [Registry::Crates, Registry::Npm];

    /// The downloads that make a package widely used: a million in all
    /// for a crate, as crates.io counts; 100,000 in the last week for an
    /// npm package, as npm counts.
    pub fn widely_used(self) -> u64 {
        match self {
            Registry::Crates => 1_000_000,
            Registry::Npm => 100_000,
        }
    }

    /// The `[workspace.metadata]` table of its trusted owners.
    fn owners_table(self) -> &'static str {
        match self {
            Registry::Crates => "trusted-owners",
            Registry::Npm => "trusted-npm-owners",
        }
    }

    /// Its name in OSV's queries, and in an advisory's evidence.
    fn ecosystem(self) -> &'static str {
        match self {
            Registry::Crates => "crates.io",
            Registry::Npm => "npm",
        }
    }

    /// Every package its lock files hold, served by it; a problem for
    /// each one served from elsewhere.
    fn locked(self, locks: Locks) -> (Vec<Locked>, Vec<String>) {
        let mut served = Vec::new();
        let mut elsewhere = Vec::new();
        let locked: Vec<(String, Locked)> = match (self, locks.npm) {
            (Registry::Crates, _) => crate::locked(locks.cargo)
                .into_iter()
                // The workspace's own packages.
                .filter(|locked| locked.source.is_some())
                .map(|locked| ("Cargo.lock".to_string(), locked))
                .collect(),
            (Registry::Npm, Some(npm)) => crate::npm_locked(npm),
            (Registry::Npm, None) => vec![],
        };
        for (file, locked) in locked {
            let from = match self {
                Registry::Crates => locked.from_crates_io(),
                Registry::Npm => locked.from_npm(),
            };
            if from {
                served.push(locked);
            } else {
                elsewhere.push(format!(
                    "{file}: {} {} comes from {}, not {}",
                    locked.name,
                    locked.version,
                    locked.source.as_deref().unwrap_or("nowhere it names"),
                    self.ecosystem()
                ));
            }
        }
        (served, elsewhere)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Evidence {
    /// The day it was read, `YYYY-MM-DD`.
    checked: String,
    /// The crates, by name.
    crates: BTreeMap<String, Published>,
    /// The npm packages, by name.
    npm: BTreeMap<String, Published>,
    /// The advisories against a locked version, by id.
    advisories: BTreeMap<String, Advisory>,
}

impl Evidence {
    fn of(&self, registry: Registry) -> &BTreeMap<String, Published> {
        match registry {
            Registry::Crates => &self.crates,
            Registry::Npm => &self.npm,
        }
    }
}

/// Who publishes a package, and how widely it's used.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Published {
    /// Its owners' logins: a crate's team as its GitHub organization, an
    /// npm package's scope (`@types`) beside its maintainers.
    owners: Vec<String>,
    /// As [`Registry::widely_used`] counts them.
    downloads: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    unofficial: bool,
}

/// An advisory, as OSV says it: what it's about, and the locked versions
/// it's against (`crates.io ttf-parser 0.25.1`).
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Advisory {
    summary: String,
    affects: Vec<String>,
}

/// A locked package as an advisory's evidence names it.
fn affected(registry: Registry, locked: &Locked) -> String {
    format!(
        "{} {} {}",
        registry.ecosystem(),
        locked.name,
        locked.version
    )
}

/// Each package of `root`'s lock files the evidence at `evidence` doesn't
/// show trusted, each one from elsewhere than its registry, each advisory
/// against a locked version not under `[workspace.metadata.advisories]`,
/// each listed one that no longer applies, and what the evidence holds
/// that the lock files no longer do.
pub fn trust(root: &Path, evidence: &Path) -> Vec<String> {
    trust_in(Locks::of(root), evidence)
}

/// [`trust`] for lock files apart: a Cargo workspace's, and the npm
/// lock files tracked under another folder, or none.
pub fn trust_in(locks: Locks, evidence: &Path) -> Vec<String> {
    let root = locks.cargo;
    let refresh = "refresh it: run its package's refresh test, cargo test -- --ignored refresh";
    let evidence: Evidence = match std::fs::read_to_string(evidence) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", evidence.display())),
        Err(_) => return vec![format!("{}: no evidence; {refresh}", evidence.display())],
    };
    let mut problems = Vec::new();
    let mut locked = BTreeSet::new();
    let in_tree = trusted_trees(locks, &mut problems);
    for registry in Registry::ALL {
        let owners = crate::listed(root, registry.owners_table());
        let (served, elsewhere) = registry.locked(locks);
        problems.extend(elsewhere);
        let names: BTreeSet<&str> = served.iter().map(|l| l.name.as_str()).collect();
        locked.extend(served.iter().map(|l| affected(registry, l)));
        let published = evidence.of(registry);
        for name in &names {
            let Some(seen) = published.get(*name) else {
                problems.push(format!(
                    "{name}: no evidence of who publishes it; {refresh}"
                ));
                continue;
            };
            let first_party = seen.owners.iter().any(|owner| owners.contains_key(owner));
            let trusted_tree = registry == Registry::Npm && in_tree.contains(*name);
            if !first_party && !trusted_tree && seen.downloads < registry.widely_used() {
                problems.push(format!(
                    "{name}: {} downloads, under {}, and none of its owners ({}) is under [workspace.metadata.{}]",
                    seen.downloads,
                    registry.widely_used(),
                    seen.owners.join(", "),
                    registry.owners_table()
                ));
            }
            if seen.unofficial {
                problems.push(format!(
                    "{name}: its description says it is unofficial or not affiliated"
                ));
            }
        }
        for name in published.keys() {
            if !names.contains(name.as_str()) {
                problems.push(format!(
                    "{name}: in the evidence but no longer locked; {refresh}"
                ));
            }
        }
    }
    let listed = crate::listed(root, "advisories");
    for (id, advisory) in &evidence.advisories {
        if let Some(gone) = advisory.affects.iter().find(|a| !locked.contains(*a)) {
            problems.push(format!(
                "{id}: in the evidence against {gone}, no longer locked; {refresh}"
            ));
        } else if !listed.contains_key(id) {
            problems.push(format!(
                "{id} ({}): {}: upgrade past it, or list it under [workspace.metadata.advisories] with why",
                advisory.affects.join(", "),
                advisory.summary
            ));
        }
    }
    for id in listed.keys() {
        if !evidence.advisories.contains_key(id) {
            problems.push(format!(
                "{id} is listed under [workspace.metadata.advisories] but no longer applies: take it off"
            ));
        }
    }
    problems
}

/// The npm packages locked only in the lock files under
/// `[workspace.metadata.trusted-npm-trees]`, and a problem for each
/// listed one git doesn't track.
fn trusted_trees(locks: Locks, problems: &mut Vec<String>) -> BTreeSet<String> {
    let trees = crate::listed(locks.cargo, "trusted-npm-trees");
    let npm = locks.npm.map(crate::npm_locked).unwrap_or_default();
    for file in trees.keys() {
        if !npm.iter().any(|(f, _)| f == file) {
            problems.push(format!(
                "{file} is under [workspace.metadata.trusted-npm-trees] but locks nothing: take it off"
            ));
        }
    }
    let outside: BTreeSet<&str> = npm
        .iter()
        .filter(|(file, _)| !trees.contains_key(file))
        .map(|(_, l)| l.name.as_str())
        .collect();
    npm.iter()
        .filter(|(file, l)| trees.contains_key(file) && !outside.contains(l.name.as_str()))
        .map(|(_, l)| l.name.clone())
        .collect()
}

/// A crates.io login as `trusted-owners` names it: a team
/// (`github:tokio-rs:core`) as its organization.
fn owner(login: &str) -> String {
    match login.strip_prefix("github:") {
        Some(team) => team.split(':').next().unwrap_or(team).to_string(),
        None => login.to_string(),
    }
}

/// Whether a description says its package isn't its service's own.
fn unofficial(description: &str) -> bool {
    let description = description.to_lowercase();
    UNOFFICIAL.iter().any(|w| description.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_team_is_its_organization() {
        assert_eq!(owner("github:tokio-rs:core"), "tokio-rs");
        assert_eq!(owner("dtolnay"), "dtolnay");
    }

    #[test]
    fn a_package_that_says_it_is_unofficial_is() {
        assert!(unofficial("An UNOFFICIAL client of a chat service"));
        assert!(!unofficial("Bash-like brace expansion"));
    }
}

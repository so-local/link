//! The checks that read a whole Cargo workspace rather than one package's
//! code: what its lock file holds, whether each crate there is one we
//! may release, and whether each package takes the workspace's lint levels.
//! Each takes the workspace's root and returns its problems, none when it
//! passes, so the public and the private workspace run the same checks on
//! their own roots.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub mod commands;
pub mod credentials;
pub mod flags;
pub mod flakes;
pub mod legacy;
mod licenses;
pub mod lints;
pub mod manifest_order;
pub mod placed;
pub mod program_names;
pub mod ready_mades;
pub mod saved;
pub mod shapes;
pub mod state_roots;
pub mod test_kit;
pub mod tool_names;
pub mod tracked;
mod trust;
pub mod versions;

pub use licenses::{ALLOWED, licenses, npm_licenses, permitted};
pub use trust::{refresh, refresh_in, trust, trust_in};

/// Where the trust check reads lock files: a Cargo workspace's root, its
/// `Cargo.lock` and `[workspace.metadata]`, and the folder whose tracked
/// `package-lock.json` files it reads, or none. A repository whose Rust
/// roots sit apart from its npm workspaces checks each root with them.
#[derive(Clone, Copy)]
pub struct Locks<'a> {
    pub cargo: &'a Path,
    pub npm: Option<&'a Path>,
}

impl<'a> Locks<'a> {
    /// A workspace's own: its `Cargo.lock` and the npm lock files under it.
    pub fn of(root: &'a Path) -> Self {
        Locks {
            cargo: root,
            npm: Some(root),
        }
    }
}

/// One package of a lock file: a crate as `Cargo.lock` names it, or an
/// npm package as a `package-lock.json` does.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Locked {
    pub name: String,
    pub version: String,
    /// Where it comes from: for a crate `registry+…`, `sparse+…`,
    /// `git+…`, none for the workspace's own packages; for an npm package,
    /// the URL it was fetched from.
    pub source: Option<String>,
}

impl Locked {
    /// Whether crates.io serves it.
    pub fn from_crates_io(&self) -> bool {
        self.source.as_deref().is_some_and(|source| {
            source == "registry+https://github.com/rust-lang/crates.io-index"
                || source == "sparse+https://index.crates.io/"
        })
    }

    /// Whether npm's registry serves it.
    pub fn from_npm(&self) -> bool {
        self.source
            .as_deref()
            .is_some_and(|source| source.starts_with("https://registry.npmjs.org/"))
    }
}

/// Every npm package the `package-lock.json` files git tracks under
/// `root` hold, each where one names it: what's installed, development
/// tools included, but the workspace's own packages and those bundled
/// inside another's.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
pub fn npm_locked(root: &Path) -> Vec<(String, Locked)> {
    #[derive(serde::Deserialize)]
    struct Lock {
        packages: BTreeMap<String, Package>,
    }
    #[derive(serde::Deserialize)]
    struct Package {
        version: Option<String>,
        resolved: Option<String>,
        #[serde(default)]
        link: bool,
        #[serde(default, rename = "inBundle")]
        in_bundle: bool,
    }
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z", "--", "*package-lock.json"])
        .current_dir(root)
        .output()
        .expect("git ls-files");
    assert!(listed.status.success(), "git ls-files failed");
    let mut found = Vec::new();
    for file in String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|f| !f.is_empty())
    {
        let lock: Lock =
            serde_json::from_str(&read(&root.join(file))).unwrap_or_else(|e| panic!("{file}: {e}"));
        for (path, package) in lock.packages {
            let Some((_, name)) = path.rsplit_once("node_modules/") else {
                continue;
            };
            if package.link || package.in_bundle {
                continue;
            }
            found.push((
                file.to_string(),
                Locked {
                    name: name.to_string(),
                    version: package.version.unwrap_or_default(),
                    source: package.resolved,
                },
            ));
        }
    }
    found
}

/// Every crate `root/Cargo.lock` holds, for every platform.
pub fn locked(root: &Path) -> Vec<Locked> {
    #[derive(serde::Deserialize)]
    struct Lock {
        package: Vec<Package>,
    }
    #[derive(serde::Deserialize)]
    struct Package {
        name: String,
        version: String,
        source: Option<String>,
    }
    let text = read(&root.join("Cargo.lock"));
    let lock: Lock = toml::from_str(&text).expect("Cargo.lock");
    lock.package
        .into_iter()
        .map(|p| Locked {
            name: p.name,
            version: p.version,
            source: p.source,
        })
        .collect()
}

/// A table of `root/Cargo.toml`'s `[workspace.metadata]`: each key and
/// why it's there.
pub fn listed(root: &Path, table: &str) -> BTreeMap<String, String> {
    let manifest: toml::Value =
        toml::from_str(&read(&root.join("Cargo.toml"))).expect("Cargo.toml");
    manifest
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get(table))
        .and_then(toml::Value::as_table)
        .map(|t| {
            t.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// One version of each crate: a crate locked at two or more is listed in
/// `[workspace.metadata.second-versions]` with why, and a listed crate
/// is still at two, so the list never outlives its reason.
pub fn one_version(root: &Path) -> Vec<String> {
    let mut versions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for crate_ in locked(root) {
        versions
            .entry(crate_.name)
            .or_default()
            .insert(crate_.version);
    }
    let allowed = listed(root, "second-versions");
    let mut problems = Vec::new();
    for (name, at) in &versions {
        if at.len() > 1 && !allowed.contains_key(name) {
            problems.push(format!(
                "{name} is locked at {}: unify it, or list it under [workspace.metadata.second-versions] with why",
                at.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    for name in allowed.keys() {
        if versions.get(name).is_none_or(|at| at.len() < 2) {
            problems.push(format!(
                "{name} is listed under [workspace.metadata.second-versions] but locked at one version or none: take it off"
            ));
        }
    }
    problems
}

/// The file at `path`, which must be there.
fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[cfg(test)]
link_test_kit::program!(system);

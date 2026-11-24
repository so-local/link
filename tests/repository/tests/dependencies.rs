//! This workspace's lock file against the dependency rules
//! (`docs/more/developing/conventions.md`, "Dependencies"): permissive
//! licenses, one version of each crate, and packages we can trust, crates
//! and npm packages, with no advisory against them that isn't listed.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn evidence() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("trust.toml")
}

fn none(problems: Vec<String>) {
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_crate_is_permissively_licensed() {
    none(link_repository_check::licenses(&root()));
}

#[test]
fn every_npm_package_is_permissively_licensed_or_accepted() {
    let accepted = Path::new(env!("CARGO_MANIFEST_DIR")).join("npm-licenses.toml");
    none(link_repository_check::npm_licenses(&root(), &accepted));
}

#[test]
fn every_crate_is_at_one_version_or_listed() {
    none(link_repository_check::one_version(&root()));
}

#[test]
fn every_crate_is_from_a_trusted_publisher_and_maintained() {
    none(link_repository_check::trust(&root(), &evidence()));
}

/// Reads crates.io, npm's registry and OSV again and writes `trust.toml`:
/// run it when a lock file changes, then review the diff. Run by hand
/// too (`dependencies.yml`), with the trust check after it.
#[test]
#[ignore = "reads crates.io, npm and OSV: run with --ignored when a lock file changes"]
fn refresh() {
    let today = std::env::var("LNK_TODAY").expect("LNK_TODAY=YYYY-MM-DD, the day it's read");
    link_repository_check::refresh(&root(), &evidence(), &today);
}

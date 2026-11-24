//! What a run may execute: `exec`, read-only to every run, and the staging
//! directory an installer hands an adapter.

use super::{Hidden, LAYOUT};
use anyhow::ensure;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct Settings {
    exec_variable: String,
}

impl Settings {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.exec_variable.starts_with("LNK_")
                && self
                    .exec_variable
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b'_'),
            "the installation variable is a Link environment name"
        );
        Ok(())
    }
}

/// The variable by which Link hands an adapter its folder in `exec`.
pub fn exec_variable() -> &'static str {
    &LAYOUT.installation.exec_variable
}

fn given() -> anyhow::Result<Option<PathBuf>> {
    let path = std::env::var_os(exec_variable()).map(PathBuf::from);
    checked(path)
}

fn checked(path: Option<PathBuf>) -> anyhow::Result<Option<PathBuf>> {
    if let Some(path) = &path {
        ensure!(
            path.is_absolute()
                && path
                    .components()
                    .all(|part| matches!(part, Component::RootDir | Component::Normal(_))),
            "{} must name an absolute program directory without traversal",
            exec_variable()
        );
    }
    Ok(path)
}

/// Validate the adapter's handoff before dispatching a command.
pub fn check_exec_environment() -> anyhow::Result<()> {
    given().map(|_| ())
}

/// An adapter's programs, with an explicit isolated-home default. Link hands
/// it a staged directory during install and an immutable directory afterwards.
/// The handoff conveys no filesystem grant. The dispatcher validates it first.
pub fn exec_in(location: Hidden, home: &Path) -> PathBuf {
    assert_eq!(location.kind(), super::Kind::Exec, "an exec location");
    given()
        .expect("validated adapter program directory")
        .unwrap_or_else(|| location.in_home(home))
}

/// A harness's installed programs on this machine, before instance and
/// version names: `exec/harnesses/<harness>`.
pub fn harness_exec(harness: &str) -> anyhow::Result<PathBuf> {
    // A harness's name, which may be Link's own (`link`), never a path.
    super::check_owner_name(harness, "")?;
    Ok(crate::try_installation_dir()?
        .join(&LAYOUT.boundaries.exec)
        .join(super::owner_folder("harnesses", super::Part::Exec)?)
        .join(harness))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_install_handoff_refuses_relative_paths_and_traversal() {
        for invalid in ["", "exec/claude", "../exec", "/exec/../safe"] {
            assert!(checked(Some(invalid.into())).is_err(), "{invalid}");
        }
        assert_eq!(
            checked(Some("/runtime/install/claude".into())).unwrap(),
            Some("/runtime/install/claude".into())
        );
        assert_eq!(checked(None).unwrap(), None);
    }
}

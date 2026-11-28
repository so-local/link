//! This machine's agent policy, separate from installation identity.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod constants;
pub use constants::KEEP_MOVED_DAYS;

/// Machine-wide agent policy. Each optional switch keeps its existing default.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub unsandboxed: Option<bool>,
    pub keep_moved: Option<String>,
    pub shared_harness: Option<bool>,
    pub shared_bridge: Option<bool>,
    /// Where agents' blocks of ports start here: a new agent's, one
    /// arriving, and one moving off ports another program holds.
    /// Unset, [`crate::settings::FIRST_PORT`].
    pub ports_from: Option<u16>,
    #[serde(default)]
    pub storage: link_store_contract::storage::Supported,
}

impl Settings {
    fn check(&self) -> anyhow::Result<()> {
        self.retention()?;
        self.ports_from()?;
        self.storage.check()
    }

    /// [`Settings::ports_from`], or its default: an unprivileged port
    /// below the system's own range for outgoing connections, with room
    /// for a whole block below it.
    fn ports_from(&self) -> anyhow::Result<u16> {
        let from = self.ports_from.unwrap_or(crate::settings::FIRST_PORT);
        let below = crate::settings::EPHEMERAL - crate::settings::BLOCK_MAX;
        anyhow::ensure!(
            (1024..=below).contains(&from),
            "ports_from is a port from 1024 to {below}, not {from}"
        );
        Ok(from)
    }

    fn retention(&self) -> anyhow::Result<Option<u64>> {
        match self.keep_moved.as_deref() {
            None => Ok(Some(KEEP_MOVED_DAYS)),
            Some("never") => Ok(None),
            Some(text) => text
                .strip_suffix('d')
                .and_then(|n| n.parse().ok())
                .map(Some)
                .context("keep_moved is a number of days (\"30d\") or \"never\""),
        }
    }
}

/// The agent policy file on this machine.
pub fn path() -> anyhow::Result<PathBuf> {
    link_files::layout::Hidden::AgentPolicy.try_path()
}

link_files::specification!(Settings, include_str!("machine_settings.schema.toml"));

/// This machine's agent policy, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Settings>()
}

fn read_at(path: &Path) -> anyhow::Result<Settings> {
    let settings: Settings = link_files::saved::read(path)?;
    settings
        .check()
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(settings)
}

/// Machine-supported collection storage, read by the owning agent module.
pub fn storage() -> anyhow::Result<link_store_contract::storage::Supported> {
    Ok(read_at(&path()?)?.storage)
}

/// Refuse unsandboxed execution where trusted machine policy forbids it.
/// Missing trusted policy preserves the machine's default, including on a box
/// or from inside a user namespace: `--no-sandbox` is always an explicit
/// choice, while sandboxed execution remains the default.
pub fn check_unsandboxed() -> anyhow::Result<()> {
    check_unsandboxed_at(&path()?)
}

pub fn check_unsandboxed_at(path: &Path) -> anyhow::Result<()> {
    check_unsandboxed_on(path)
}

fn check_unsandboxed_on(path: &Path) -> anyhow::Result<()> {
    // Read whole and checked; the decision is the isolation contract's,
    // which Box asks too.
    read_at(path)?;
    link_sandbox_contract::isolation::check_unsandboxed_at(path)
}

pub fn shared_harness() -> anyhow::Result<bool> {
    shared_harness_at(&path()?)
}

pub fn shared_harness_at(path: &Path) -> anyhow::Result<bool> {
    Ok(read_at(path)?.shared_harness.unwrap_or(false) && cfg!(target_os = "linux"))
}

pub fn shared_bridge() -> anyhow::Result<bool> {
    shared_bridge_at(&path()?)
}

pub fn shared_bridge_at(path: &Path) -> anyhow::Result<bool> {
    Ok(read_at(path)?.shared_bridge.unwrap_or(false))
}

/// Where agents' blocks of ports start on this machine
/// (`ports_from`, else [`crate::settings::FIRST_PORT`]).
pub fn ports_from() -> anyhow::Result<u16> {
    ports_from_at(&path()?)
}

pub fn ports_from_at(path: &Path) -> anyhow::Result<u16> {
    read_at(path)?.ports_from()
}

pub fn keep_moved() -> anyhow::Result<Option<u64>> {
    read_at(&path()?)?.retention()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_policy_reads_only_its_typed_preferences() {
        let root = link_test_kit::TempDir::under(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
            "agent-policy",
        );
        let new = root.join(link_files::layout::Hidden::AgentPolicy.relative());
        link_files::write_private(&new, b"schema = 1\nunsandboxed = false\nkeep_moved = '1d'\nshared_harness = true\nshared_bridge = true\n[storage]\nconversations = ['sqlite', 'file']\n").unwrap();
        if !link_test_kit::gate::settings(&new) {
            return;
        }
        let settings = read_at(&new).unwrap();
        assert_eq!(settings.unsandboxed, Some(false));
        assert_eq!(settings.retention().unwrap(), Some(1));
        assert_eq!(settings.shared_harness, Some(true));
        assert_eq!(settings.shared_bridge, Some(true));
        assert_eq!(
            settings.storage.conversations,
            Some(vec!["sqlite".into(), "file".into()])
        );
        assert!(check_unsandboxed_on(&new).is_err());
        link_files::write_private(&new, b"schema = 1\nkeeper = false\n").unwrap();
        assert!(read_at(&new).is_err());
    }

    /// An explicit no-sandbox choice is allowed on every kind of machine
    /// unless `agents.toml` says `unsandboxed = false`; a file anyone else
    /// could have written, or one that says anything else, refuses it.
    #[test]
    fn no_sandbox_is_refused_only_where_machine_policy_says() {
        use std::os::unix::fs::PermissionsExt;
        let temp = link_test_kit::TempDir::under(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
            "machine-unsandboxed",
        );
        let path = temp.join("lnk/agents.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        if !link_test_kit::gate::settings(&path) {
            return;
        }
        check_unsandboxed_on(&path).unwrap();
        std::fs::write(&path, "schema = 1\n").unwrap();
        check_unsandboxed_on(&path).unwrap();
        std::fs::write(&path, "schema = 1\nunsandboxed = true\n").unwrap();
        check_unsandboxed_on(&path).unwrap();
        std::fs::write(&path, "schema = 1\nunsandboxed = false\n").unwrap();
        let refused = format!("{:#}", check_unsandboxed_on(&path).unwrap_err());
        assert!(refused.contains("unsandboxed = false"), "{refused}");
        std::fs::write(&path, "schema = 1\nunsandboxed = \"no\"\n").unwrap();
        assert!(check_unsandboxed_on(&path).is_err());
        // Others may write it: whatever it says, refused.
        std::fs::write(&path, "schema = 1\nunsandboxed = true\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(check_unsandboxed_on(&path).is_err());
        // Missing from a folder others write, where anyone could have
        // removed it: refused too.
        std::fs::remove_file(&path).unwrap();
        let folder = path.parent().unwrap();
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(check_unsandboxed_on(&path).is_err());
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        check_unsandboxed_on(&path).unwrap();
        // Control never follows a dotfile-manager alias into granted data,
        // regardless of the target's value or permissions.
        let kept = temp.join("dotfiles");
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::write(
            kept.join("agents.toml"),
            "schema = 1\nunsandboxed = false\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(kept.join("agents.toml"), &path).unwrap();
        assert!(check_unsandboxed_on(&path).is_err());
        std::fs::write(kept.join("agents.toml"), "schema = 1\nunsandboxed = true\n").unwrap();
        assert!(check_unsandboxed_on(&path).is_err());
        std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(check_unsandboxed_on(&path).is_err());
        std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_file(&path).unwrap();
        // Machine kind is deliberately irrelevant: the explicit switch is
        // the only machine-wide refusal.
        check_unsandboxed_on(&path).unwrap();
    }

    /// An owner's agents share one harness only where the machine says
    /// so: off with no file, off where it's left out, on with `true` on
    /// Linux alone, and anything else refused.
    #[test]
    fn a_shared_harness_is_off_unless_the_machine_says() {
        let dir = link_test_kit::TempDir::under(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
            "machine-shared",
        );
        let path = dir.join("agents.toml");
        if !link_test_kit::gate::settings(&path) {
            return;
        }
        assert!(!shared_harness_at(&path).unwrap());
        std::fs::write(&path, "schema = 1\n").unwrap();
        assert!(!shared_harness_at(&path).unwrap());
        std::fs::write(&path, "schema = 1\nshared_harness = false\n").unwrap();
        assert!(!shared_harness_at(&path).unwrap());
        std::fs::write(&path, "schema = 1\nshared_harness = true\n").unwrap();
        assert_eq!(shared_harness_at(&path).unwrap(), cfg!(target_os = "linux"));
        std::fs::write(&path, "schema = 1\nshared_harness = \"yes\"\n").unwrap();
        assert!(shared_harness_at(&path).is_err());
    }

    /// Agents' ports start at 20000 unless the machine says where, at an
    /// unprivileged port with a whole block below the system's range.
    #[test]
    fn agents_ports_start_where_the_machine_says() {
        let dir = link_test_kit::TempDir::under(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
            "machine-ports",
        );
        let path = dir.join("agents.toml");
        if !link_test_kit::gate::settings(&path) {
            return;
        }
        assert_eq!(ports_from_at(&path).unwrap(), crate::settings::FIRST_PORT);
        std::fs::write(&path, "schema = 1\nports_from = 24000\n").unwrap();
        assert_eq!(ports_from_at(&path).unwrap(), 24000);
        for refused in [
            "schema = 1\nports_from = 80\n",
            "schema = 1\nports_from = 32000\n",
            "schema = 1\nports_from = 70000\n",
        ] {
            std::fs::write(&path, refused).unwrap();
            assert!(ports_from_at(&path).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_shared_bridge_is_off_unless_the_machine_says() {
        let dir = link_test_kit::TempDir::under(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
            "machine-bridge",
        );
        let path = dir.join("agents.toml");
        if !link_test_kit::gate::settings(&path) {
            return;
        }
        assert!(!shared_bridge_at(&path).unwrap());
        std::fs::write(&path, "schema = 1\nshared_harness = true\n").unwrap();
        assert!(!shared_bridge_at(&path).unwrap());
        std::fs::write(&path, "schema = 1\nshared_bridge = false\n").unwrap();
        assert!(!shared_bridge_at(&path).unwrap());
        std::fs::write(&path, "schema = 1\nshared_bridge = true\n").unwrap();
        assert!(shared_bridge_at(&path).unwrap());
        std::fs::write(&path, "schema = 1\nshared_bridge = 1\n").unwrap();
        assert!(shared_bridge_at(&path).is_err());
    }
}

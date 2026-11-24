//! System settings, state and programs have trusted, searchable shared roots.

use crate::{
    fs::{self, Trust},
    layout::{LAYOUT, SystemRoot},
    nofollow::Dir,
};
use std::{io, os::unix::fs::MetadataExt, path::Path};

pub(super) fn prepare(path: &Path) -> io::Result<()> {
    for root in roots() {
        if path.starts_with(root) {
            return prepare_in(path, root);
        }
    }
    Ok(())
}

pub(super) fn shared(path: &Path) -> bool {
    roots().iter().any(|root| path == root)
}

fn roots() -> &'static Vec<std::path::PathBuf> {
    static ROOTS: std::sync::LazyLock<Vec<std::path::PathBuf>> = std::sync::LazyLock::new(|| {
        let mut roots: Vec<_> = [SystemRoot::Settings, SystemRoot::State, SystemRoot::Exec]
            .into_iter()
            .flat_map(|kind| {
                let root = kind.path();
                [root.clone(), real_parent(root)]
            })
            .collect();
        roots.sort();
        roots.dedup();
        roots
    });
    &ROOTS
}

fn real_parent(root: std::path::PathBuf) -> std::path::PathBuf {
    root.parent()
        .and_then(|parent| parent.canonicalize().ok())
        .map(|parent| parent.join(root.file_name().expect("a system state root has a name")))
        .unwrap_or(root)
}

fn prepare_in(path: &Path, root: &Path) -> io::Result<()> {
    if !path.starts_with(root) {
        return Ok(());
    }
    // A parent must already be root's and trusted before any creation or chmod.
    let trust = Trust {
        owners: &[0],
        deny_mode: 0o022,
        ancestors: true,
        no_link: true,
    };
    fs::check_folders(&root.join("entry"), trust).map_err(io::Error::other)?;
    std::fs::create_dir_all(root)?;
    let metadata = root.symlink_metadata()?;
    if !metadata.is_dir() || metadata.uid() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "a shared system root must belong to root",
        ));
    }
    // SAFETY: geteuid has no failure.
    if unsafe { libc::geteuid() } == 0 {
        Dir::open_unlinked(root)?.set_mode(LAYOUT.system.shared_mode)?;
    } else if metadata.mode() & 0o111 != 0o111 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "a shared system root is not searchable by this service user",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_state_root_is_selected_through_its_system_parent_alias() {
        let temp = link_test_kit::TempDir::new("system-state-alias");
        let real = temp.join("real");
        std::fs::create_dir(&real).unwrap();
        let alias = temp.join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        assert_eq!(
            real_parent(alias.join("state")),
            real.canonicalize().unwrap().join("state")
        );
        assert!(!real.join("state").exists());
    }

    #[test]
    fn system_state_preparation_keeps_the_shared_and_private_modes() {
        if !link_test_kit::gate::root() {
            return;
        }
        let temp = link_test_kit::TempDir::new("system-shared-mode");
        if !link_test_kit::gate::settings(&temp.join("probe.toml")) {
            return;
        }
        let root = temp.join("state");
        let journal = root.join("sandbox/workers");
        prepare_in(&journal, &root).unwrap();
        crate::create_private_dir(&journal).unwrap();
        let keeper = root.join("secret");
        prepare_in(&keeper, &root).unwrap();
        crate::create_private_dir(&keeper).unwrap();
        assert_eq!(
            std::fs::metadata(&root).unwrap().mode() & 0o777,
            LAYOUT.system.shared_mode
        );
        for private in [journal, keeper] {
            assert_eq!(std::fs::metadata(private).unwrap().mode() & 0o777, 0o700);
        }
    }
}

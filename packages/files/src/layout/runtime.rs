//! User runtime locations, kept separate from persistent control and data.
//!
//! Sockets live here, so its path is short wherever it is: macOS's
//! `sun_path` holds 104 bytes, and a Mac's `$TMPDIR`
//! (`/var/folders/<2>/<28>/T/`) takes about 50 of them. Where the system
//! gives no runtime folder (`XDG_RUNTIME_DIR`, Linux's `/run/user/<uid>`)
//! it is `<fallback>/<fallback-prefix><uid>`, never under `$TMPDIR`. That
//! parent is every user's, so the folder is made this user's alone (0700)
//! before anything is made in it, and one there first that is a link or
//! another user's is refused, never used ([`guard`]).

use anyhow::ensure;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Settings {
    pub folder: String,
    pub linux: PathBuf,
    pub fallback: PathBuf,
    pub fallback_prefix: String,
}

impl Settings {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        for name in [&self.folder, &self.fallback_prefix] {
            super::relative(Path::new(name))?;
            ensure!(
                Path::new(name).components().count() == 1,
                "a runtime name is one folder"
            );
        }
        for parent in [&self.linux, &self.fallback] {
            ensure!(parent.is_absolute(), "a runtime parent is absolute");
        }
        Ok(())
    }

    /// User `uid`'s runtime folder where the system gives none.
    pub fn fallback_for(&self, uid: u32) -> PathBuf {
        self.fallback.join(format!("{}{uid}", self.fallback_prefix))
    }
}

/// Where the runtime folder is, and whether it's the fallback, in a
/// folder every user writes.
fn place() -> anyhow::Result<(PathBuf, bool)> {
    let settings = &super::LAYOUT.runtime_root;
    if let Some(base) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        ensure!(base.is_absolute(), "XDG_RUNTIME_DIR must be absolute");
        return Ok((base.join(&settings.folder), false));
    }
    // SAFETY: geteuid cannot fail.
    let uid = unsafe { libc::geteuid() };
    if cfg!(target_os = "linux") {
        let base = settings.linux.join(uid.to_string());
        if base.is_dir() {
            return Ok((base.join(&settings.folder), false));
        }
    }
    Ok((settings.fallback_for(uid), true))
}

pub(super) fn path() -> anyhow::Result<PathBuf> {
    place().map(|(path, _)| path)
}

/// Before a folder at `path` is made: if it's in the fallback runtime
/// folder, that folder is made this user's alone first, so what's made
/// in it is never made through a link or a folder another user put
/// there first.
#[cfg(unix)]
pub(crate) fn guard(path: &Path) -> std::io::Result<()> {
    match place() {
        Ok((root, true)) if path.starts_with(&root) => own(&root),
        _ => Ok(()),
    }
}

/// Makes `root` this user's private folder (0700), or refuses one there
/// that is a link, not a folder, or another user's.
#[cfg(unix)]
fn own(root: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    match std::fs::DirBuilder::new().mode(0o700).create(root) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    let meta = std::fs::symlink_metadata(root)?;
    // SAFETY: geteuid cannot fail.
    let me = unsafe { libc::geteuid() };
    if !meta.is_dir() || meta.uid() != me {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "{} isn't this user's own folder (a link, or another user's); not using it",
                root.display()
            ),
        ));
    }
    if meta.mode() & 0o077 != 0 {
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    /// The fallback runtime folder is in a folder every user writes: it's
    /// made this user's alone, and one there first that is a link is
    /// refused, never followed.
    #[test]
    fn a_shared_parents_runtime_folder_is_only_this_users() {
        let temp = link_test_kit::TempDir::new("runtime-own");
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let made = temp.join("lnk-run-1");
        own(&made).unwrap();
        assert_eq!(mode(&made), 0o700);
        // One made first, open to others (as far as the umask lets it).
        let opened = temp.join("lnk-run-3");
        std::fs::DirBuilder::new()
            .mode(0o755)
            .create(&opened)
            .unwrap();
        own(&opened).unwrap();
        assert_eq!(mode(&opened), 0o700, "one opened to others is made private");
        let planted = temp.join("lnk-run-2");
        std::os::unix::fs::symlink(&made, &planted).unwrap();
        let refused = own(&planted).unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    }

    /// Without a system runtime folder, a user's is in the layout's
    /// fallback, whatever `$TMPDIR` is: on a Mac that's about 50 bytes,
    /// too many for a socket under it.
    #[test]
    fn the_fallback_runtime_folder_is_short() {
        let settings = &super::super::LAYOUT.runtime_root;
        let folder = settings.fallback_for(u32::MAX);
        assert!(folder.starts_with(&settings.fallback));
        assert!(folder.as_os_str().len() <= 24, "{}", folder.display());
    }
}

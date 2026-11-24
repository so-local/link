//! Publish an install from its own runtime staging into immutable program storage.

use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use link_files::layout::{Hidden, Runtime};
use link_files::nofollow::{Copying, Dir, Links};

use super::{Adapter, Boxed};

pub(super) fn directory(adapter: &Adapter, boxed: &Boxed) -> anyhow::Result<PathBuf> {
    if let Some(directory) = &boxed.programs {
        return Ok(directory.clone());
    }
    let key = boxed
        .identity
        .as_ref()
        .map(|(id, _)| id.as_str())
        .unwrap_or(&boxed.agent);
    ensure!(
        !key.is_empty()
            && Path::new(key).components().count() == 1
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b)),
        "an installation needs one stable agent ID or name"
    );
    let version = link_digest::sha256_of(File::open(&adapter.program)?)?;
    Ok(link_files::layout::harness_exec(&adapter.name)?
        .join(key)
        .join(version))
}

/// The installer can write only staging. Link publishes its finished output
/// after it exits; no installed tree is ever handed to a run for writing.
pub(super) struct Installation {
    pub staged: PathBuf,
    target: PathBuf,
    _lock: File,
}

impl Installation {
    pub fn prepare(adapter: &Adapter, boxed: &Boxed) -> anyhow::Result<Option<Self>> {
        let target = directory(adapter, boxed)?;
        let owner = link_files::layout::harness_exec(&adapter.name)?;
        let lock = Hidden::Installations
            .try_path()?
            .join(&adapter.name)
            .join(target.strip_prefix(owner)?);
        let lock = link_files::lock_file(&lock)?;
        match std::fs::symlink_metadata(&target) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_dir(),
                    "{} is not an installed program tree",
                    target.display()
                );
                return Ok(None);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let root = Runtime::ProgramStaging.try_path()?;
        link_files::create_private_dir(&root)?;
        let name = link_files::random_token()?;
        Dir::open_unlinked(&root)?.create_new_folder(std::ffi::OsStr::new(&name), 0o700)?;
        Ok(Some(Self {
            staged: root.join(name),
            target,
            _lock: lock,
        }))
    }

    pub fn publish(self) -> anyhow::Result<()> {
        let source = Dir::open_unlinked(&self.staged)?;
        if source.names()?.is_empty() {
            // An adapter without a private runtime still receives install on
            // every start. It has no installed program tree to cache.
            return Ok(());
        }
        publish(&source, &self.staged, &self.target)
    }
}

fn publish(source: &Dir, staged: &Path, target: &Path) -> anyhow::Result<()> {
    let parent = target.parent().context("an installation has an owner")?;
    link_files::create_private_dir(parent)?;
    let name = link_files::layout::temporary_name();
    let destination =
        Dir::open_unlinked(parent)?.create_new_folder(std::ffi::OsStr::new(&name), 0o700)?;
    let candidate = parent.join(name);
    let copied = source.copy_into(
        &destination,
        &Copying {
            skip: &|_| false,
            links: Links::Keep,
            limit: None,
            private: false,
        },
    )?;
    ensure!(
        copied.left_out.is_empty(),
        "an installation contains a socket, device or pipe"
    );
    link_settings::folders::rewrite_tree(&candidate, staged, target)?;
    finish_files(&candidate, staged, target)?;
    // The locked target does not exist. Rename publishes only a complete tree,
    // without changing programs already used by a different adapter version.
    ensure!(
        !target.exists(),
        "the installation changed before publication"
    );
    std::fs::rename(candidate, target)?;
    Ok(())
}

fn finish_files(root: &Path, staged: &Path, target: &Path) -> anyhow::Result<()> {
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.is_dir() {
                folders.push(path);
            } else if metadata.file_type().is_symlink() {
                let link = std::fs::read_link(&path)?;
                if let Ok(rest) = link.strip_prefix(staged) {
                    let replacement = path.with_file_name(link_files::layout::temporary_name());
                    std::os::unix::fs::symlink(target.join(rest), &replacement)?;
                    std::fs::rename(replacement, path)?;
                }
            } else if metadata.permissions().mode() & 0o111 != 0 {
                // A parent's writer can be inherited by a concurrent child.
                // Install a fresh inode in a helper before making it runnable.
                let replacement = path.with_file_name(link_files::layout::temporary_name());
                link_files::copy_program(&path, &replacement)?;
                std::fs::rename(replacement, path)?;
            }
        }
    }
    Ok(())
}

/// Of `write`, the folders step `step` may run what it writes in: all of
/// them for the install step of a harness that says its install runs what
/// it downloads ([`link_harness_contract::harness::Needs::install_runs_downloads`]),
/// whose `write` is the one folder it installs into; none for any other
/// step, or a harness that doesn't say so. Published, that folder is read
/// only to every run.
pub(super) fn runs_written<'a>(
    step: &str,
    needs: &link_harness_contract::harness::Needs,
    write: &'a [PathBuf],
) -> &'a [PathBuf] {
    match step == "install" && needs.install_runs_downloads {
        true => write,
        false => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the install step of a harness that says its install runs what
    /// it downloads runs what it writes, and only in the folder it
    /// installs into; no other step ever does.
    #[test]
    fn only_a_declared_install_runs_what_it_writes() {
        let staged = [PathBuf::from("/nowhere/staging/abc")];
        let mut needs = link_harness_contract::harness::Needs::default();
        for step in ["install", "configure", "run", "health"] {
            assert!(runs_written(step, &needs, &staged).is_empty(), "{step}");
        }
        needs.install_runs_downloads = true;
        assert_eq!(runs_written("install", &needs, &staged), staged);
        for step in ["configure", "run", "health", "connection"] {
            assert!(runs_written(step, &needs, &staged).is_empty(), "{step}");
        }
    }

    #[test]
    fn publication_preserves_programs_and_relocates_their_staged_paths() {
        let fixture = link_test_kit::TempDir::new("publish-programs");
        let root = fixture.canonicalize().unwrap();
        let stage = root.join("stage");
        std::fs::create_dir_all(stage.join("bin")).unwrap();
        link_test_kit::write_program(&stage.join("bin/hello"), "#!/bin/sh\necho ready\n").unwrap();
        std::fs::write(stage.join("asset"), stage.display().to_string()).unwrap();
        std::os::unix::fs::symlink(stage.join("bin/hello"), stage.join("hello")).unwrap();
        let target = root.join("programs/version");
        publish(&Dir::open_unlinked(&stage).unwrap(), &stage, &target).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("asset")).unwrap(),
            target.display().to_string()
        );
        assert_eq!(
            std::fs::read_link(target.join("hello")).unwrap(),
            target.join("bin/hello")
        );
        let output = link_test_kit::env::command(target.join("hello"), &fixture)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"ready\n");
        assert!(stage.join("bin/hello").is_file());
    }

    #[test]
    fn publication_uses_its_pinned_source_after_a_name_is_replaced() {
        let fixture = link_test_kit::TempDir::new("publish-pinned");
        let stage = fixture.join("stage");
        let elsewhere = fixture.join("private");
        std::fs::create_dir(&stage).unwrap();
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::write(stage.join("asset"), "staged").unwrap();
        std::fs::write(elsewhere.join("credential"), "never copied").unwrap();
        let source = Dir::open_unlinked(&stage).unwrap();
        std::fs::rename(&stage, fixture.join("held")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &stage).unwrap();
        let target = fixture.join("programs/version");
        publish(&source, &stage, &target).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("asset")).unwrap(),
            "staged"
        );
        assert!(!target.join("credential").exists());
        assert!(Dir::open_unlinked(&stage).is_err());
    }
}

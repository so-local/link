//! Revision-checked handoff of a stopped native schedule source.
use anyhow::{Context, ensure};
use link_files::nofollow::Dir;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Component, Path};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    source_max: u64,
}
static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
    toml::from_str(include_str!("schedules.toml")).expect("built-in native schedule limits")
});

/// Read a native source without treating corruption or unreadability as empty.
/// Discovery permits the owner's native home to be a symlink. Clearing the
/// managed copy below refuses links at every path component.
pub fn read(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    match std::fs::File::open(path) {
        Ok(file) => Ok(Some(link_files::nofollow::read_open(
            file,
            LIMITS.source_max,
        )?)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

fn directory(path: &Path, create: bool) -> anyhow::Result<Dir> {
    ensure!(
        path.is_absolute(),
        "managed schedule paths must be absolute"
    );
    // Passed through to its parent, then opened: a sandbox grants no read
    // of the folders above the managed copy (`Dir::passing`).
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => names.push(name),
            _ => anyhow::bail!("managed schedule paths cannot traverse a parent or link"),
        }
    }
    let last = names
        .pop()
        .context("managed schedule paths need a folder")?;
    let mut parent = Dir::root_to_pass()?;
    for name in names {
        parent = parent.passing(name, create, 0o700)?;
    }
    Ok(match create {
        true => parent.create_folder(last, 0o700)?,
        false => parent.folder(last)?,
    })
}

fn replace(dir: &Dir, name: &OsStr, bytes: &[u8]) -> anyhow::Result<()> {
    let temporary = link_files::random_token()?;
    let temporary = OsStr::new(&temporary);
    let mut file = dir.create_new(temporary, 0o600)?;
    let written = (|| -> anyhow::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        dir.rename(temporary, name)?;
        dir.sync_all()?;
        Ok(())
    })();
    if written.is_err() {
        let _ = dir.remove_file(temporary);
    }
    written
}

/// Preserve the complete original in the managed home's Link state, then
/// replace its stopped native source. A retry proves that archive exists.
/// The caller must stop the native runner before entering this boundary.
pub fn clear(path: &Path, archive: &Path, expected: &str, empty: &[u8]) -> anyhow::Result<()> {
    ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid native schedule revision"
    );
    let parent = directory(
        path.parent().context("native source needs a parent")?,
        false,
    )?;
    let name = path
        .file_name()
        .context("native source needs a file name")?;
    let bytes = parent.read(name, LIMITS.source_max)?;
    let archive = directory(archive, true)?;
    let _lock = archive.lock(libc::LOCK_EX)?;
    let saved_name = format!("{expected}.json");
    let saved_name = OsStr::new(&saved_name);
    if bytes == empty
        && let Ok(saved) = archive.read(saved_name, LIMITS.source_max)
    {
        ensure!(
            link_digest::sha256(&saved) == expected,
            "native schedule archive changed"
        );
        return Ok(());
    }
    ensure!(
        link_digest::sha256(&bytes) == expected,
        "native schedules changed after export; export again before clearing"
    );
    replace(&archive, saved_name, &bytes)?;
    replace(&parent, name, empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_source_limits_are_valid() {
        assert!(LIMITS.source_max > 0 && LIMITS.source_max < u64::MAX);
    }
    #[test]
    fn a_link_to_the_original_is_never_cleared() {
        let root = link_test_kit::TempDir::new("schedule-copy-link");
        let original = root.join("original.json");
        let copied = root.join("copied.json");
        let bytes = b"{\"jobs\":[{\"id\":\"inbox\"}]}";
        std::fs::write(&original, bytes).unwrap();
        std::os::unix::fs::symlink(&original, &copied).unwrap();
        assert!(
            clear(
                &copied,
                &root.join("archive"),
                &link_digest::sha256(bytes),
                b"{\"jobs\":[]}"
            )
            .is_err()
        );
        assert_eq!(std::fs::read(original).unwrap(), bytes);
    }
}

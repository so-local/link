//! An install's files copied into its agent's harness home: databases
//! with SQLite's online backup, each other file checked by its hash where
//! it lands, then every path in them naming where it was rewritten to
//! where it is.

use link_format::WithinExt;
use link_settings::folders::is_database;

use super::*;
use crate::databases;

/// Copies `from` to `to`, a file or a folder with all in it but what's in
/// `leaving`, links as links, then checks each file where it landed: a
/// database read whole, every other file by its hash. A copy there
/// already is replaced. `from` itself a link, as a home kept in a
/// dotfiles folder is, is copied through: its folder, never the link.
pub fn copy_checked(from: &Path, to: &Path, leaving: &[PathBuf]) -> anyhow::Result<()> {
    let meta = from.symlink_metadata();
    ensure!(meta.is_ok(), "{} isn't there anymore", from.display());
    if meta.is_ok_and(|m| m.file_type().is_symlink()) {
        let real =
            std::fs::canonicalize(from).within(format!("following {}, a link", from.display()))?;
        let leaving: Vec<PathBuf> = leaving
            .iter()
            .map(|l| {
                l.strip_prefix(from)
                    .map_or_else(|_| l.clone(), |rest| real.join(rest))
            })
            .collect();
        return copy_checked(&real, to, &leaving);
    }
    remove(to)?;
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    copy(from, to, leaving)?;
    check(from, to, leaving)
}

/// In the copy at `to` (a file or a folder), each path in a folder of
/// `pairs` names the same path in its second folder: every text file
/// rewritten as text, every database through SQL. How many files and
/// values changed.
pub fn rewrite(to: &Path, pairs: &[(PathBuf, PathBuf)]) -> anyhow::Result<usize> {
    let mut changed = 0;
    let texts: Vec<(String, String)> = pairs
        .iter()
        .map(|(was, now)| (was.display().to_string(), now.display().to_string()))
        .collect();
    let mut left = vec![to.to_path_buf()];
    let portable = link_files::state::path(to, link_files::state::Area::Context);
    while let Some(at) = left.pop() {
        // Portable originals and sourced revisions remain immutable evidence.
        // Only the native runtime's derived paths are relocated.
        if at.starts_with(&portable) {
            continue;
        }
        let Ok(meta) = at.symlink_metadata() else {
            continue;
        };
        if meta.is_dir() {
            for entry in std::fs::read_dir(&at)?.flatten() {
                left.push(entry.path());
            }
        } else if meta.is_file() && is_main_database(&at) {
            changed += databases::rewrite(&at, &texts)?;
        } else if meta.is_file() {
            for (was, now) in pairs {
                changed += usize::from(link_settings::folders::rewrite_paths(&at, was, now)?);
            }
        }
    }
    Ok(changed)
}

/// Whether `path` is a database itself, not its log or journal: it's
/// copied by SQLite, which carries those in it.
fn is_main_database(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 16];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok_and(|()| &head == b"SQLite format 3\0")
}

fn copy(from: &Path, to: &Path, leaving: &[PathBuf]) -> anyhow::Result<()> {
    if leaving.iter().any(|l| l == from) {
        return Ok(());
    }
    let meta = from.symlink_metadata()?;
    if meta.file_type().is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(from)?, to)?;
    } else if meta.is_dir() {
        std::fs::create_dir(to).within(format!("making {}", to.display()))?;
        std::fs::set_permissions(to, meta.permissions())?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy(&entry.path(), &to.join(entry.file_name()), leaving)?;
        }
    } else if meta.is_file() && is_main_database(from) {
        databases::backup(from, to).within(format!("copying {}", from.display()))?;
        std::fs::set_permissions(to, meta.permissions())?;
    } else if meta.is_file() && !is_database(from) {
        std::fs::copy(from, to).within(format!("copying {}", from.display()))?;
    }
    // A database's log is in its copy; sockets and pipes are a running
    // harness's: not copied.
    Ok(())
}

fn check(from: &Path, to: &Path, leaving: &[PathBuf]) -> anyhow::Result<()> {
    if leaving.iter().any(|l| l == from) {
        return Ok(());
    }
    let meta = from.symlink_metadata()?;
    if meta.is_dir() {
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            check(&entry.path(), &to.join(entry.file_name()), leaving)?;
        }
    } else if meta.is_file() && is_main_database(from) {
        databases::check(to)?;
    } else if meta.is_file() && !is_database(from) {
        let hash = |p: &Path| -> anyhow::Result<String> {
            Ok(link_digest::sha256_of(std::fs::File::open(p)?)?)
        };
        if hash(from)? != hash(to)? {
            bail!(
                "{} changed as it was copied: run lnk agent migrate again",
                from.display()
            );
        }
    }
    Ok(())
}

/// Removes `path`, a folder with all in it or a file, never following a
/// link; nothing if it isn't there.
pub(super) fn remove(path: &Path) -> anyhow::Result<()> {
    match path.symlink_metadata() {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path)?,
        Ok(_) => std::fs::remove_file(path)?,
        Err(_) => {}
    }
    Ok(())
}

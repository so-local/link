//! Expired atomic-write staging is removed from Safes and only listed in data.

use crate::{
    layout::{self, Hidden, Home, Kind, Root, System},
    nofollow::{self, Dir},
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
mod tests;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct State {
    swept: u64,
}

crate::specification!(State, include_str!("maintenance/schema.toml"));

pub fn now() -> anyhow::Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

/// A folder or file a sweep couldn't read or change, left as it was.
#[derive(Debug)]
pub struct Unswept {
    pub path: PathBuf,
    pub error: io::Error,
}

/// Scan this user's Safes at most once per configured period. Best
/// effort: what it can't read, or what is deeper than it walks, is left
/// as it is and returned, and the sweep is recorded all the same, so a
/// folder it can't read is met once a period, never on every command.
pub fn sweep(now: u64) -> anyhow::Result<Vec<Unswept>> {
    let hidden = Root::Hidden.try_path()?;
    let mut safes = vec![
        hidden.join(&layout::LAYOUT.boundaries.hidden_safe),
        Home::Safe.try_path()?,
    ];
    for safe in layout::system_safes() {
        // A systemd private alias may be inaccessible to this user. It
        // cannot authorize that user's cleanup.
        if let Err(error) = safe.symlink_metadata()
            && error.kind() == io::ErrorKind::PermissionDenied
        {
            continue;
        }
        safes.push(safe);
    }
    sweep_in(&safes, &Hidden::Maintenance.at(&hidden), now)
}

fn sweep_in(safes: &[PathBuf], state: &Path, now: u64) -> anyhow::Result<Vec<Unswept>> {
    let _lock = crate::lock_file(&state.with_extension("lock"))?;
    let parent = Dir::open_unlinked(state.parent().expect("maintenance has a parent"))?;
    // A record that can't be read is a sweep never recorded: this one
    // writes it again.
    let last = parent
        .open_file(
            state.file_name().expect("maintenance has a name"),
            libc::O_RDONLY,
            0,
        )
        .ok()
        .and_then(|mut file| {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut file, &mut text).ok()?;
            crate::saved::parse::<State>(&text).ok()
        });
    if last.is_some_and(|last| {
        now.saturating_sub(last.swept) < layout::LAYOUT.temporary.sweep_period_seconds
    }) {
        return Ok(Vec::new());
    }
    let mut scan = Scan::new(now, true);
    for safe in safes {
        visit_root(safe, &mut scan);
    }
    crate::saved::write(state, &State { swept: now })?;
    Ok(scan.unswept)
}

/// Old staging in this user's data, never removed. Linked directories and
/// another user's files are not traversed; a folder that can't be read is
/// an error, naming it.
pub fn staged_data(now: u64) -> anyhow::Result<Vec<PathBuf>> {
    let home = Root::Home.try_path()?;
    let mut roots = vec![
        Root::Hidden
            .try_path()?
            .join(&layout::LAYOUT.boundaries.data),
    ];
    roots.extend(
        Home::ALL
            .iter()
            .filter(|p| p.kind() == Kind::Data)
            .map(|p| p.at(&home)),
    );
    roots.extend(
        System::ALL
            .iter()
            .filter(|p| p.kind() == Kind::Data)
            .map(|p| p.path()),
    );
    let mut scan = Scan::new(now, false);
    for root in roots {
        visit_root(&root, &mut scan);
    }
    if let Some(unswept) = scan.unswept.into_iter().next() {
        anyhow::bail!("{}: {}", unswept.path.display(), unswept.error);
    }
    let mut found = scan.found;
    found.sort();
    found.dedup();
    Ok(found)
}

/// One walk: when, whether it removes, and what it found and left.
struct Scan {
    now: u64,
    remove: bool,
    found: Vec<PathBuf>,
    unswept: Vec<Unswept>,
}

impl Scan {
    fn new(now: u64, remove: bool) -> Self {
        Self {
            now,
            remove,
            found: Vec::new(),
            unswept: Vec::new(),
        }
    }

    fn left(&mut self, path: PathBuf, error: io::Error) {
        self.unswept.push(Unswept { path, error });
    }
}

fn visit_root(root: &Path, scan: &mut Scan) {
    let metadata = match root.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => return scan.left(root.to_path_buf(), error),
    };
    // SAFETY: geteuid has no failure.
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        return;
    }
    match Dir::open_unlinked(root) {
        Ok(dir) => visit(&dir, root, 0, scan),
        Err(error) => scan.left(root.to_path_buf(), error),
    }
}

fn visit(dir: &Dir, path: &Path, depth: usize, scan: &mut Scan) {
    if depth > nofollow::DEEPEST {
        return scan.left(
            path.to_path_buf(),
            io::Error::other(format!(
                "deeper than {} folders: not swept past that",
                nofollow::DEEPEST
            )),
        );
    }
    let (metadata, names) = match dir.metadata().and_then(|m| Ok((m, dir.names()?))) {
        Ok(read) => read,
        Err(error) => return scan.left(path.to_path_buf(), error),
    };
    // SAFETY: geteuid has no failure.
    if metadata.uid() != unsafe { libc::geteuid() } || (scan.remove && metadata.mode() & 0o022 != 0)
    {
        return;
    }
    let mut changed = false;
    for name in names {
        match entry(dir, path, &name, u64::from(metadata.uid()), depth, scan) {
            Ok(removed) => changed |= removed,
            Err(error) => scan.left(path.join(&name), error),
        }
    }
    if changed && let Err(error) = dir.sync_all() {
        scan.left(path.to_path_buf(), error);
    }
}

/// One name in `dir`: a folder walked, expired staging listed or removed.
/// Whether it removed a file.
fn entry(
    dir: &Dir,
    path: &Path,
    name: &std::ffi::OsStr,
    owner: u64,
    depth: usize,
    scan: &mut Scan,
) -> io::Result<bool> {
    let gone = |error: &io::Error| error.kind() == io::ErrorKind::NotFound;
    let stat = match dir.stat(name) {
        Ok(stat) => stat,
        Err(error) if gone(&error) => return Ok(false),
        Err(error) => return Err(error),
    };
    if stat.kind == nofollow::Kind::Folder {
        match dir.folder(name) {
            Ok(child) => visit(&child, &path.join(name), depth + 1, scan),
            Err(error)
                if gone(&error)
                    || matches!(error.raw_os_error(), Some(libc::ENOTDIR | libc::ELOOP)) => {}
            Err(error) => return Err(error),
        }
        return Ok(false);
    }
    if stat.kind != nofollow::Kind::File
        || stat.uid != owner
        || !name.to_str().is_some_and(layout::is_temporary)
        || scan.now.saturating_sub(stat.mtime) < layout::LAYOUT.temporary.max_age_seconds
    {
        return Ok(false);
    }
    if !scan.remove {
        scan.found.push(path.join(name));
        return Ok(false);
    }
    let file = match dir.open_file(name, libc::O_RDONLY, 0) {
        Ok(file) => file,
        Err(error) if gone(&error) => return Ok(false),
        Err(error) => return Err(error),
    };
    let modified = file
        .metadata()?
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    if !crate::fs::try_lock(&file)?
        || scan.now.saturating_sub(modified) < layout::LAYOUT.temporary.max_age_seconds
    {
        return Ok(false);
    }
    match dir.remove_file(name) {
        Ok(()) => Ok(true),
        Err(error) if gone(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

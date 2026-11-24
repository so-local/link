//! A harness's state put in place: restored from an archive, or moved
//! from one home to another, never through a link.

use super::*;
use link_format::WithinExt;

/// Replaces the harness's `state` in its home `dir` with the one in
/// `archive` (written by [`archive_state`]): unpacked first in a folder
/// made new for this restore, `~/.lnk/safe/agents/transfers/restore/restoring-<random>`, out of
/// every agent's reach (two restores at once never remove each other's,
/// and one left by a restore cut short is removed once it's a week old,
/// [`STALE_RESTORE`]), so a broken archive leaves the state as it was. What `state.exclude` names (its
/// install, its keys) stays. Paths leaving that folder are refused by
/// the unpacking ([`unpack_in`]), and a link in it as `links` says: the
/// agent's own folder's are refused, symbolic or hard (two names of one
/// file: its conversation's events added to would change its
/// instructions too), as a backup sits in Home, which the agent may
/// write. The harness writes its home, and may still run (a process that
/// outlived `lnk agent stop`): the home is changed only through folder
/// handles ([`remove_paths`], [`move_paths`]).
pub fn replace_state(
    archive: impl Read,
    dir: &Path,
    state: &State,
    links: Links,
) -> anyhow::Result<()> {
    let agents = link_files::layout::Hidden::Restoring.path();
    // Where missing, made as an agent's folder makes it: only the user's.
    std::os::unix::fs::DirBuilderExt::mode(std::fs::DirBuilder::new().recursive(true), 0o700)
        .create(&agents)
        .within(format!("creating {}", agents.display()))?;
    let top = Dir::open(&agents).within(format!("opening {}", agents.display()))?;
    remove_stale_restores(&top, STALE_RESTORE);
    let name = format!("{RESTORING}{}", &link_files::random_token()?[..12]);
    let name = OsStr::new(&name);
    // Made new: a name already there is never taken over.
    top.create_new_folder(name, 0o700).within(format!(
        "making a folder in {} to restore into",
        agents.display()
    ))?;
    let staging = agents.join(name);
    let replaced = (|| -> anyhow::Result<()> {
        unpack_in(archive, &staging, links).within("unpacking the backup")?;
        put_state(&staging, dir, state)
    })();
    let _ = top.remove_all(name);
    replaced
}

/// How a restore's folder in `~/.lnk/safe/agents/transfers/restore` begins ([`replace_state`]).
const RESTORING: &str = "restoring-";

/// How old a restore's folder is before it's surely one a restore cut
/// short left, and removed: no restore takes a week.
const STALE_RESTORE: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

/// Removes from `agents` each restore's folder ([`RESTORING`]) older
/// than `age`, by its handle, never through a link; one in use by a
/// restore running now is younger, and stays.
pub(super) fn remove_stale_restores(agents: &Dir, age: std::time::Duration) {
    let Some(before) = std::time::SystemTime::now()
        .checked_sub(age)
        .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
    else {
        return;
    };
    for name in agents.names().unwrap_or_default() {
        if !name.to_str().is_some_and(|n| n.starts_with(RESTORING)) {
            continue;
        }
        if agents
            .stat(&name)
            .is_ok_and(|stat| stat.kind == Kind::Folder && stat.mtime < before.as_secs())
        {
            let _ = agents.remove_all(&name);
        }
    }
}

/// Replaces the harness's `state` in its home `dir` with what of it is in
/// `from`, a folder of Link's own: what `state` names in `dir` is removed,
/// but what it excludes (its install, its keys), then moved there from
/// `from`. Nothing else in `from` comes.
pub(super) fn put_state(from: &Path, dir: &Path, state: &State) -> anyhow::Result<()> {
    link_files::create_private_dir(dir)?;
    let excluded = in_home(dir, &state.exclude)?;
    remove_paths(dir, &in_home(dir, &state.include)?, &excluded)?;
    if from.symlink_metadata().is_ok_and(|m| m.is_dir()) {
        move_state(from, dir, state)?;
    }
    Ok(())
}

/// Moves the harness's `state` from its home `from` into the home `to`:
/// what `state.include` names, but not what `state.exclude` does (its
/// install, its keys), which stays. Folders already in `to` are merged
/// into, a file in the way is replaced; links are moved as links.
pub fn move_state(from: &Path, to: &Path, state: &State) -> anyhow::Result<()> {
    let excluded = in_home(from, &state.exclude)?;
    move_paths(from, to, &in_home(from, &state.include)?, &excluded)
}

/// `includes`, paths in the folder `from`, moved to the same place in the
/// folder `to`, but not what's in `excluded`. Both are walked from folder
/// handles, one name at a time, and each name moved with `renameat` from
/// the folder holding it into the one it goes to: in `to`, a link on the
/// way to a state path, whenever it came, is refused, and one in its place
/// is replaced, never followed; in `from`, nothing is moved through one.
pub(super) fn move_paths(
    from: &Path,
    to: &Path,
    includes: &[PathBuf],
    excluded: &[PathBuf],
) -> anyhow::Result<()> {
    if includes.is_empty() {
        return Ok(());
    }
    let source = match Dir::open(from) {
        Ok(source) => source,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).within(format!("opening {}", from.display())),
    };
    let target = Dir::open(to).within(format!("opening {}", to.display()))?;
    'include: for include in includes {
        let rel = include.strip_prefix(from)?;
        let names: Vec<&OsStr> = rel.iter().collect();
        let Some((last, folders)) = names.split_last() else {
            // The whole folder.
            move_in(&source, from, &target, to, excluded)?;
            continue;
        };
        let (mut source, mut target) = (source.try_clone()?, target.try_clone()?);
        let mut at = to.to_path_buf();
        for name in folders {
            at.push(name);
            source = match source.folder(name) {
                Ok(next) => next,
                // Missing, not a folder, or a link: nothing of it comes.
                Err(_) => continue 'include,
            };
            let mode = source.metadata()?.permissions().mode() & 0o777;
            target = match target.create_folder(name, mode) {
                Ok(next) => next,
                Err(_) if target.kind(name).is_ok_and(|k| k == Kind::Link) => bail!(
                    "{} is a link: not moving {} through it",
                    at.display(),
                    to.join(rel).display()
                ),
                Err(err) => return Err(err).within(format!("creating {}", at.display())),
            };
        }
        move_at(&source, &target, last, include, &to.join(rel), excluded)?;
    }
    Ok(())
}

/// Moves what's in the open folder `from` (at `from_path`) into the open
/// folder `into` (at `to_path`), but not what's in `keep`.
pub(super) fn move_in(
    from: &Dir,
    from_path: &Path,
    into: &Dir,
    to_path: &Path,
    keep: &[PathBuf],
) -> anyhow::Result<()> {
    let names = from
        .names()
        .within(format!("reading {}", from_path.display()))?;
    for name in names {
        let (from_child, to_child) = (from_path.join(&name), to_path.join(&name));
        move_at(from, into, &name, &from_child, &to_child, keep)?;
    }
    Ok(())
}

/// Moves `name` from the folder `from` (at `from_path`) to the folder
/// `into` (at `to_path`), but not what's in `keep` (nor the folders
/// holding it, which are made again in `into`, with the same
/// permissions); into a folder already there. What's in the way is
/// replaced: a link there is removed or renamed over, never followed.
fn move_at(
    from: &Dir,
    into: &Dir,
    name: &OsStr,
    from_path: &Path,
    to_path: &Path,
    keep: &[PathBuf],
) -> anyhow::Result<()> {
    if keep.iter().any(|k| k == from_path) {
        return Ok(());
    }
    let moving = || format!("moving {} to {}", from_path.display(), to_path.display());
    let stat = match from.stat(name) {
        Ok(stat) => stat,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).within(moving()),
    };
    let there = into.kind(name).ok();
    let into_folder = stat.kind == Kind::Folder
        && (keep.iter().any(|k| k.starts_with(from_path)) || there == Some(Kind::Folder));
    if into_folder {
        if there.is_some_and(|k| k != Kind::Folder) {
            gone(into.remove_file(name), to_path)?;
        }
        let mode = stat.mode & 0o777;
        let creating = || format!("creating {}", to_path.display());
        let target = into.create_folder(name, mode).within(creating())?;
        target.set_mode(mode).within(creating())?;
        let source = from.folder(name).within(moving())?;
        move_in(&source, from_path, &target, to_path, keep)?;
        // Gone once empty; one holding what stays, stays.
        let _ = from.remove_folder(name);
        return Ok(());
    }
    match there {
        Some(Kind::Folder) => remove_at(into, name, to_path, &[])?,
        Some(_) => gone(into.remove_file(name), to_path)?,
        None => {}
    }
    from.rename_to(name, into, name).within(moving())
}

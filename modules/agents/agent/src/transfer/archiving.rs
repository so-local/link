//! What goes into a stream: the files a move carries, and a harness's
//! state or a connection's paths added as tar entries, never through a link.

use super::*;
use link_format::WithinExt;

/// The folders of Home named as one of the agent's own (`skills`) that a
/// move leaves out: once it's laid out, its own folder's are carried in
/// their place.
pub(super) fn left_out(folder: &link_agent_contract::folder::Folder) -> Vec<String> {
    link_agent_contract::folder::SEEN
        .iter()
        .map(|c| c.directory())
        .filter(|name| link_agent_contract::folder::is_folder(&folder.home.join(name)))
        .map(String::from)
        .collect()
}

/// The files of `folder` as a stream names them (`files/...`): Home's,
/// and its instructions, skills and conversations through their stores.
pub(super) fn seen_files(
    folder: &link_agent_contract::folder::Folder,
) -> anyhow::Result<std::collections::BTreeSet<String>> {
    let own = |path: &String| {
        link_agent_contract::folder::seen_collection(path.strip_prefix("files/").unwrap_or(path))
            .is_some()
    };
    let found: std::collections::BTreeSet<String> = files_in(&folder.home)?
        .into_iter()
        .filter(|p| !own(p))
        .collect();
    Ok(found)
}

/// The paths of the files in the files folder `files`, as the stream
/// names them (`files/...`), walked without following a link.
fn files_in(files: &Path) -> anyhow::Result<std::collections::BTreeSet<String>> {
    let mut found = std::collections::BTreeSet::new();
    let Ok(folder) = Dir::open(files) else {
        return Ok(found);
    };
    list_in(&folder, Path::new("files"), &mut found)?;
    Ok(found)
}

fn list_in(
    folder: &Dir,
    at: &Path,
    found: &mut std::collections::BTreeSet<String>,
) -> anyhow::Result<()> {
    for name in folder.names()? {
        let path = at.join(&name);
        match folder.kind(&name) {
            Ok(Kind::File) => {
                found.insert(path.to_string_lossy().into_owned());
            }
            Ok(Kind::Folder) => {
                if let Ok(inner) = folder.folder(&name) {
                    list_in(&inner, &path, found)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Writes the harness's `state` in its home `dir` to `out` as a tar,
/// paths as they are in the home (`.<name>/...`): what `lnk agent
/// backup` keeps of it. No settings and no keys: those the move's
/// manifest carries, and `state` leaves out the `.env` they're written to.
pub fn archive_state(out: impl Write, dir: &Path, state: &State) -> anyhow::Result<()> {
    let mut tar = tar::Builder::new(out);
    tar.follow_symlinks(false);
    add_state(&mut tar, dir, state, Path::new(""), &Out::none())?;
    tar.into_inner()?.flush()?;
    Ok(())
}

/// What `state` names in the folder `dir`, under `prefix`, but not what it
/// excludes; what isn't there is skipped. The harness writes `dir`, and
/// may still run (a process that outlived `lnk agent stop`): every name is
/// opened from the folder holding it without following a link, so a link
/// swapped in after [`in_home`]'s check is refused on the way, or carried
/// as a link, never read through.
pub(super) fn add_state<W: Write>(
    tar: &mut tar::Builder<W>,
    dir: &Path,
    state: &State,
    prefix: &Path,
    ctx: &Out,
) -> anyhow::Result<()> {
    let excluded = in_home(dir, &state.exclude)?;
    let includes = in_home(dir, &state.include)?;
    add_paths(tar, dir, &includes, prefix, &excluded, ctx)
}

/// `includes`, paths in `dir`, under `prefix`, but nothing in `excluded`;
/// a link on the way to one, whenever it came, is refused.
pub(super) fn add_paths<W: Write>(
    tar: &mut tar::Builder<W>,
    dir: &Path,
    includes: &[PathBuf],
    prefix: &Path,
    excluded: &[PathBuf],
    ctx: &Out,
) -> anyhow::Result<()> {
    if includes.is_empty() {
        return Ok(());
    }
    let home = match Dir::open(dir) {
        Ok(home) => home,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).within(format!("opening {}", dir.display())),
    };
    for include in includes {
        if include == dir {
            add_folder(tar, home.try_clone()?, dir, prefix, excluded, ctx)?;
            continue;
        }
        let Some((parent, last)) = holding(&home, dir, include)? else {
            continue;
        };
        let entry = prefix.join(include.strip_prefix(dir)?);
        add(tar, &parent, last, include, &entry, excluded, ctx)?;
    }
    Ok(())
}

/// The folder holding `path`, a path in the folder `dir` open as `home`,
/// and its name there: each folder on the way opened from the one before
/// without following a link, so a link on the way, whenever it came, is
/// refused. None if one is missing or isn't a folder: nothing of `path` is
/// there.
pub(super) fn holding<'a>(
    home: &Dir,
    dir: &Path,
    path: &'a Path,
) -> anyhow::Result<Option<(Dir, &'a OsStr)>> {
    let names: Vec<&OsStr> = path.strip_prefix(dir)?.iter().collect();
    let Some((last, folders)) = names.split_last() else {
        bail!("{} is the folder itself", path.display());
    };
    let mut at = home.try_clone()?;
    let mut walked = dir.to_path_buf();
    for name in folders {
        walked.push(name);
        at = match at.folder(name) {
            Ok(next) => next,
            Err(_) if at.kind(name).is_ok_and(|k| k == Kind::Link) => bail!(
                "{} is a link, on the way to {}: not following it (remove it to go on)",
                walked.display(),
                path.display()
            ),
            Err(_) => return Ok(None),
        };
    }
    Ok(Some((at, last)))
}

/// `paths` (`~/...`) in `dir`, the harness's home or a connection's folder;
/// none may leave it.
pub(super) fn in_box(dir: &Path, paths: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    paths
        .iter()
        .map(|path| {
            let rel = path
                .strip_prefix("~/")
                .within(format!("{path:?} isn't in its folder (~/...)"))?;
            let rel = Path::new(rel);
            ensure!(
                rel.components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
                "{path:?} isn't a plain path in its folder"
            );
            Ok(dir.join(rel))
        })
        .collect()
}

/// [`in_box`], on the disk: the paths in `dir`, refused if a folder on the
/// way to one is a link. The harness writes its home, so a link there
/// (`~/.signal-cli` to another agent's home) would have `lnk`, outside
/// the sandbox, read, remove or write through it.
pub(super) fn in_home(dir: &Path, paths: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    let paths = in_box(dir, paths)?;
    for path in &paths {
        if let Some(link) = crosses_link(dir, path) {
            bail!(
                "{} is a link, on the way to {}: not following it (remove it to go on)",
                link.display(),
                path.display()
            );
        }
    }
    Ok(paths)
}

/// The first folder between `dir` and `path` (neither included) that is
/// a link, if any.
fn crosses_link(dir: &Path, path: &Path) -> Option<PathBuf> {
    let rel = path.strip_prefix(dir).ok()?;
    let mut at = dir.to_path_buf();
    for part in rel.parent()?.components() {
        at.push(part);
        if at
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Some(at);
        }
    }
    None
}

/// `name` in the folder `parent` (at `path`) and what's under it, as
/// `entry` in the stream, but nothing in `excluded`; links as links, and
/// nothing that isn't a file, folder or link. Opened from `parent`
/// without following a link, and read from what was opened: a file
/// swapped for a link after it was listed goes as that link.
pub(super) fn add<W: Write>(
    tar: &mut tar::Builder<W>,
    parent: &Dir,
    name: &OsStr,
    path: &Path,
    entry: &Path,
    excluded: &[PathBuf],
    ctx: &Out,
) -> anyhow::Result<()> {
    if excluded.iter().any(|e| e == path) {
        return Ok(());
    }
    let reading = || format!("reading {}", path.display());
    let stat = match parent.stat(name) {
        Ok(stat) => stat,
        // Gone since it was listed.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).within(reading()),
    };
    let file = match stat.kind {
        Kind::Link => return add_link(tar, parent, name, path, entry),
        Kind::Other => return Ok(()),
        Kind::File | Kind::Folder => match parent.open_entry(name) {
            Ok(file) => file,
            // A link since: as a link.
            Err(err) if nofollow::is_link(&err) => {
                return add_link(tar, parent, name, path, entry);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err).within(reading()),
        },
    };
    // What it is now, from the handle: not what the listing said.
    let meta = file.metadata().within(reading())?;
    if meta.is_dir() {
        add_folder(tar, Dir::from(file), path, entry, excluded, ctx)
    } else if meta.is_file() {
        let mut header = tar::Header::new_gnu();
        header.set_metadata_in_mode(&meta, tar::HeaderMode::Complete);
        // Exactly the size in its header, were it to grow or shrink while
        // read; with its hash, unless the other end holds it the same.
        stream::append_file(
            tar,
            &mut header,
            entry,
            file,
            meta.len(),
            ctx.have,
            &ctx.spool,
        )
        .map(|_| ())
        .within(reading())
    } else {
        Ok(())
    }
}

/// The open folder `folder` (at `path`) as `entry`, then what's in it.
pub(super) fn add_folder<W: Write>(
    tar: &mut tar::Builder<W>,
    folder: Dir,
    path: &Path,
    entry: &Path,
    excluded: &[PathBuf],
    ctx: &Out,
) -> anyhow::Result<()> {
    let reading = || format!("reading {}", path.display());
    let meta = folder.metadata().within(reading())?;
    let mut header = tar::Header::new_gnu();
    header.set_metadata_in_mode(&meta, tar::HeaderMode::Complete);
    tar.append_data(&mut header, entry, std::io::empty())
        .within(reading())?;
    for child in folder.names().within(reading())? {
        add(
            tar,
            &folder,
            &child,
            &path.join(&child),
            &entry.join(&child),
            excluded,
            ctx,
        )?;
    }
    Ok(())
}

/// The link `name` in `parent` as `entry`: where it points, never what.
fn add_link<W: Write>(
    tar: &mut tar::Builder<W>,
    parent: &Dir,
    name: &OsStr,
    path: &Path,
    entry: &Path,
) -> anyhow::Result<()> {
    let reading = || format!("reading {}", path.display());
    let (stat, target) = match parent.stat(name).and_then(|stat| {
        let target = parent.read_link(name)?;
        Ok((stat, target))
    }) {
        Ok(link) => link,
        // Gone, or no longer a link, since: skipped.
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                || err.raw_os_error() == Some(libc::EINVAL) =>
        {
            return Ok(());
        }
        Err(err) => return Err(err).within(reading()),
    };
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(stat.mode);
    header.set_uid(stat.uid);
    header.set_gid(stat.gid);
    header.set_mtime(stat.mtime);
    tar.append_link(&mut header, entry, target)
        .within(reading())
}

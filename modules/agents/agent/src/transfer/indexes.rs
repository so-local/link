//! Memory's index in a move: copied as it's sent, SQLite's file with
//! SQLite's online backup and the parts beside it as files, and put in its
//! engine's folder when it arrives.
//!
//! An engine keeps its index as `index.sqlite` and its parts beside it,
//! each named after it (`index.sqlite.<part>`, a file or a folder of
//! files), such as Link Memory's words and vectors. Its parts are copied
//! first and its SQLite file last, each consistent alone, and the engine
//! sets right a part that differs from the file it came with.

use super::*;
use link_format::WithinExt;

/// The name of memory's index in its engine's folder.
pub const INDEX: &str = "index.sqlite";

/// How many times a part that's a folder is copied while files in it are
/// replaced as it's read (an engine merging what it keeps there).
const FOLDER_TRIES: usize = 8;

/// Memory's index as a move carries it: each entry's path in the stream
/// and its copy, in a folder of the spool removed with it.
pub(super) struct Carried {
    folder: PathBuf,
    pub(super) entries: Vec<(String, PathBuf)>,
}

impl Drop for Carried {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// Memory's index, when the agent has one: copied into the spool now,
/// never the live files, as [`index_from`] copies it.
pub(super) fn index_of(settings: &Settings) -> anyhow::Result<Option<Carried>> {
    let Some(engine) = &settings.memory else {
        return Ok(None);
    };
    check_engine(engine)?;
    index_from(&settings.memory_state(engine), engine, &spool())
}

/// The index of `engine` in its folder `live`, copied into a folder of
/// `spool`: each of its parts first (`memory/<engine>/index.sqlite.<part>`,
/// a folder's files each an entry), then its SQLite file
/// (`memory/<engine>/index.sqlite`) with SQLite's online backup,
/// consistent while memory writes, never the live file and its `-wal`.
/// None when there's no index.
pub(super) fn index_from(
    live: &Path,
    engine: &str,
    spool: &Path,
) -> anyhow::Result<Option<Carried>> {
    let file = live.join(INDEX);
    if !file.symlink_metadata().is_ok_and(|m| m.is_file()) {
        return Ok(None);
    }
    link_files::create_private_dir(spool)?;
    let carried = Carried {
        folder: spool.join(format!("index-{}", link_files::random_token()?)),
        entries: Vec::new(),
    };
    link_files::create_private_dir(&carried.folder)?;
    let mut carried = carried;
    let prefix = format!("{INDEX}.");
    let mut parts: Vec<String> = std::fs::read_dir(live)?
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| name.starts_with(&prefix) && name.len() > prefix.len())
        .collect();
    parts.sort();
    for part in parts {
        let from = live.join(&part);
        let to = carried.folder.join(&part);
        let Ok(meta) = from.symlink_metadata() else {
            continue;
        };
        let copied = if meta.is_dir() {
            copy_folder(&from, &to)
        } else if meta.is_file() {
            std::fs::copy(&from, &to).map(|_| vec![String::new()])
        } else {
            continue;
        };
        match copied {
            Ok(files) => carried.entries.extend(files.into_iter().map(|name| {
                let (entry, copy) = match name.is_empty() {
                    true => (part.clone(), to.clone()),
                    false => (format!("{part}/{name}"), to.join(&name)),
                };
                (format!("memory/{engine}/{entry}"), copy)
            })),
            // Left out: the engine makes it again.
            Err(err) => link_format::aside(link_format::Report::warning(link_format::sentence!(
                "Memory's {} isn't carried: {}",
                link_format::Foreign::new(&part),
                link_format::Foreign::new(&err.to_string())
            ))),
        }
    }
    let copy = carried.folder.join(INDEX);
    backup_index(&file, &copy).within(format!("copying {}", file.display()))?;
    carried
        .entries
        .push((format!("memory/{engine}/{INDEX}"), copy));
    Ok(Some(carried))
}

/// The files of the folder `from` copied into `to`, by their names: begun
/// again when one goes as it's copied, as an engine merging its files
/// replaces them.
fn copy_folder(from: &Path, to: &Path) -> std::io::Result<Vec<String>> {
    let mut tries = 0;
    loop {
        tries += 1;
        let _ = std::fs::remove_dir_all(to);
        std::fs::create_dir_all(to)?;
        let copied = (|| -> std::io::Result<Vec<String>> {
            let mut names = Vec::new();
            for entry in std::fs::read_dir(from)? {
                let entry = entry?;
                let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                // Plain files alone: a lock or a list of the engine's own
                // starts with a dot.
                if name.starts_with('.') || !entry.file_type()?.is_file() {
                    continue;
                }
                std::fs::copy(entry.path(), to.join(&name))?;
                names.push(name);
            }
            names.sort();
            Ok(names)
        })();
        match copied {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && tries < FOLDER_TRIES => {}
            done => return done,
        }
    }
}

/// `live`, an SQLite file another process may be writing, copied to
/// `to` with SQLite's online backup; tried again for a while when it's
/// busy.
fn backup_index(live: &Path, to: &Path) -> anyhow::Result<()> {
    crate::databases::backup(live, to)
}

/// The copy of memory's index at `copy`, as `entry` in the stream.
pub(super) fn add_index<W: Write>(
    tar: &mut tar::Builder<W>,
    entry: &str,
    copy: &Path,
    ctx: &Out,
) -> anyhow::Result<()> {
    let file = std::fs::File::open(copy)?;
    let len = file.metadata()?.len();
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o600);
    header.set_mtime(crate::moved::now());
    header.set_entry_type(tar::EntryType::Regular);
    stream::append_file(
        tar,
        &mut header,
        Path::new(entry),
        file,
        len,
        ctx.have,
        &ctx.spool,
    )?;
    Ok(())
}

/// An error unless `engine` can be a memory engine's name, a folder of
/// its own: lowercase letters, digits and dashes.
fn check_engine(engine: &str) -> anyhow::Result<()> {
    ensure!(
        !engine.is_empty()
            && engine.len() <= 40
            && engine
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "{:?} isn't a memory engine's name",
        link_format::clean(engine)
    );
    Ok(())
}

/// Memory's index of each engine that came (`memory/<engine>/index.sqlite`
/// in `from`, staging, and its parts beside it) put in its engine's folder
/// (`place` of the engine), in place of the one there, its journal and
/// every part it had: memory reads it as it is, and doesn't make it
/// again.
pub(super) fn put_index(from: &Path, place: impl Fn(&str) -> PathBuf) -> anyhow::Result<()> {
    let Ok(engines) = std::fs::read_dir(from) else {
        return Ok(());
    };
    let prefix = format!("{INDEX}.");
    let parts_in = |dir: &Path| -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|e| e.file_name().to_str().map(str::to_string))
                    .filter(|name| name.starts_with(&prefix) && name.len() > prefix.len())
                    .collect()
            })
            .unwrap_or_default()
    };
    for engine in engines.flatten() {
        let Some(name) = engine.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let index = engine.path().join(INDEX);
        let real = engine.file_type().is_ok_and(|t| t.is_dir())
            && index.symlink_metadata().is_ok_and(|m| m.is_file());
        if check_engine(&name).is_err() || !real {
            continue;
        }
        let to = place(&name);
        link_files::create_private_dir(&to)?;
        for old in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(to.join(format!("{INDEX}{old}")));
        }
        // None of the parts there goes with the index that came.
        for part in parts_in(&to) {
            let old = to.join(part);
            let _ = std::fs::remove_dir_all(&old).or_else(|_| std::fs::remove_file(&old));
        }
        for part in parts_in(&engine.path()) {
            let from = engine.path().join(&part);
            let real = from
                .symlink_metadata()
                .is_ok_and(|m| m.is_file() || m.is_dir());
            if real {
                std::fs::rename(&from, to.join(&part))
                    .within(format!("putting memory's {part} in {}", to.display()))?;
            }
        }
        std::fs::rename(&index, to.join(INDEX))
            .within(format!("putting memory's index in {}", to.display()))?;
    }
    Ok(())
}

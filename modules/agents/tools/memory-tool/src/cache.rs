//! The index as a cache of a copy kept elsewhere: with the agent's
//! `memory` collection where its machine keeps it, through the storage
//! bridge ([`link_store_contract::store::memory_files`]), or beside Home where
//! Home is kept in a bucket, through the files bridge. The index here is a
//! cache, and the copy is the index's objects, kept as
//! `link_store_contract::files::index` keeps one: one stream cut into
//! segments named by their SHA-256, and a manifest naming them, written
//! last and only if it's still the version this cache was read from. A
//! writer refused there was behind another, and drops its cache to fetch
//! the copy again ([`link_store_contract::files::is_conflict`]); what the
//! cache held that the copy doesn't is made again from Home and the
//! conversations.
//!
//! The stream is a tar archive of the index's parts ([`Part`]): its words'
//! files first, oldest first, then its vectors, then its SQLite file, so a
//! write back after a small change shares its first segments with the copy
//! and sends only those after.

use link_format::sentence;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use link_store_contract::files::{Store, Version, index};
use rusqlite::Connection;

pub use index::{MANIFEST, Manifest, SEGMENT};

/// The index's engine, as its objects are named.
pub const ENGINE: &str = "link-memory";

/// A part of the index, as the copy's archive names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    /// The SQLite file: documents, passages, the journal and the queue.
    Index,
    /// A file of the words' folder, Tantivy's (`words/<name>`).
    Words,
    /// The vectors' binary codes.
    Codes,
    /// The small vectors.
    Small,
}

impl Part {
    fn name(self) -> &'static str {
        match self {
            Part::Index => "index.sqlite",
            Part::Words => "words",
            Part::Codes => "codes",
            Part::Small => "small",
        }
    }

    /// The part an archive's entry is, and for a word's file, its name;
    /// none for anything else, which the copy never holds.
    fn of(entry: &str) -> Option<(Part, Option<&str>)> {
        for part in [Part::Index, Part::Codes, Part::Small] {
            if entry == part.name() {
                return Some((part, None));
            }
        }
        let name = entry.strip_prefix("words/")?;
        let plain = !name.is_empty()
            && !name.starts_with('.')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_');
        plain.then_some((Part::Words, Some(name)))
    }
}

/// Where each part of the index `file` is kept on this machine.
fn place(file: &Path, part: Part) -> PathBuf {
    let [codes, small] = crate::vectors::files_of(file);
    match part {
        Part::Index => file.to_path_buf(),
        Part::Words => crate::index::words::folder_of(file),
        Part::Codes => codes,
        Part::Small => small,
    }
}

/// Removes the parts of the index `file` beside it: its words and its
/// vectors. Its SQLite file and log are the caller's.
pub fn remove_parts(file: &Path) {
    let _ = std::fs::remove_dir_all(place(file, Part::Words));
    for part in [Part::Codes, Part::Small] {
        let _ = std::fs::remove_file(place(file, part));
    }
}

/// The copy's manifest and its version; none when there's no copy.
pub fn manifest(files: &dyn Store) -> Result<Option<(Manifest, Version)>> {
    index::manifest(files, ENGINE)
}

/// The copy's version, its manifest read whole whatever it holds; none
/// when there's no copy. A call through the bridge as the agent, so it
/// also proves the agent's key is taken.
pub fn version(files: &dyn Store) -> Result<Option<Version>> {
    Ok(files
        .get(&format!(
            "{}{ENGINE}/{MANIFEST}",
            link_store_contract::files::INDEX
        ))
        .context("reading the index's copy")?
        .map(|(_, version)| version))
}

/// Fetches the copy into the index `file` and the parts beside it, each
/// segment checked against its hash as it's streamed into a folder aside,
/// put in place once it's checked ([`crate::check_copy`]); the manifest's
/// version, or none when there's no copy (and no file is made). A copy
/// that can't be had whole (a segment gone or changed), or that holds
/// more than this index makes, isn't used: no file is made, and the
/// version is still the manifest's, so the index made again from Home is
/// written back over it. Words that aren't whole are left out, made again
/// from the files ([`crate::index::words::Words::reconcile`]), and vectors
/// that aren't are embedded again.
pub fn fetch(files: &dyn Store, file: &Path) -> Result<Option<Version>> {
    if let Some(dir) = file.parent() {
        link_files::create_private_dir(dir)?;
    }
    let aside = file.with_extension("fetching");
    // Streamed a segment at a time, never held whole; read again once: a
    // segment gone may be another writer's, whose manifest replaced the
    // one read.
    let fetch = || -> Result<Option<Version>> {
        let _ = std::fs::remove_dir_all(&aside);
        link_files::create_private_dir(&aside)?;
        let Some((reader, version)) = index::open(files, ENGINE)? else {
            return Ok(None);
        };
        unpack(reader, &aside)?;
        Ok(Some(version))
    };
    let version = match fetch().or_else(|_| fetch()) {
        Ok(Some(version)) => version,
        Ok(None) => {
            let _ = std::fs::remove_dir_all(&aside);
            return Ok(None);
        }
        Err(err) => {
            // The bridge answers, but the copy isn't whole: made again.
            let _ = std::fs::remove_dir_all(&aside);
            let version = self::version(files)?;
            crate::warn(sentence!("The index's copy isn't used."), &err);
            return Ok(version);
        }
    };
    if let Err(err) = crate::check_copy(&aside.join(Part::Index.name())) {
        crate::warn(sentence!("The index's copy isn't used."), &err);
        let _ = std::fs::remove_dir_all(&aside);
        return Ok(Some(version));
    }
    let words = aside.join(Part::Words.name());
    if words.exists()
        && let Err(err) = crate::index::words::check_copy(&words)
    {
        crate::warn(sentence!("The index's words aren't used."), &err);
        let _ = std::fs::remove_dir_all(&words);
    }
    remove_parts(file);
    for part in [Part::Words, Part::Codes, Part::Small, Part::Index] {
        let from = aside.join(part.name());
        if from.exists() {
            std::fs::rename(&from, place(file, part))
                .with_context(|| format!("putting {} in place", from.display()))?;
        }
    }
    let _ = std::fs::remove_dir_all(&aside);
    Ok(Some(version))
}

/// Unpacks the copy's archive, `reader`, into the folder `to`: each part
/// in its own file, the words' in `words/`. An entry no part names, or one
/// that isn't a plain file, refuses the copy.
fn unpack(reader: impl Read, to: &Path) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    let mut indexed = false;
    for entry in archive.entries().context("reading the index's copy")? {
        let mut entry = entry.context("reading the index's copy")?;
        let name = entry.path()?.to_string_lossy().into_owned();
        let Some((part, word)) = Part::of(&name) else {
            bail!(
                "the index's copy holds {}",
                link_format::clean(&format!("{name:?}"))
            );
        };
        ensure!(
            entry.header().entry_type().is_file(),
            "the index's copy holds {} as no file",
            part.name()
        );
        let target = match word {
            Some(word) => {
                let words = to.join(Part::Words.name());
                link_files::create_private_dir(&words)?;
                words.join(word)
            }
            None => to.join(part.name()),
        };
        indexed |= part == Part::Index;
        let mut out = std::io::BufWriter::new(private_file(&target)?);
        std::io::copy(&mut entry, &mut out)?;
        out.flush()?;
    }
    ensure!(indexed, "the index's copy holds no index");
    Ok(())
}

/// Writes the index back as the copy, from a consistent snapshot: its
/// words linked into `scratch`, a folder, and its SQLite file `db` copied
/// there with SQLite's online backup, while no batch commits; then its
/// vectors, read from beside `file` as they are. Written if the copy is
/// still `read`'s version (none: there was no copy) and the parts are no
/// larger than `most` together ([`crate::opinions::Opinions::kept_off`]).
/// Streamed as one archive, a segment at a time
/// ([`index::Writer`]), never read whole into memory, and only the
/// segments the copy lacks are sent. The manifest's new version;
/// [`Written::TooLarge`] past `most`; a
/// [`link_store_contract::files::Conflict`] when another wrote it
/// meanwhile.
pub(crate) fn write_back(
    db: &Connection,
    words: &crate::index::words::Words,
    file: &Path,
    files: &dyn Store,
    read: Option<&Version>,
    scratch: &Path,
    most: u64,
) -> Result<Written> {
    let _ = std::fs::remove_dir_all(scratch);
    link_files::create_private_dir(scratch)?;
    let written = (|| -> Result<Written> {
        let sqlite = scratch.join(Part::Index.name());
        let folder = scratch.join(Part::Words.name());
        words.snapshot(&folder, || {
            let mut copy = crate::open_file(&sqlite)?;
            let backup = rusqlite::backup::Backup::new(db, &mut copy)?;
            backup.run_to_completion(256, std::time::Duration::from_millis(0), None)?;
            Ok(())
        })?;
        // Each entry: its name, where it's read from, and its length now.
        let mut entries: Vec<(String, PathBuf, u64)> = Vec::new();
        let mut held: Vec<(std::time::SystemTime, String, PathBuf, u64)> = Vec::new();
        for entry in std::fs::read_dir(&folder)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if meta.is_file() && Part::of(&format!("words/{name}")).is_some() {
                let changed = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                held.push((changed, name, entry.path(), meta.len()));
            }
        }
        // The oldest first: what didn't change stays where it was.
        held.sort();
        for (_, name, path, length) in held {
            entries.push((format!("words/{name}"), path, length));
        }
        for part in [Part::Codes, Part::Small] {
            let path = place(file, part);
            if let Ok(meta) = std::fs::metadata(&path) {
                entries.push((part.name().into(), path, meta.len()));
            }
        }
        let length = std::fs::metadata(&sqlite)?.len();
        entries.push((Part::Index.name().into(), sqlite, length));
        let size: u64 = entries.iter().map(|(_, _, length)| length).sum();
        if size > most {
            return Ok(Written::TooLarge { size });
        }
        let mut copy = index::Writer::new(files, ENGINE, read)?;
        {
            let mut tar = tar::Builder::new(&mut copy);
            for (name, path, length) in &entries {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Regular);
                header.set_mode(0o600);
                header.set_mtime(0);
                header.set_size(*length);
                header.set_cksum();
                // Exactly as long as it was: a vector file cut meanwhile
                // reads as zeros past its end, embedded again where it's
                // used.
                let input = std::fs::File::open(path)
                    .with_context(|| format!("reading {}", path.display()))?
                    .chain(std::io::repeat(0))
                    .take(*length);
                tar.append_data(&mut header, name, input)?;
            }
            tar.finish()?;
        }
        Ok(Written::Copy(copy.finish()?))
    })();
    let _ = std::fs::remove_dir_all(scratch);
    written
}

/// What writing an index back did.
#[derive(Debug, PartialEq)]
pub enum Written {
    /// The copy is this version now.
    Copy(Version),
    /// Nothing: the index, `size` bytes with its parts, is past the most
    /// kept off the machine.
    TooLarge { size: u64 },
}

impl Written {
    /// The copy's version, when it was written.
    pub fn copy(self) -> Option<Version> {
        match self {
            Written::Copy(version) => Some(version),
            Written::TooLarge { .. } => None,
        }
    }
}

/// A file made new for this user alone, or emptied if it's there.
fn private_file(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("making {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_holds_only_the_parts_it_names() {
        assert_eq!(Part::of("index.sqlite"), Some((Part::Index, None)));
        assert_eq!(Part::of("small"), Some((Part::Small, None)));
        assert_eq!(
            Part::of("words/0a1b.idx"),
            Some((Part::Words, Some("0a1b.idx")))
        );
        for odd in [
            "words/../x",
            "words/a/b",
            "words/.lock",
            "words/",
            "x",
            "/index.sqlite",
        ] {
            assert_eq!(Part::of(odd), None, "{odd}");
        }
    }
}

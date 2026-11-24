//! Persistent revision reservations, independent of a file's contents.
use super::super::under::{Dir, read_open};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::MetadataExt;

pub(super) const FOLDER: &str = "revisions";
const COUNTER: &str = "next";
const RECORD_MAX: u64 = 4096;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Counter {
    reserved: u64,
}
link_files::specification!(Counter, include_str!("revisions/counter.schema.toml"));

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    device: u64,
    inode: u64,
    size: u64,
    changed: i64,
    changed_ns: i64,
    modified: i64,
    modified_ns: i64,
}
impl From<&std::fs::Metadata> for Identity {
    fn from(meta: &std::fs::Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            changed: meta.ctime(),
            changed_ns: meta.ctime_nsec(),
            modified: meta.mtime(),
            modified_ns: meta.mtime_nsec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fingerprint {
    identity: Identity,
    digest: [u8; 32],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u64,
    fingerprint: Fingerprint,
}

link_files::specification!(Record, include_str!("revisions/record.schema.toml"));

pub(super) struct Image {
    pub bytes: Vec<u8>,
    fingerprint: Fingerprint,
}

/// Read bytes and identity from the same no-follow handle. A person may
/// edit without our lock; refuse a file changing during this read.
pub(super) fn read(dir: &Dir, name: &str, max: u64) -> anyhow::Result<Option<Image>> {
    let file = match dir.open(name, libc::O_RDONLY, 0) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let identity = Identity::from(&file.metadata()?);
    let bytes = read_open(file.try_clone()?, max)?;
    ensure!(
        identity == Identity::from(&file.metadata()?),
        "the file changed while its revision was read"
    );
    let digest = link_digest::sha256_bytes(&bytes);
    Ok(Some(Image {
        bytes,
        fingerprint: Fingerprint { identity, digest },
    }))
}

fn last(revisions: &Dir) -> anyhow::Result<Option<u64>> {
    revisions
        .read(COUNTER, RECORD_MAX)?
        .map(|bytes| {
            link_files::saved::parse_json::<Counter>(&bytes)
                .map(|counter| counter.reserved)
                .context("reading the file revision counter")
        })
        .transpose()
}

/// Reserve durably before mutating bytes. A crash may spend a revision,
/// but cannot roll the counter back behind an acknowledged data change.
pub(super) fn reserve(revisions: &Dir) -> anyhow::Result<u64> {
    let last = match last(revisions)? {
        Some(last) => last,
        None => {
            ensure!(
                revisions
                    .names()?
                    .iter()
                    .all(|name| link_files::layout::is_temporary(name)),
                "the file revision counter is missing from existing revision state"
            );
            0
        }
    };
    let next = last
        .checked_add(1)
        .context("file revisions are exhausted")?;
    revisions.write_synced(
        COUNTER,
        &link_files::saved::print_json(&Counter { reserved: next })?,
    )?;
    Ok(next)
}

fn place(revisions: &Dir, path: &str, make: bool) -> anyhow::Result<Option<(Dir, String)>> {
    let (folder, name) = path.rsplit_once('/').context("a file has a collection")?;
    Ok(revisions
        .walk(&format!("files/{folder}"), make)?
        .map(|dir| (dir, name.to_owned())))
}

fn save(revisions: &Dir, path: &str, record: &Record) -> anyhow::Result<()> {
    let (dir, name) = place(revisions, path, true)?.context("revision record folder")?;
    dir.write_synced(&name, &link_files::saved::print_json(record)?)?;
    Ok(())
}

/// Observe external edits as fresh revisions too. No file is version zero.
pub(super) fn current(revisions: &Dir, path: &str, image: Option<&Image>) -> anyhow::Result<u64> {
    let record: Option<Record> = match place(revisions, path, false)? {
        Some((dir, name)) => dir
            .read(&name, RECORD_MAX)?
            .map(|bytes| link_files::saved::parse_json(&bytes))
            .transpose()
            .context("reading a file revision record")?,
        None => None,
    };
    if let Some(record) = record {
        let last = last(revisions)?.context("the file revision counter is missing")?;
        ensure!(
            record.version != 0 && record.version <= last,
            "invalid file revision record"
        );
        if image.is_some_and(|image| record.fingerprint == image.fingerprint) {
            return Ok(record.version);
        }
    }
    let Some(image) = image else { return Ok(0) };
    let version = reserve(revisions)?;
    save(
        revisions,
        path,
        &Record {
            version,
            fingerprint: Fingerprint {
                identity: image.fingerprint.identity.clone(),
                digest: image.fingerprint.digest,
            },
        },
    )?;
    Ok(version)
}

/// Record the newly written inode against the already reserved version.
pub(super) fn remember(
    revisions: &Dir,
    path: &str,
    dir: &Dir,
    name: &str,
    version: u64,
) -> anyhow::Result<()> {
    let image =
        read(dir, name, super::super::file_max(path))?.context("the written file disappeared")?;
    save(
        revisions,
        path,
        &Record {
            version,
            fingerprint: image.fingerprint,
        },
    )
}

//! Memory's index beside Home: an index file kept as segments of
//! [`SEGMENT`] bytes, each named by its SHA-256
//! (`index/<engine>/seg-<sha256>`), and a manifest naming them in order
//! (`index/<engine>/manifest.json`), written last and only if it's still
//! the version read, so one writer behind another is refused and never
//! overwrites it unseen. A write sends only the segments the copy lacks.

use super::{Expect, Store, Version, sha256};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

/// A segment's size: what a write back sends for a change.
pub const SEGMENT: usize = 8 << 20;

/// The manifest's name.
pub const MANIFEST: &str = "manifest.json";

/// The largest index fetched: a copy naming more is refused.
pub const COPY_MAX: u64 = 8 << 30;

/// The copy's manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// The whole file's size.
    pub size: u64,
    /// Each segment's SHA-256, in order.
    pub segments: Vec<String>,
}

fn name(engine: &str, file: &str) -> String {
    format!("{}{engine}/{file}", super::INDEX)
}

fn segment(engine: &str, hash: &str) -> String {
    name(engine, &format!("seg-{hash}"))
}

/// The copy's manifest and its version; none when there's no copy.
pub fn manifest(store: &dyn Store, engine: &str) -> anyhow::Result<Option<(Manifest, Version)>> {
    let Some((bytes, version)) = store.get(&name(engine, MANIFEST))? else {
        return Ok(None);
    };
    let manifest: Manifest =
        serde_json::from_slice(&bytes).context("reading the index's manifest")?;
    ensure!(
        manifest.format == 1,
        "an index's manifest of another format"
    );
    ensure!(
        manifest.size <= COPY_MAX
            && manifest.segments.len() as u64 == manifest.size.div_ceil(SEGMENT as u64)
            && manifest
                .segments
                .iter()
                .all(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())),
        "the index's manifest doesn't hold together"
    );
    Ok(Some((manifest, version)))
}

/// The copy, whole, each segment checked against its hash, and the
/// manifest's version; none when there's no copy.
pub fn read(store: &dyn Store, engine: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
    let Some((manifest, version)) = manifest(store, engine)? else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(manifest.size as usize);
    for hash in &manifest.segments {
        let (bytes, _) = store
            .get(&segment(engine, hash))?
            .with_context(|| format!("the index's segment {hash} is missing"))?;
        ensure!(
            sha256(&bytes) == *hash,
            "the index's segment {hash} isn't what its manifest says"
        );
        out.extend_from_slice(&bytes);
    }
    ensure!(
        out.len() as u64 == manifest.size,
        "the index's segments don't add up to its size"
    );
    Ok(Some((out, version)))
}

/// `bytes`, an index file whole, written as the copy: the segments the
/// copy lacks, then the manifest if it's still `read`'s version (none:
/// there was no copy). The manifest's new version; a
/// [`super::Conflict`] when another wrote it meanwhile. Segments no
/// manifest names after it are removed.
pub fn write(
    store: &dyn Store,
    engine: &str,
    bytes: &[u8],
    read: Option<&Version>,
) -> anyhow::Result<Version> {
    let prefix = name(engine, "seg-");
    let held: std::collections::BTreeSet<String> = store
        .list(&prefix)?
        .into_iter()
        .filter_map(|o| o.path.strip_prefix(&prefix).map(str::to_string))
        .collect();
    let mut segments = Vec::new();
    for part in bytes.chunks(SEGMENT) {
        let hash = sha256(part);
        if !held.contains(&hash) {
            // Named by what it holds: whoever writes it writes the same.
            store.put(&segment(engine, &hash), part, &Expect::Any)?;
        }
        segments.push(hash);
    }
    let manifest = Manifest {
        format: 1,
        size: bytes.len() as u64,
        segments,
    };
    let expect = match read {
        Some(version) => Expect::Version(version.clone()),
        None => Expect::Absent,
    };
    let version = store.put(
        &name(engine, MANIFEST),
        &serde_json::to_vec(&manifest)?,
        &expect,
    )?;
    let named: std::collections::BTreeSet<&String> = manifest.segments.iter().collect();
    for hash in held.iter().filter(|h| !named.contains(h)) {
        let _ = store.delete(&segment(engine, hash), &Expect::Any);
    }
    Ok(version)
}

/// The copy, read as a stream ([`std::io::Read`]) a segment at a time, each
/// checked against its hash as it's fetched, and the manifest's version;
/// none when there's no copy. Its end is an error unless the segments add
/// up to the manifest's size.
pub fn open<'a>(
    store: &'a dyn Store,
    engine: &str,
) -> anyhow::Result<Option<(Reader<'a>, Version)>> {
    let Some((manifest, version)) = manifest(store, engine)? else {
        return Ok(None);
    };
    let reader = Reader {
        store,
        engine: engine.to_string(),
        size: manifest.size,
        segments: manifest.segments.into_iter(),
        current: std::io::Cursor::new(Vec::new()),
        length: 0,
    };
    Ok(Some((reader, version)))
}

/// A copy being read ([`open`]).
pub struct Reader<'a> {
    store: &'a dyn Store,
    engine: String,
    size: u64,
    segments: std::vec::IntoIter<String>,
    current: std::io::Cursor<Vec<u8>>,
    length: u64,
}

impl Reader<'_> {
    /// The next segment, checked; none past the last.
    fn next_segment(&mut self) -> anyhow::Result<Option<Vec<u8>>> {
        let Some(hash) = self.segments.next() else {
            ensure!(
                self.length == self.size,
                "the index's segments don't add up to its size"
            );
            return Ok(None);
        };
        let (bytes, _) = self
            .store
            .get(&segment(&self.engine, &hash))?
            .with_context(|| format!("the index's segment {hash} is missing"))?;
        ensure!(
            bytes.len() <= SEGMENT && sha256(&bytes) == hash,
            "the index's segment differs from its manifest"
        );
        self.length += bytes.len() as u64;
        ensure!(
            self.length <= self.size,
            "the index's segments pass its size"
        );
        Ok(Some(bytes))
    }
}

impl std::io::Read for Reader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let n = self.current.read(buf)?;
            if n > 0 || buf.is_empty() {
                return Ok(n);
            }
            match self.next_segment() {
                Ok(Some(bytes)) => self.current = std::io::Cursor::new(bytes),
                Ok(None) => return Ok(0),
                Err(err) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
            }
        }
    }
}

/// The copy, written as a stream ([`std::io::Write`]): a segment sent as
/// each fills, if the copy lacks it, so it holds one segment at a time;
/// then [`Writer::finish`] writes the manifest.
pub struct Writer<'a> {
    store: &'a dyn Store,
    engine: String,
    read: Option<Version>,
    /// The segments the copy holds, by hash.
    held: std::collections::BTreeSet<String>,
    part: Vec<u8>,
    segments: Vec<String>,
    size: u64,
}

impl<'a> Writer<'a> {
    /// A copy to write over `read`'s version (none: there was no copy).
    pub fn new(
        store: &'a dyn Store,
        engine: &str,
        read: Option<&Version>,
    ) -> anyhow::Result<Writer<'a>> {
        let prefix = name(engine, "seg-");
        let held = store
            .list(&prefix)?
            .into_iter()
            .filter_map(|o| o.path.strip_prefix(&prefix).map(str::to_string))
            .collect();
        Ok(Writer {
            store,
            engine: engine.to_string(),
            read: read.cloned(),
            held,
            part: Vec::with_capacity(SEGMENT),
            segments: Vec::new(),
            size: 0,
        })
    }

    /// The segment written so far sent, if the copy lacks it.
    fn send(&mut self) -> anyhow::Result<()> {
        if self.part.is_empty() {
            return Ok(());
        }
        let hash = sha256(&self.part);
        if !self.held.contains(&hash) {
            // Named by what it holds: whoever writes it writes the same.
            self.store
                .put(&segment(&self.engine, &hash), &self.part, &Expect::Any)?;
        }
        self.segments.push(hash);
        self.part.clear();
        Ok(())
    }

    /// The last segment sent, then the manifest, if the copy is still the
    /// version read: its new version, or a [`super::Conflict`] when another
    /// wrote it meanwhile. Segments no manifest names after it are removed.
    pub fn finish(mut self) -> anyhow::Result<Version> {
        self.send()?;
        let manifest = Manifest {
            format: 1,
            size: self.size,
            segments: std::mem::take(&mut self.segments),
        };
        let expect = match &self.read {
            Some(version) => Expect::Version(version.clone()),
            None => Expect::Absent,
        };
        let version = self.store.put(
            &name(&self.engine, MANIFEST),
            &serde_json::to_vec(&manifest)?,
            &expect,
        )?;
        let named: std::collections::BTreeSet<&String> = manifest.segments.iter().collect();
        for hash in self.held.iter().filter(|h| !named.contains(h)) {
            let _ = self
                .store
                .delete(&segment(&self.engine, hash), &Expect::Any);
        }
        Ok(version)
    }
}

impl std::io::Write for Writer<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = buf.len().min(SEGMENT - self.part.len());
        if self.size + n as u64 > COPY_MAX {
            return Err(std::io::Error::other("the index is too large"));
        }
        self.part.extend_from_slice(&buf[..n]);
        self.size += n as u64;
        if self.part.len() == SEGMENT {
            self.send().map_err(std::io::Error::other)?;
        }
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The copy gone: its manifest, then every segment.
pub fn remove(store: &dyn Store, engine: &str) -> anyhow::Result<()> {
    store.delete(&name(engine, MANIFEST), &Expect::Any)?;
    for object in store.list(&name(engine, "seg-"))? {
        store.delete(&object.path, &Expect::Any)?;
    }
    Ok(())
}

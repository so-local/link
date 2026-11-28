//! The stream an agent moves in (`lnk agent move`), as every
//! end of it reads and writes it: `lnk agent state-send` and `receive`, a cloud
//! that hosts agents (`agent-receive`, `agent-send`, `link_agent_place`), and
//! `lnk storage` for such an agent's Home.
//!
//! A stream is a tar. Its first entry is the manifest, [`MANIFEST`], a JSON
//! object whose [`Head`] says the version of the move protocol it speaks
//! ([`PROTOCOL`]), the phase and the move; the
//! rest of it is the sender's to say. Each file in it is preceded by a PAX
//! header [`SHA256`], its SHA-256 in hex, which the receiver checks against
//! what it wrote: a file whose hash differs, or that has none, refuses the
//! stream. Only files, folders and links are in it, each under a folder
//! the phase allows (`files/`, `memory/`, `harness/`, `connections/`), and
//! it ends with an empty entry, [`END`]: a stream cut short is refused.
//!
//! `files/` is the agent's folder as the agent sees it
//! ([`crate::folder::Folder::seen`]): Home's files, and beside them its
//! `instructions/`, `skills/` and `chats/`, kept in its own
//! folder; `memory/<engine>/` is its memory engine's index.
//!
//! What a receiver holds already is its [`Have`]: each file's path in the
//! stream and its hash. A sender skips each file whose hash it holds, so
//! a copy tried again, or a move back to a machine that kept the agent's
//! files, sends only what's new.

use link_format::WithinExt;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};

use link_digest::sha256_of;

pub mod collections;

/// The move protocol, as `moving/protocol.toml` declares it: the version
/// a stream's manifest states in its `format`, and the oldest a receiver
/// still reads. A stream is one-way, so its reader refuses an older one
/// and a newer one alike, by name ([`check_format`]).
pub static PROTOCOL: std::sync::LazyLock<link_version::Protocol> = std::sync::LazyLock::new(|| {
    link_version::Protocol::shipped(include_str!("moving/protocol.toml"))
});

/// The manifest's name, the stream's first entry, whatever its version.
pub const MANIFEST: &str = "lnk-move.json";

/// The stream's last entry, empty: a stream cut short, even between two
/// entries, has none, and is refused.
pub const END: &str = "lnk-move.end";

/// Refused unless a manifest's `format`, the move protocol's version its
/// sender speaks, is one this release reads: one below its oldest or
/// above its own is refused by name, naming the upgrade.
pub fn check_format(manifest: &serde_json::Value) -> anyhow::Result<()> {
    let format = manifest["format"]
        .as_u64()
        .and_then(|f| u32::try_from(f).ok())
        .with_context(|| format!("{MANIFEST} states no move protocol version (format)"))?;
    PROTOCOL.read("the agent's stream", format)?;
    Ok(())
}

/// The PAX header holding a file's SHA-256, in lowercase hex.
pub const SHA256: &str = "LNK.sha256";

/// The largest manifest a receiver reads.
pub const MAX_MANIFEST: u64 = 16 << 20;

/// The largest [`Have`] a receiver or a sender reads, as JSON.
pub const MAX_HAVE: u64 = 64 << 20;

/// A file this size or smaller is hashed in memory before it's sent;
/// a larger one is copied aside once (an unnamed file), so what's sent is
/// what was hashed even while the file changes.
pub const IN_MEMORY: u64 = 8 << 20;

/// The largest extension entry a receiver reads (a PAX header, or GNU's
/// long name or link): each is read whole into memory, and a sender's
/// say a file's hash and little more.
pub const MAX_EXTENSION: u64 = 64 << 10;

/// The most a receiver reads of one stream: past it, the stream is
/// refused, so no sender fills the receiver's disk.
pub const MAX_STREAM: u64 = 64 << 30;

/// The deepest a path in a stream is, below its phase's folder
/// (`files/`): as deep as the sender adds and the receiver puts in place
/// ([`link_files::nofollow::DEEPEST`]).
pub const DEEPEST: usize = link_files::nofollow::DEEPEST;

/// A stream as [`unpack`] reads it: at most [`MAX_STREAM`] of it, and
/// each extension entry at most [`MAX_EXTENSION`], told from the tar
/// headers as they pass, before the archive reads one whole.
struct Bounded<R> {
    input: R,
    read: u64,
    /// Bytes until the next header, and where in a header this is.
    until_header: u64,
    header: Vec<u8>,
    /// A PAX header's data as it passes, to read the size it gives the
    /// next entry, which its header's own size field doesn't say.
    pax: Option<Vec<u8>>,
    pax_left: u64,
    next_size: Option<u64>,
}

impl<R: Read> Bounded<R> {
    fn new(input: R) -> Self {
        Bounded {
            input,
            read: 0,
            until_header: 0,
            header: Vec::with_capacity(512),
            pax: None,
            pax_left: 0,
            next_size: None,
        }
    }

    /// Follows `bytes`, just read, through the headers and data they hold.
    fn follow(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            if self.until_header > 0 {
                let n = bytes.len().min(self.until_header as usize);
                if let Some(pax) = &mut self.pax {
                    let take = n.min(self.pax_left as usize);
                    pax.extend_from_slice(&bytes[..take]);
                    self.pax_left -= take as u64;
                    if self.pax_left == 0 {
                        let pax = self.pax.take().unwrap_or_default();
                        self.next_size = pax_size(&pax);
                    }
                }
                self.until_header -= n as u64;
                bytes = &bytes[n..];
                continue;
            }
            let n = bytes.len().min(512 - self.header.len());
            self.header.extend_from_slice(&bytes[..n]);
            bytes = &bytes[n..];
            if self.header.len() == 512 {
                let header = std::mem::take(&mut self.header);
                self.header_read(&header)?;
            }
        }
        Ok(())
    }

    /// A whole header block: what follows it, and an extension refused
    /// when it's over [`MAX_EXTENSION`].
    fn header_read(&mut self, header: &[u8]) -> io::Result<()> {
        let refused = |why: String| io::Error::new(io::ErrorKind::InvalidData, why);
        if header.iter().all(|b| *b == 0) {
            return Ok(());
        }
        let size = header_size(&header[124..136])
            .ok_or_else(|| refused("a header in the stream has no size".into()))?;
        let kind = header[156];
        let extension = matches!(kind, b'x' | b'g' | b'L' | b'K');
        if extension && size > MAX_EXTENSION {
            return Err(refused(format!(
                "an extension entry of {size} bytes is over {} KB: the stream is refused",
                MAX_EXTENSION >> 10
            )));
        }
        let size = match extension {
            true => size,
            false => self.next_size.take().unwrap_or(size),
        };
        if kind == b'x' && size > 0 {
            self.pax = Some(Vec::with_capacity(size as usize));
            self.pax_left = size;
        }
        self.until_header = size.div_ceil(512) * 512;
        Ok(())
    }
}

impl<R: Read> Read for Bounded<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.input.read(buf)?;
        self.read += n as u64;
        if self.read > MAX_STREAM {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the stream is over {} GB: refused", MAX_STREAM >> 30),
            ));
        }
        self.follow(&buf[..n])?;
        Ok(n)
    }
}

/// A tar header's size field: octal, or base-256 when its first byte's
/// high bit is set.
fn header_size(field: &[u8]) -> Option<u64> {
    if field[0] & 0x80 != 0 {
        let mut size: u64 = (field[0] & 0x7f).into();
        for b in &field[1..] {
            size = size.checked_mul(256)?.checked_add((*b).into())?;
        }
        return Some(size);
    }
    let text = std::str::from_utf8(field).ok()?;
    let text = text.trim_matches(|c: char| c == '\0' || c == ' ');
    if text.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(text, 8).ok()
}

/// The `size` a PAX header's records give the next entry, if any.
fn pax_size(pax: &[u8]) -> Option<u64> {
    let mut rest = pax;
    let mut size = None;
    while !rest.is_empty() {
        let space = rest.iter().position(|b| *b == b' ')?;
        let len: usize = std::str::from_utf8(&rest[..space]).ok()?.parse().ok()?;
        if len <= space || len > rest.len() {
            return None;
        }
        let record = &rest[space + 1..len];
        let record = record.strip_suffix(b"\n").unwrap_or(record);
        if let Some(value) = record.strip_prefix(b"size=") {
            size = std::str::from_utf8(value).ok()?.parse().ok();
        }
        rest = &rest[len..];
    }
    size
}

/// The authenticated envelope of a move's complete switch stream.
pub fn stream_about() -> link_secretive::About {
    link_secretive::About {
        owner: "move".into(),
        kind: "stream".into(),
        version: 1,
    }
}

/// Which part of a move a stream is, as the hosting contract declares it.
pub use link_agent_place::Phase;

/// What every manifest says, whoever reads it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Head {
    pub format: u32,
    pub phase: Phase,
    /// The move's ID ([`new_move_id`]): the same in its copy and its
    /// switch.
    #[serde(rename = "move")]
    pub move_id: String,
    /// In a switch: the paths in `files/`, `memory/` and `harness/` gone
    /// since the copy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
}

/// What a receiver holds: each file's path in the stream (`files/...`,
/// `memory/...`) and its SHA-256 in hex.
pub type Have = BTreeMap<String, String>;

/// The fields of a switch's manifest that hold its secrets: its model
/// and its fallback (their keys), its connections (their tokens), and its
/// buckets (their password).
pub const SECRET_FIELDS: &[&str] = &["model", "fallback", "connected", "bucket"];

/// What a switch's secrets are sealed as, for the agent `id`.
pub fn sealed_about(id: &str) -> link_secretive::About {
    link_secretive::About {
        owner: format!("agent:{id}"),
        kind: "move".into(),
        version: 1,
    }
}

/// A switch's `manifest` with its [`SECRET_FIELDS`] taken out and sealed,
/// whole, to `to` (the key the receiving `lnk agent receive` made for
/// this switch and said), in `"sealed"`: what every sender of a switch
/// writes (`lnk agent state-send --seal-to`, an account's `agent-send`).
pub fn seal_manifest(
    manifest: &mut serde_json::Map<String, serde_json::Value>,
    to: &link_secretive::SealTo,
) -> anyhow::Result<()> {
    let id = manifest
        .get("agent")
        .and_then(|agent| agent.get("id"))
        .and_then(serde_json::Value::as_str)
        .within("the manifest names no agent")?
        .to_string();
    let mut taken = link_secretive::Secret::new(link_secretive::Json::default());
    let mut fields = serde_json::Map::new();
    for field in SECRET_FIELDS {
        if let Some(value) = manifest.remove(*field) {
            fields.insert(field.to_string(), value);
        }
    }
    *taken.expose_mut() = link_secretive::Json(serde_json::Value::Object(fields));
    let sealed = link_secretive::Sealed::seal_json(taken.expose(), to, sealed_about(&id))
        .map_err(|e| anyhow::anyhow!("sealing the switch's secrets: {e}"))?;
    manifest.insert("sealed".into(), serde_json::to_value(sealed)?);
    Ok(())
}

/// A switch's `manifest` with what [`seal_manifest`] sealed opened by
/// `recipient` and put back. Refused when it has no seal, when it
/// carries any of [`SECRET_FIELDS`] beside it, or when the seal isn't
/// `recipient`'s: once a receiver has said what to seal to, a secret
/// that comes unsealed never passes.
pub fn open_manifest(
    manifest: &mut serde_json::Map<String, serde_json::Value>,
    recipient: &link_secretive::Recipient,
) -> anyhow::Result<()> {
    let sealed = manifest
        .remove("sealed")
        .within("the switch carries its secrets unsealed: refused")?;
    ensure!(
        SECRET_FIELDS
            .iter()
            .all(|field| !manifest.contains_key(*field)),
        "the switch carries secrets beside its seal: refused"
    );
    let id = manifest
        .get("agent")
        .and_then(|agent| agent.get("id"))
        .and_then(serde_json::Value::as_str)
        .within("the manifest names no agent")?
        .to_string();
    let sealed: link_secretive::Sealed<link_secretive::Secret<Vec<u8>>> =
        serde_json::from_value(sealed).within("the switch's seal isn't one")?;
    let mut opened: link_secretive::Secret<link_secretive::Json> = recipient
        .open_json(&sealed, &sealed_about(&id))
        .map_err(|e| anyhow::anyhow!("opening what the switch carries: {e}"))?;
    let serde_json::Value::Object(fields) = std::mem::take(&mut opened.expose_mut().0) else {
        bail!("what the switch carries isn't its fields");
    };
    for (field, value) in fields {
        ensure!(
            SECRET_FIELDS.contains(&field.as_str()),
            "the switch's seal holds {field:?}, which isn't a secret's field"
        );
        manifest.insert(field, value);
    }
    Ok(())
}

/// A new move's ID: 128 random bits, in hex.
pub fn new_move_id() -> anyhow::Result<String> {
    link_files::random_token().within("making the move's ID")
}

/// An error unless `id` is a move's ID: 32 lowercase hex characters.
pub fn check_move_id(id: &str) -> anyhow::Result<()> {
    ensure!(
        id.len() == 32
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{:?} isn't a move's ID",
        link_format::clean(id)
    );
    Ok(())
}

/// An error unless `hash` is a SHA-256 in lowercase hex.
pub fn check_sha256(hash: &str) -> anyhow::Result<()> {
    ensure!(
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{:?} isn't a SHA-256",
        link_format::clean(hash)
    );
    Ok(())
}

/// An error unless `path` is a plain path in a stream under one of
/// `folders`: relative, each part a name (no `..`, no `.`), none empty.
pub fn check_path(path: &str, folders: &[&str]) -> anyhow::Result<()> {
    let plain = !path.is_empty()
        && path.len() <= 4096
        && !path.contains('\0')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && part.len() <= 255);
    ensure!(
        plain,
        "{:?} isn't a plain path in the stream",
        link_format::clean(path)
    );
    ensure!(
        path.split('/').count() <= DEEPEST + 1,
        "{:?} is more than {DEEPEST} folders deep: the stream is refused",
        link_format::clean(path)
    );
    let first = path.split('/').next().unwrap_or_default();
    ensure!(
        folders.contains(&first),
        "{:?} isn't in {} of the stream",
        link_format::clean(path),
        folders.join("/, ") + "/"
    );
    Ok(())
}

/// An error unless every path of `have` is a plain one in `files/` or
/// `memory/`, and every hash a SHA-256.
pub fn check_have(have: &Have) -> anyhow::Result<()> {
    for (path, hash) in have {
        check_path(path, &["files", "memory", "harness"])?;
        check_sha256(hash)?;
    }
    Ok(())
}

/// A [`Have`] read from `input`, as JSON, at most [`MAX_HAVE`] of it, and
/// checked ([`check_have`]).
pub fn read_have(input: impl Read) -> anyhow::Result<Have> {
    let mut bytes = Vec::new();
    input.take(MAX_HAVE + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_HAVE,
        "what the other end holds is over {} MB: not read",
        MAX_HAVE >> 20
    );
    let have: Have = serde_json::from_slice(&bytes).within("reading what the other end holds")?;
    check_have(&have)?;
    Ok(have)
}

/// The manifest, `json`, as the stream's first entry, with its hash.
pub fn append_manifest<W: Write>(tar: &mut tar::Builder<W>, json: &[u8]) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(json.len() as u64);
    header.set_mode(0o600);
    let hash = sha256_of(json)?;
    tar.append_pax_extensions([(SHA256, hash.as_bytes())])?;
    header.set_cksum();
    tar.append_data(&mut header, MANIFEST, json)
}

/// The stream's last entry, [`END`]; the caller finishes the tar after
/// it.
pub fn append_end<W: Write>(tar: &mut tar::Builder<W>) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_mode(0o600);
    let hash = sha256_of(io::empty())?;
    tar.append_pax_extensions([(SHA256, hash.as_bytes())])?;
    header.set_cksum();
    tar.append_data(&mut header, END, io::empty())
}

/// Appends a file of `len` bytes from `data` as `path`, with `header`'s
/// metadata, its SHA-256 in a PAX header before it. `data` is read once:
/// held in memory up to [`IN_MEMORY`], beyond it copied into an unnamed
/// file in `spool` (a private folder), so what's sent is what was hashed.
/// Returns the hash, and whether it was sent: not when `have` holds
/// `path` with that hash.
pub fn append_file<W: Write>(
    tar: &mut tar::Builder<W>,
    header: &mut tar::Header,
    path: &Path,
    data: impl Read,
    len: u64,
    have: &Have,
    spool: &Path,
) -> io::Result<(String, bool)> {
    // Exactly `len` bytes, were it to grow or shrink while read.
    let mut data = data.take(len).chain(io::repeat(0)).take(len);
    let held: Box<dyn ReadSeek> = match len <= IN_MEMORY {
        true => {
            let mut bytes = Vec::with_capacity(len as usize);
            data.read_to_end(&mut bytes)?;
            Box::new(io::Cursor::new(bytes))
        }
        false => {
            let mut file = unnamed(spool)?;
            io::copy(&mut data, &mut file)?;
            file.rewind()?;
            Box::new(file)
        }
    };
    let mut held = held;
    let hash = sha256_of(&mut held)?;
    held.rewind()?;
    let shown = path.to_string_lossy();
    if have.get(shown.as_ref()) == Some(&hash) {
        return Ok((hash, false));
    }
    tar.append_pax_extensions([(SHA256, hash.as_bytes())])?;
    header.set_size(len);
    tar.append_data(header, path, held)?;
    Ok((hash, true))
}

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// A private unnamed file for a transfer spool, removed when closed.
pub fn temporary_file() -> io::Result<File> {
    unnamed(&link_files::layout::Hidden::TransferScratch.path())
}

/// A new file in the folder `spool`, unnamed once it's open: no one else
/// can open it, and it's gone when it's closed.
fn unnamed(spool: &Path) -> io::Result<File> {
    link_files::nofollow::temporary(spool)
}

/// The folder `root` (a folder of the caller's own, read without
/// following a link in it) under `prefix` in the stream: each folder,
/// file and link in it, files with their hashes, but those `have` holds
/// the same. `only`, when given, keeps to those paths under `root` (and
/// what's in them). Returns the paths of the files it holds (`prefix/...`),
/// sent or not.
pub fn append_folder<W: Write>(
    tar: &mut tar::Builder<W>,
    root: &Path,
    prefix: &str,
    have: &Have,
    only: Option<&[PathBuf]>,
    spool: &Path,
) -> anyhow::Result<Vec<String>> {
    let mut files = Vec::new();
    let meta = std::fs::symlink_metadata(root).within(format!("reading {}", root.display()))?;
    ensure!(
        meta.is_dir(),
        "{} isn't a folder (a link? not following it)",
        root.display()
    );
    let mut header = tar::Header::new_gnu();
    header.set_metadata_in_mode(&meta, tar::HeaderMode::Complete);
    tar.append_data(&mut header, prefix, io::empty())?;
    walk(
        tar,
        root,
        Path::new(""),
        Path::new(prefix),
        have,
        only,
        spool,
        &mut files,
    )?;
    Ok(files)
}

#[expect(
    clippy::too_many_arguments,
    reason = "a recursive walk's state, passed down whole at each level"
)]
fn walk<W: Write>(
    tar: &mut tar::Builder<W>,
    root: &Path,
    rel: &Path,
    prefix: &Path,
    have: &Have,
    only: Option<&[PathBuf]>,
    spool: &Path,
    files: &mut Vec<String>,
) -> anyhow::Result<()> {
    let dir = root.join(rel);
    let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(&dir)
        .within(format!("reading {}", dir.display()))?
        .map(|e| e.map(|e| e.file_name()))
        .collect::<io::Result<_>>()?;
    names.sort();
    for name in names {
        let rel = rel.join(&name);
        let wanted = only.is_none_or(|only| {
            only.iter()
                .any(|o| rel.starts_with(o) || o.starts_with(&rel))
        });
        if !wanted {
            continue;
        }
        let path = root.join(&rel);
        let entry = prefix.join(&rel);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let mut header = tar::Header::new_gnu();
        header.set_metadata_in_mode(&meta, tar::HeaderMode::Complete);
        let kind = meta.file_type();
        if kind.is_symlink() {
            let target = std::fs::read_link(&path)?;
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            tar.append_link(&mut header, &entry, target)?;
        } else if kind.is_dir() {
            tar.append_data(&mut header, &entry, io::empty())?;
            walk(tar, root, &rel, prefix, have, only, spool, files)?;
        } else if kind.is_file() {
            // Opened as the file it was listed as, never through a link
            // swapped in since.
            let file = match open_under(root, &rel) {
                Ok(file) => file,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(err).within(format!("reading {}", path.display()));
                }
            };
            let len = file.metadata()?.len();
            append_file(tar, &mut header, &entry, file, len, have, spool)
                .within(format!("reading {}", path.display()))?;
            files.push(entry.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

/// What the folder `root` holds, as a [`Have`] under `prefix`
/// (`files`): each file's hash, read without following a link. None for
/// a folder that isn't there.
pub fn have_of(root: &Path, prefix: &str) -> anyhow::Result<Have> {
    let mut have = Have::new();
    if !std::fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
        return Ok(have);
    }
    hash_walk(root, Path::new(""), prefix, &mut have)?;
    Ok(have)
}

fn hash_walk(root: &Path, rel: &Path, prefix: &str, have: &mut Have) -> anyhow::Result<()> {
    let dir = root.join(rel);
    for entry in std::fs::read_dir(&dir).within(format!("reading {}", dir.display()))? {
        let entry = entry?;
        let rel = rel.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            hash_walk(root, &rel, prefix, have)?;
        } else if kind.is_file() {
            let file = match open_under(root, &rel) {
                Ok(file) => file,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(err).within(format!("reading {}", root.join(&rel).display()));
                }
            };
            let path = Path::new(prefix).join(&rel);
            have.insert(path.to_string_lossy().into_owned(), sha256_of(file)?);
        }
    }
    Ok(())
}

/// The file at `rel` in the folder `root`, opened for reading one name
/// at a time from `root`, never through a link: one on the way, or in
/// its place, is refused.
pub fn open_under(root: &Path, rel: &Path) -> io::Result<File> {
    let names: Vec<&OsStr> = rel
        .components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name),
            _ => Err(io::Error::from(io::ErrorKind::InvalidInput)),
        })
        .collect::<io::Result<_>>()?;
    let Some((last, folders)) = names.split_last() else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    let mut at = link_files::nofollow::Dir::open(root)?;
    for name in folders {
        at = at.folder(name)?;
    }
    at.open_file(last, libc::O_RDONLY, 0)
}

/// A stream read by [`unpack`]: its manifest, as JSON, and its head.
#[derive(Debug)]
pub struct Unpacked {
    pub manifest: serde_json::Value,
    pub head: Head,
}

/// Reads a stream from `input` into the folder `into` gives (a private
/// folder of the receiver's, made by it once it has read the manifest),
/// checking each entry: the manifest first, of this format; then only
/// files, folders and links, each a plain path under a folder its phase
/// allows; each file with its hash, which what's written must have. A
/// file whose hash differs is removed, and the stream refused. The
/// stream is read to its end, so the sender, still writing, never finds
/// the pipe closed.
///
/// The stream was written by another machine, from what a harness, or a
/// cloud, wrote: it's hostile. A link is unpacked as a link, never
/// followed by what comes after it (tar's own checks), and modes keep
/// their permission bits only.
pub fn unpack(
    input: impl Read,
    into: impl FnOnce(&serde_json::Value, &Head) -> anyhow::Result<PathBuf>,
) -> anyhow::Result<Unpacked> {
    let mut archive = tar::Archive::new(Bounded::new(input));
    archive.set_preserve_permissions(false);
    archive.set_overwrite(true);
    let unpacked = (|| -> anyhow::Result<Unpacked> {
        let mut entries = archive.entries()?;
        let first = entries
            .next()
            .within("the stream is empty")?
            .within("reading the stream")?;
        let (manifest, head) = read_manifest(first)?;
        let root = into(&manifest, &head)?;
        link_files::create_private_dir(&root).within(format!("creating {}", root.display()))?;
        ensure!(
            std::fs::symlink_metadata(&root).is_ok_and(|m| m.is_dir()),
            "{} isn't a folder (a link? not following it)",
            root.display()
        );
        let mut ended = false;
        for entry in entries {
            let entry = entry.within("reading the stream (it stopped?)")?;
            ensure!(!ended, "the stream goes on after its end: refused");
            if entry.path()?.as_os_str() == END {
                ended = true;
                continue;
            }
            unpack_entry(entry, &root, head.phase)?;
        }
        ensure!(
            ended,
            "the stream stopped before its end (the sender failed, or the connection broke)"
        );
        Ok(Unpacked { manifest, head })
    })();
    // Read to the end: the archive stops at its first end block.
    let drained = io::copy(&mut archive.into_inner(), &mut io::sink());
    unpacked.and_then(|unpacked| {
        drained?;
        Ok(unpacked)
    })
}

/// The manifest, the stream's first entry, read and checked against its
/// hash.
fn read_manifest<R: Read>(
    mut entry: tar::Entry<'_, R>,
) -> anyhow::Result<(serde_json::Value, Head)> {
    let path = entry.path()?.to_string_lossy().into_owned();
    match path.as_str() {
        MANIFEST => {}
        _ => bail!("the stream doesn't start with {MANIFEST}"),
    }
    ensure!(
        entry.header().entry_type() == tar::EntryType::Regular,
        "{MANIFEST} isn't a file"
    );
    let hash = sha_of(&mut entry)?;
    let size = entry.header().size()?;
    ensure!(
        size <= MAX_MANIFEST,
        "{MANIFEST} is over {} MB: not read",
        MAX_MANIFEST >> 20
    );
    let mut bytes = Vec::new();
    (&mut entry).take(MAX_MANIFEST).read_to_end(&mut bytes)?;
    ensure!(
        sha256_of(bytes.as_slice())? == hash,
        "{MANIFEST} isn't what was sent (its SHA-256 differs): not taken"
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&bytes).within(format!("reading {MANIFEST}"))?;
    check_format(&manifest)?;
    let head: Head =
        serde_json::from_value(manifest.clone()).within(format!("reading {MANIFEST}"))?;
    check_move_id(&head.move_id)?;
    for removed in &head.removed {
        check_path(removed, &["files", "memory", "harness"])?;
    }
    ensure!(
        head.removed.is_empty() || head.phase == Phase::Switch,
        "only a switch removes files"
    );
    Ok((manifest, head))
}

/// The hash an entry's PAX header gives it.
fn sha_of<R: Read>(entry: &mut tar::Entry<'_, R>) -> anyhow::Result<String> {
    let path = entry.path()?.to_string_lossy().into_owned();
    let mut found = None;
    if let Some(extensions) = entry.pax_extensions()? {
        for extension in extensions {
            let extension = extension?;
            if extension.key_bytes() == SHA256.as_bytes() {
                found = Some(String::from_utf8_lossy(extension.value_bytes()).into_owned());
            }
        }
    }
    let hash = found.within(format!(
        "{} came without its SHA-256: not taken",
        link_format::clean(&path)
    ))?;
    check_sha256(&hash)?;
    Ok(hash)
}

/// One entry after the manifest, unpacked in `root` and checked.
fn unpack_entry<R: Read>(
    mut entry: tar::Entry<'_, R>,
    root: &Path,
    phase: Phase,
) -> anyhow::Result<()> {
    let path = entry.path()?.to_string_lossy().into_owned();
    let path = path.trim_end_matches('/').to_string();
    check_path(&path, phase.accepts())?;
    let kind = entry.header().entry_type();
    match kind {
        tar::EntryType::Regular | tar::EntryType::Continuous => {
            let hash = sha_of(&mut entry)?;
            unpacked(entry.unpack_in(root), &path)?;
            let written = open_under(root, Path::new(&path))
                .and_then(sha256_of)
                .within(format!("reading back {}", link_format::clean(&path)))?;
            if written != hash {
                let _ = std::fs::remove_file(root.join(&path));
                bail!(
                    "{} isn't what was sent (its SHA-256 differs): the stream is refused",
                    link_format::clean(&path)
                );
            }
        }
        tar::EntryType::Directory | tar::EntryType::Symlink => {
            unpacked(entry.unpack_in(root), &path)?;
        }
        _ => bail!(
            "{} is neither a file, a folder nor a link: the stream is refused",
            link_format::clean(&path)
        ),
    }
    Ok(())
}

fn unpacked(result: io::Result<bool>, path: &str) -> anyhow::Result<()> {
    match result {
        Ok(true) => Ok(()),
        Ok(false) => bail!("{} leaves its folder: refused", link_format::clean(path)),
        Err(err) => Err(err).within(format!(
            "unpacking {} (through a link out of its folder?)",
            link_format::clean(path)
        )),
    }
}

/// Removes `path` (`files/...`, `memory/...` or `harness/...`) from the folder `root` and what's under
/// it, never through a link: one on the way is refused, one in its
/// place removed as a link.
pub fn remove_under(root: &Path, path: &str) -> anyhow::Result<()> {
    check_path(path, &["files", "memory", "harness"])?;
    let mut at = root.to_path_buf();
    let parts: Vec<&str> = path.split('/').collect();
    let Some((last, folders)) = parts.split_last() else {
        return Ok(());
    };
    for part in folders {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => bail!(
                "{} is a link, on the way to {}: not following it",
                at.display(),
                link_format::clean(path)
            ),
            Err(_) => return Ok(()),
        }
    }
    at.push(last);
    match std::fs::symlink_metadata(&at) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&at)?,
        Ok(_) => std::fs::remove_file(&at)?,
        Err(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests;

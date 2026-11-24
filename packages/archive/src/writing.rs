//! Writing an archive: a gzip-compressed tar file with `archive.toml`
//! first, then each folder's plain files and folders, in order, with no
//! owner, time or mode of this machine's, so the same state writes the
//! same archive anywhere.

use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use anyhow::{Context, bail, ensure};
use link_files::nofollow::{Dir, Kind};

use crate::{Replaced, limits};

/// An archive being written.
pub struct Writer<W: Write> {
    builder: tar::Builder<flate2::write::GzEncoder<W>>,
    entries: u64,
    bytes: u64,
}

/// The mode of each file and folder written: no machine's own.
const FILE_MODE: u32 = 0o644;
const DIR_MODE: u32 = 0o755;

impl<W: Write> Writer<W> {
    pub fn new(out: W) -> Self {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            out,
            flate2::Compression::default(),
        ));
        builder.follow_symlinks(false);
        Self {
            builder,
            entries: 0,
            bytes: 0,
        }
    }

    /// A file at `path` holding `bytes`.
    pub fn file(&mut self, path: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let mut header = header(tar::EntryType::Regular, FILE_MODE, bytes.len() as u64);
        self.builder
            .append_data(&mut header, path, bytes)
            .with_context(|| format!("writing {path} into the archive"))
    }

    /// A folder at `path`.
    pub fn dir(&mut self, path: &str) -> anyhow::Result<()> {
        let mut header = header(tar::EntryType::Directory, DIR_MODE, 0);
        self.builder
            .append_data(&mut header, format!("{path}/"), std::io::empty())
            .with_context(|| format!("writing {path} into the archive"))
    }

    /// Folder `from` whole, at `at`: its plain files and folders, by name,
    /// each opened by its folder's handle, never through a link, so one
    /// swapped in while it's read isn't followed. A link, a device, a file
    /// with a second name (a hard link) or anything else in it is refused,
    /// naming it, as is a path or a size the reader wouldn't admit.
    /// Returns the bytes of its files.
    pub fn tree(&mut self, at: &str, from: &Path) -> anyhow::Result<u64> {
        self.tree_replacing(at, from, &[])
    }

    /// [`Self::tree`], each file of `replaced` written from what goes in
    /// its place, read the same way, or left out.
    pub fn tree_replacing(
        &mut self,
        at: &str,
        from: &Path,
        replaced: &[Replaced],
    ) -> anyhow::Result<u64> {
        let dir = Dir::open_unlinked(from)
            .with_context(|| format!("reading {}, which must be a folder", from.display()))?;
        self.tree_in(at, &dir, from, replaced)
    }

    fn tree_in(
        &mut self,
        at: &str,
        dir: &Dir,
        shown: &Path,
        replaced: &[Replaced],
    ) -> anyhow::Result<u64> {
        self.entry(at)?;
        self.dir(at)?;
        let mut bytes = 0;
        for name in dir
            .names()
            .with_context(|| format!("reading {}", shown.display()))?
        {
            let path = shown.join(&name);
            let Some(text) = name.to_str() else {
                bail!(
                    "{} has a name that isn't UTF-8, which an archive can't carry",
                    path.display()
                );
            };
            let kind = dir.kind(&name)?;
            let swap = replaced.iter().find(|r| r.file == path);
            if kind == Kind::File && swap.is_some_and(|r| r.with.is_none()) {
                continue;
            }
            let child = format!("{at}/{text}");
            self.entry(&child)
                .with_context(|| format!("{} can't be carried", path.display()))?;
            match kind {
                Kind::Folder => {
                    let folder = dir
                        .folder(&name)
                        .with_context(|| format!("reading {}", path.display()))?;
                    bytes += self.tree_in(&child, &folder, &path, replaced)?;
                }
                Kind::File => {
                    let (file, read) = match swap.and_then(|r| r.with.as_deref()) {
                        Some(with) => (open_unlinked_file(with), with),
                        None => (dir.open_entry(&name), path.as_path()),
                    };
                    let file = file.with_context(|| format!("reading {}", read.display()))?;
                    bytes += self.plain(&child, file, read)?;
                }
                Kind::Link | Kind::Other => bail!(
                    "{} is a link or a device, which an archive doesn't carry: move it out, then \
                     export again",
                    path.display()
                ),
            }
        }
        Ok(bytes)
    }

    /// Plain file `file`, read from `shown`, at `child`: refused when it
    /// has a second name (a hard link) or isn't a plain file, or past
    /// the archive's size. Returns its bytes.
    fn plain(&mut self, child: &str, file: std::fs::File, shown: &Path) -> anyhow::Result<u64> {
        let meta = file.metadata()?;
        if !meta.is_file() || meta.nlink() > 1 {
            bail!(
                "{} is a file with another name too (a hard link), or not a plain file, which an \
                 archive doesn't carry: copy it, then export again",
                shown.display()
            );
        }
        let size = meta.len();
        self.bytes = self.bytes.saturating_add(size);
        ensure!(
            self.bytes <= limits().max_bytes,
            "it holds more than {} bytes, the most an archive may",
            limits().max_bytes
        );
        let mut header = header(tar::EntryType::Regular, FILE_MODE, size);
        self.builder
            .append_data(&mut header, child, file.take(size))
            .with_context(|| format!("writing {} into the archive", shown.display()))?;
        Ok(size)
    }

    /// One more entry at `path`, as the reader would admit it.
    fn entry(&mut self, path: &str) -> anyhow::Result<()> {
        crate::reading::check_path(path)?;
        self.entries += 1;
        ensure!(
            self.entries < limits().max_entries,
            "it holds more than {} entries, the most an archive may",
            limits().max_entries
        );
        Ok(())
    }

    /// The archive, written whole.
    pub fn finish(self) -> anyhow::Result<W> {
        let gz = self.builder.into_inner().context("finishing the archive")?;
        gz.finish().context("finishing the archive")
    }
}

/// The file at `path`, never through a link.
fn open_unlinked_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

fn header(kind: tar::EntryType, mode: u32, size: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(mode);
    header.set_size(size);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    header
}

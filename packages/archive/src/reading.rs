//! Reading an archive as untrusted data: executing nothing, admitting
//! only `archive.toml` (its first entry), the root files beside it and
//! the folders it names, as plain files and folders, within the limits
//! and the space free here. An absolute path, `..`, a link, a device or
//! a duplicate entry is refused, naming the entry. It's staged in
//! `~/.lnk`, never in `~/Link`, and moved into place only once whole.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail, ensure};
use link_files::layout::{Home, Part, map::Entry};

use crate::{Manifest, Selection, limits};

/// One of a staged thing's folders.
#[derive(Clone, Debug)]
pub struct Folder {
    pub part: Part,
    /// The location whose folder of a thing this is.
    pub home: Home,
    /// Where it's staged.
    pub staged: PathBuf,
}

/// A thing read from an archive, staged.
#[derive(Clone, Debug)]
pub struct StagedThing {
    pub selection: Selection,
    /// The word of its owner in `layout.toml`.
    pub owner: String,
    pub folders: Vec<Folder>,
}

/// An archive read whole into staging, removed when dropped.
#[derive(Debug)]
pub struct Staged {
    root: PathBuf,
    pub manifest: Manifest,
    pub things: Vec<StagedThing>,
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A thing's folder in an archive: its part, its location and its path
/// in the archive (`Agents/programmer`).
type Planned = (Part, Home, String);

/// Where each thing's folders are in an archive, as its map says.
pub(crate) struct Plan {
    /// Each thing, its owner, and its folders.
    things: Vec<(Selection, String, Vec<Planned>)>,
}

impl Plan {
    /// Reads `manifest`: each thing's kind known here, each key of its map
    /// one this Link knows, each path inside the archive and apart from
    /// the others.
    pub(crate) fn of(manifest: &Manifest) -> anyhow::Result<Self> {
        let limits = limits();
        let mut paths: BTreeMap<String, Home> = BTreeMap::new();
        let mut homes = BTreeMap::new();
        for (key, path) in &manifest.map {
            let home = match Entry::parse(key) {
                Ok(Entry::Home(home)) => home,
                Ok(_) => bail!("archive.toml maps {key}, which isn't in the Link folder"),
                Err(err) => bail!("archive.toml maps {key:?}, which this Link doesn't know: {err}"),
            };
            check_path(path).with_context(|| format!("archive.toml maps {key} to {path:?}"))?;
            ensure!(
                !limits.root_files.contains(path),
                "archive.toml maps {key} to {path}, the archive's own file"
            );
            for other in paths.keys() {
                ensure!(
                    !overlaps(path, other),
                    "archive.toml maps two locations where one is inside the other: {path} and {other}"
                );
            }
            paths.insert(path.clone(), home);
            homes.insert(home, path.clone());
        }
        let mut things = Vec::new();
        for key in manifest.things.keys() {
            let selection =
                Selection::parse(key).with_context(|| format!("in archive.toml's [{key:?}]"))?;
            let Some((owner, _)) = link_files::layout::owner_of_kind(selection.kind()) else {
                bail!(
                    "{selection} is a {}, which no kind here is",
                    selection.kind()
                );
            };
            let mut folders = Vec::new();
            for (part, home) in link_files::layout::state_locations(owner) {
                let Some(at) = homes.get(&home) else {
                    bail!(
                        "archive.toml's map doesn't say where {} is, which holds {selection}",
                        Entry::Home(home).name()
                    );
                };
                folders.push((part, home, format!("{at}/{}", selection.name())));
            }
            things.push((selection, owner.to_string(), folders));
        }
        ensure!(!things.is_empty(), "archive.toml names no thing");
        Ok(Self { things })
    }

    /// Each thing's folders, by their paths in the archive.
    pub(crate) fn folders(&self) -> impl Iterator<Item = &str> {
        self.things
            .iter()
            .flat_map(|(_, _, f)| f)
            .map(|(_, _, at)| at.as_str())
    }

    /// Whether `path` may be in the archive: in one of its things'
    /// folders, or a folder leading to one.
    fn admits(&self, path: &str, dir: bool) -> bool {
        self.things
            .iter()
            .flat_map(|(_, _, f)| f)
            .any(|(_, _, at)| {
                path == at
                    || path.starts_with(&format!("{at}/"))
                    || (dir && at.starts_with(&format!("{path}/")))
            })
    }
}

/// Whether one of two archive paths is the other or inside it.
fn overlaps(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

/// A path in the archive: relative, plain names only, within its limit.
pub(crate) fn check_path(path: &str) -> anyhow::Result<()> {
    ensure!(
        path.len() <= limits().max_path_bytes,
        "its path is longer than {} bytes",
        limits().max_path_bytes
    );
    ensure!(!path.is_empty(), "its path is empty");
    for component in Path::new(path).components() {
        match component {
            Component::Normal(name) if name != "." && !name.is_empty() => {}
            _ => bail!("{path:?} isn't a plain path inside the archive"),
        }
    }
    ensure!(
        !path.contains('\\') && !path.contains("//"),
        "{path:?} isn't a plain path inside the archive"
    );
    Ok(())
}

/// The bytes free for this user at `at`.
fn free_bytes(at: &Path) -> anyhow::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(at.as_os_str().as_bytes())?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a NUL-terminated string and `stat` a buffer
    // statvfs fills.
    let done = unsafe { libc::statvfs(path.as_ptr(), &mut stat) };
    ensure!(
        done == 0,
        "reading the space free at {}: {}",
        at.display(),
        std::io::Error::last_os_error()
    );
    #[allow(
        clippy::unnecessary_cast,
        reason = "statvfs's fields are u64 on Linux and narrower on macOS"
    )]
    Ok(stat.f_bavail as u64 * stat.f_frsize as u64)
}

/// An archive's decompressed bytes, never more than an archive may hold
/// with its headers, however small the file.
fn decoded(file: std::fs::File) -> impl Read {
    let limits = limits();
    let most = limits
        .max_bytes
        .saturating_add(limits.max_entries.saturating_mul(2048));
    flate2::read::GzDecoder::new(file).take(most)
}

/// A GNU long name's text: the next entry's path, within the limit.
fn long_name<R: Read>(entry: &mut tar::Entry<R>) -> anyhow::Result<String> {
    let most = limits().max_path_bytes as u64 + 1;
    ensure!(
        entry.size() <= most,
        "a long name longer than {} bytes",
        limits().max_path_bytes
    );
    let mut bytes = Vec::new();
    entry.take(most).read_to_end(&mut bytes)?;
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    let text =
        String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("an entry's path isn't UTF-8"))?;
    Ok(text.trim_end_matches('/').to_string())
}

/// An archive's `archive.toml` alone: its first entry, read within its
/// limit, nothing else read.
pub fn read_manifest(archive: &Path) -> anyhow::Result<Manifest> {
    let file =
        std::fs::File::open(archive).with_context(|| format!("reading {}", archive.display()))?;
    let mut tar = tar::Archive::new(decoded(file));
    let mut entries = tar
        .entries()
        .with_context(|| format!("reading {}", archive.display()))?
        .raw(true);
    let entry = entries
        .next()
        .with_context(|| format!("{} is empty", archive.display()))?
        .with_context(|| format!("reading {}", archive.display()))?;
    manifest_of(entry).with_context(|| format!("in {}", archive.display()))
}

fn manifest_of<R: Read>(mut entry: tar::Entry<R>) -> anyhow::Result<Manifest> {
    let limits = limits();
    let path = entry_path(&entry)?;
    ensure!(
        path == limits.manifest && entry.header().entry_type() == tar::EntryType::Regular,
        "its first entry is {path}, not {}: it isn't an archive",
        limits.manifest
    );
    ensure!(
        entry.size() <= limits.max_manifest_bytes,
        "{} is larger than {} bytes",
        limits.manifest,
        limits.max_manifest_bytes
    );
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .with_context(|| format!("reading {}", limits.manifest))?;
    link_files::saved::parse(&text).with_context(|| format!("reading {}", limits.manifest))
}

fn entry_path<R: Read>(entry: &tar::Entry<R>) -> anyhow::Result<String> {
    let bytes = entry.path_bytes();
    let path = std::str::from_utf8(&bytes)
        .map_err(|_| anyhow::anyhow!("an entry's path isn't UTF-8"))?
        .trim_end_matches('/')
        .to_string();
    check_path(&path).with_context(|| format!("entry {:?}", link_format::clean(&path)))?;
    Ok(path)
}

/// Reads archive `archive` whole into a new folder in `staging`, checking
/// every entry as it goes: what it holds is staged only when all of it
/// is admitted, and its things' folders are all there.
pub fn stage(archive: &Path, staging: &Path) -> anyhow::Result<Staged> {
    let limits = limits();
    link_files::create_private_dir(staging)
        .with_context(|| format!("creating {}", staging.display()))?;
    let root = staging.join(link_files::layout::temporary_name());
    link_files::create_private_dir(&root)
        .with_context(|| format!("creating {}", root.display()))?;
    let mut staged = Staged {
        root: root.clone(),
        manifest: Manifest::default(),
        things: Vec::new(),
    };
    let budget = limits.max_bytes.min(free_bytes(&root)?);
    let file =
        std::fs::File::open(archive).with_context(|| format!("reading {}", archive.display()))?;
    let mut tar = tar::Archive::new(decoded(file));
    let mut entries = tar
        .entries()
        .with_context(|| format!("reading {}", archive.display()))?
        .raw(true);
    let within = || format!("in {}", archive.display());
    let first = entries
        .next()
        .with_context(|| format!("{} is empty", archive.display()))?
        .with_context(within)?;
    staged.manifest = manifest_of(first).with_context(within)?;
    let plan = Plan::of(&staged.manifest).with_context(within)?;
    let mut seen = BTreeSet::from([limits.manifest.clone()]);
    let (mut count, mut bytes) = (1u64, 0u64);
    let mut long: Option<String> = None;
    for entry in entries {
        let mut entry = entry.with_context(within)?;
        count += 1;
        ensure!(
            count <= limits.max_entries,
            "{} holds more than {} entries",
            archive.display(),
            limits.max_entries
        );
        let kind = entry.header().entry_type();
        if kind == tar::EntryType::GNULongName {
            ensure!(
                long.is_none(),
                "{} holds two long names for one entry",
                archive.display()
            );
            long = Some(long_name(&mut entry).with_context(within)?);
            continue;
        }
        let path = match long.take() {
            Some(path) => {
                check_path(&path)
                    .with_context(|| format!("entry {:?}", link_format::clean(&path)))
                    .with_context(within)?;
                path
            }
            None => entry_path(&entry).with_context(within)?,
        };
        let dir = match kind {
            tar::EntryType::Regular | tar::EntryType::Continuous => false,
            tar::EntryType::Directory => true,
            other => bail!(
                "{} holds {path}, a {}, which an archive never does: only plain files and folders",
                archive.display(),
                describe(other)
            ),
        };
        ensure!(
            seen.insert(path.clone()),
            "{} holds {path} twice",
            archive.display()
        );
        let root_file = !dir && limits.root_files.contains(&path);
        ensure!(
            root_file || plan.admits(&path, dir),
            "{} holds {path}, which isn't in a thing archive.toml names",
            archive.display()
        );
        let to = root.join(&path);
        if dir {
            make_dirs(&to)?;
            continue;
        }
        bytes = bytes.saturating_add(entry.size());
        ensure!(
            bytes <= budget,
            "{} holds more than {budget} bytes, the most it may hold here, or the space free",
            archive.display()
        );
        if let Some(parent) = to.parent() {
            make_dirs(parent)?;
        }
        let mut out = create_new_private(&to)?;
        let size = entry.size();
        let copied = std::io::copy(&mut (&mut entry).take(size), &mut out)
            .with_context(|| format!("staging {path}"))?;
        ensure!(copied == size, "{} ends inside {path}", archive.display());
    }
    for (selection, owner, folders) in plan.things {
        let mut staged_folders = Vec::new();
        for (part, home, at) in folders {
            let folder = root.join(&at);
            ensure!(
                folder.is_dir(),
                "{} names {selection}, but doesn't hold its folder {at}",
                archive.display()
            );
            staged_folders.push(Folder {
                part,
                home,
                staged: folder,
            });
        }
        staged.things.push(StagedThing {
            selection,
            owner,
            folders: staged_folders,
        });
    }
    Ok(staged)
}

fn describe(kind: tar::EntryType) -> &'static str {
    match kind {
        tar::EntryType::Symlink => "symbolic link",
        tar::EntryType::Link => "hard link",
        tar::EntryType::Char | tar::EntryType::Block => "device",
        tar::EntryType::Fifo => "named pipe",
        _ => "special entry",
    }
}

/// Folder `path` and those leading to it, each only this user's.
pub(crate) fn make_dirs(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .with_context(|| format!("creating {}", path.display()))
}

/// A new file only this user reads, never one already there.
fn create_new_private(path: &Path) -> anyhow::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))
}

impl Staged {
    /// The staged thing selected as `selection`.
    pub fn thing(&self, selection: &Selection) -> Option<&StagedThing> {
        self.things.iter().find(|t| &t.selection == selection)
    }
}

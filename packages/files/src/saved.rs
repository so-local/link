//! Files Link saves: settings, state, grants, approvals, ledgers and the
//! map. Each names its schema, declared by its owner in a `schema.toml`
//! beside its type ([`crate::specification!`]), so a later release
//! advances an older file through the owner's steps and never invalidates
//! it, and one written by a newer release is refused, naming the upgrade.
//! A file with no `schema` is refused: Link wrote none of those.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::schema::Specification;

/// The saved file at `path`, read as control, its schema advanced to the
/// current one. Missing, it's the type's default.
pub fn read<T: Specification + Default>(path: &Path) -> anyhow::Result<T> {
    Ok(read_optional(path)?.unwrap_or_default())
}

/// The saved file at `path`, read as control, or `None` when it's missing.
pub fn read_optional<T: Specification>(path: &Path) -> anyhow::Result<Option<T>> {
    let Some(text) = crate::settings::text_optional(path)? else {
        return Ok(None);
    };
    parse(&text)
        .map(Some)
        .with_context(|| format!("reading {}", path.display()))
}

/// A saved file's text, its schema advanced.
pub fn parse<T: Specification>(text: &str) -> anyhow::Result<T> {
    Ok(T::format().read_saved(text)?.spec)
}

/// A saved file's text advanced to the current schema, without its
/// `schema`: for an owner that parses the body itself, as one whose errors
/// mustn't show its values does.
pub fn advance<T: Specification>(text: &str) -> anyhow::Result<String> {
    let (mut doc, _) = T::format().document(text)?;
    doc.remove("schema");
    Ok(doc.to_string())
}

/// `value` as its file's text, naming its schema.
pub fn print<T: Specification>(value: &T) -> anyhow::Result<String> {
    T::format().print(value)
}

/// Write `value` to `path` whole, readable by this user alone, naming its
/// schema, under the file's writer lock ([`lock`]), so an upgrade
/// converting it meanwhile is waited for. A writer already holding the
/// lock writes through it ([`Held::write`]).
pub fn write<T: Specification>(path: &Path, value: &T) -> anyhow::Result<()> {
    let _held = writer(path)?;
    write_private(path, value)
}

/// Append `value` to the line file at `path` as one line ([`line`]),
/// under the file's writer lock ([`lock`]).
pub fn append<T: Specification>(path: &Path, value: &T) -> anyhow::Result<()> {
    use std::io::Write;
    let line = line(value)?;
    let _held = writer(path)?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(&line))
        .with_context(|| format!("writing {}", path.display()))
}

fn write_private<T: Specification>(path: &Path, value: &T) -> anyhow::Result<()> {
    crate::write_private(path, print(value)?.as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}

/// The writer lock of the saved file at `path`, held: `<its name>.lock`
/// beside it, which each of its writers holds while it reads, changes
/// and writes the file back, and an upgrade while it converts it.
pub struct Held {
    path: PathBuf,
    _lock: std::fs::File,
}

impl Held {
    /// Write `value` to the file this lock guards, as [`write`] does.
    pub fn write<T: Specification>(&self, value: &T) -> anyhow::Result<()> {
        write_private(&self.path, value)
    }
}

/// The path of the writer lock of the saved file at `path`.
fn lock_path(path: &Path) -> anyhow::Result<PathBuf> {
    let mut name = path
        .file_name()
        .with_context(|| format!("{} names no file", path.display()))?
        .to_os_string();
    name.push(".lock");
    Ok(path.with_file_name(name))
}

/// The writer lock of the saved file at `path`, taken where the file is
/// written: its folder made as [`crate::write_private`] makes it.
fn writer(path: &Path) -> anyhow::Result<std::fs::File> {
    let lock = lock_path(path)?;
    if let Some(dir) = lock.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        crate::create_dirs_private(dir).with_context(|| format!("making {}", dir.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    let file = options
        .open(&lock)
        .with_context(|| format!("opening {}", lock.display()))?;
    crate::fs::lock(&file).with_context(|| format!("locking {}", lock.display()))?;
    Ok(file)
}

/// The writer lock of the saved file at `path`, held while one reads,
/// changes and writes the file back ([`Held::write`]): the one
/// [`write`] and [`append`] take, and [`crate::settings::lock`] for a
/// TOML file. Its folders are checked as control first.
pub fn lock(path: &Path) -> anyhow::Result<Held> {
    // SAFETY: geteuid has no failure and touches no memory.
    let owners = [0, unsafe { libc::geteuid() }];
    crate::fs::check_folders(path, crate::fs::Trust::control(&owners))?;
    Ok(Held {
        path: path.to_path_buf(),
        _lock: crate::lock_file(&lock_path(path)?)?,
    })
}

/// A saved JSON file's bytes, its schema advanced.
pub fn parse_json<T: Specification>(bytes: &[u8]) -> anyhow::Result<T> {
    Ok(T::format().read_json(bytes)?.spec)
}

/// `value` as a saved JSON file's bytes, naming its schema.
pub fn print_json<T: Specification>(value: &T) -> anyhow::Result<Vec<u8>> {
    T::format().print_json(value)
}

/// `value` as one line of a line ledger: its JSON, naming its schema, and
/// a newline. Every line of an append-only file names its own schema, so
/// a ledger written across an upgrade holds lines of two schemas, each
/// advanced as it's read ([`parse_line`]).
pub fn line<T: Specification>(value: &T) -> anyhow::Result<Vec<u8>> {
    let mut line = print_json(value)?;
    line.push(b'\n');
    Ok(line)
}

/// One line of a line ledger, without its newline, its schema advanced.
pub fn parse_line<T: Specification>(line: &[u8]) -> anyhow::Result<T> {
    parse_json(line)
}

#[cfg(test)]
mod tests;

//! A bucket's objects, each with its version: the [`Store`] interface,
//! a write refused from a version it isn't ([`Conflict`]).

use super::*;

/// An object's version, as its store gives it: opaque, compared whole.
pub type Version = String;

/// What a write expects of the object it replaces.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Expect {
    /// That there's none: a new file.
    Absent,
    /// That it's still this version, the one read.
    Version(Version),
    /// Nothing: whatever is there is replaced. For objects named by what
    /// they hold, which are the same whoever writes them.
    Any,
}

/// A write refused: the object isn't what it expected. Read it again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub name: String,
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} changed since it was read: read it again, then write",
            self.name
        )
    }
}

impl std::error::Error for Conflict {}

/// An object as a listing gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Object {
    /// Its name, from the prefix listed.
    pub path: String,
    pub size: u64,
    pub version: Version,
    /// When it was last written, in nanoseconds since 1970; 0 when its
    /// store doesn't say.
    #[serde(default)]
    pub modified: i64,
}

/// A shared prefix's capacity, enforced atomically with writes by its store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capacity {
    pub prefix: String,
    pub bytes: u64,
}

impl Capacity {
    pub fn check(&self, name: &str) -> anyhow::Result<()> {
        check_name(self.prefix.trim_end_matches('/'))?;
        ensure!(
            self.prefix.ends_with('/') && name.starts_with(&self.prefix),
            "an object's capacity names its folder prefix"
        );
        Ok(())
    }
}

/// A bucket's objects, each with a version.
pub trait Store: Send + Sync {
    /// A store must supply shared admission, never an unlocked listing or
    /// a process-local mutex. Unsupported stores refuse before mutation.
    fn put_bounded(
        &self,
        _name: &str,
        _data: &[u8],
        _expect: &Expect,
        _capacity: &Capacity,
    ) -> anyhow::Result<Version> {
        anyhow::bail!("this store cannot enforce a shared object capacity")
    }
    fn delete_bounded(
        &self,
        _name: &str,
        _expect: &Expect,
        _capacity: &Capacity,
    ) -> anyhow::Result<()> {
        anyhow::bail!("this store cannot enforce a shared object capacity")
    }
    /// Copies an object to `out` with bounded buffering. Large-object drivers
    /// override this; the default supports stores of small bridge objects.
    fn get_to(&self, name: &str, out: &mut dyn std::io::Write) -> anyhow::Result<Option<Version>> {
        let Some((bytes, version)) = self.get(name)? else {
            return Ok(None);
        };
        out.write_all(&bytes)?;
        Ok(Some(version))
    }
    /// Writes a seekable file without holding it in memory. `file` starts at 0.
    /// The default supports stores of small bridge objects.
    fn put_file(
        &self,
        name: &str,
        file: &std::fs::File,
        expect: &Expect,
    ) -> anyhow::Result<Version> {
        use std::io::{Read, Seek};
        let mut file = file.try_clone()?;
        file.rewind()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        self.put(name, &bytes, expect)
    }

    /// Object `name`'s bytes and version; none if there's none.
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>>;
    /// Writes object `name` if it's as `expect` says: its new version,
    /// else a [`Conflict`] (as the error, `downcast_ref`).
    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version>;
    /// Removes object `name` if it's as `expect` says; nothing if there's
    /// none.
    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()>;
    /// Every object whose name starts with `prefix` (a folder's, ending
    /// `/`, or empty for all), names whole.
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>>;
    /// Object `name`'s size; none if there's none. Listed by its whole
    /// name, so what's beside it in its folder is never read; a store
    /// that knows a size more cheaply says so.
    fn size(&self, name: &str) -> anyhow::Result<Option<u64>> {
        Ok(self
            .list(name)?
            .into_iter()
            .find(|o| o.path == name)
            .map(|o| o.size))
    }
}

/// Temporary disk space for a large object, private and unlinked on open.
pub fn spool() -> std::io::Result<std::fs::File> {
    link_files::nofollow::temporary(&link_files::layout::Hidden::TransferScratch.path())
}

/// Whether `err` is a write refused for its version.
pub fn is_conflict(err: &anyhow::Error) -> bool {
    err.downcast_ref::<Conflict>().is_some()
}

/// An object's name: parts between `/`, none empty, `.` or `..`, no
/// NUL, at most 1024 bytes.
pub fn check_name(name: &str) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 1024
            && !name.contains('\0')
            && name
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."),
        "{:?} isn't an object's name",
        link_format::clean(name)
    );
    Ok(())
}

impl<T: Store + ?Sized> Store for Arc<T> {
    fn put_bounded(
        &self,
        name: &str,
        data: &[u8],
        expect: &Expect,
        capacity: &Capacity,
    ) -> anyhow::Result<Version> {
        (**self).put_bounded(name, data, expect, capacity)
    }
    fn delete_bounded(
        &self,
        name: &str,
        expect: &Expect,
        capacity: &Capacity,
    ) -> anyhow::Result<()> {
        (**self).delete_bounded(name, expect, capacity)
    }
    fn get_to(&self, name: &str, out: &mut dyn std::io::Write) -> anyhow::Result<Option<Version>> {
        (**self).get_to(name, out)
    }
    fn put_file(
        &self,
        name: &str,
        file: &std::fs::File,
        expect: &Expect,
    ) -> anyhow::Result<Version> {
        (**self).put_file(name, file, expect)
    }
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        (**self).get(name)
    }
    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        (**self).put(name, data, expect)
    }
    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        (**self).delete(name, expect)
    }
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        (**self).list(prefix)
    }
    fn size(&self, name: &str) -> anyhow::Result<Option<u64>> {
        (**self).size(name)
    }
}

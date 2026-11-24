//! Objects kept in a folder ([`DirStore`]): a stand-in, and a machine's.

use super::*;
mod capacity;

/// Objects in a folder: object `a/b` is the file `<root>/objects/a/b`,
/// its version the SHA-256 of what it holds. Each write goes beside it
/// first and is moved in place under a lock on the folder, so a writer
/// in another process sees it whole and in order. What a machine keeps,
/// and the tests' bucket.
#[cfg(feature = "files-bridge")]
#[derive(Clone, Debug)]
pub struct DirStore {
    root: PathBuf,
}

#[cfg(feature = "files-bridge")]
impl DirStore {
    /// The store in `root`, made if it's missing.
    pub fn new(root: &Path) -> anyhow::Result<Self> {
        for dir in ["objects", "tmp"] {
            link_files::create_private_dir(&root.join(dir))?;
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    fn path(&self, name: &str) -> anyhow::Result<PathBuf> {
        check_name(name)?;
        let path = self.root.join("objects").join(name);
        // No link on the way: the folder is the store's alone.
        let mut at = self.root.join("objects");
        for part in name.split('/') {
            at.push(part);
            if let Ok(meta) = std::fs::symlink_metadata(&at) {
                ensure!(!meta.file_type().is_symlink(), "{name} crosses a link");
            }
        }
        Ok(path)
    }

    /// The store's lock, held while its guard lives.
    fn lock(&self) -> anyhow::Result<std::fs::File> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join("lock"))?;
        use std::os::fd::AsRawFd;
        // SAFETY: flock on a descriptor this function owns.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        ensure!(locked == 0, "locking {}", self.root.display());
        Ok(file)
    }

    fn version_of(path: &Path) -> Option<Version> {
        link_digest::sha256_of(std::fs::File::open(path).ok()?).ok()
    }

    fn expected(name: &str, now: Option<&Version>, expect: &Expect) -> anyhow::Result<()> {
        let fine = match expect {
            Expect::Any => true,
            Expect::Absent => now.is_none(),
            Expect::Version(v) => now == Some(v),
        };
        match fine {
            true => Ok(()),
            false => Err(Conflict { name: name.into() }.into()),
        }
    }
}

/// The version [`DirStore`] gives `bytes`: their [`sha256`].
#[cfg(feature = "files-bridge")]
pub(super) fn version(bytes: &[u8]) -> Version {
    sha256(bytes)
}

#[cfg(feature = "files-bridge")]
impl Store for DirStore {
    fn get_to(&self, name: &str, out: &mut dyn std::io::Write) -> anyhow::Result<Option<Version>> {
        use std::io::Seek;
        let mut file = match std::fs::File::open(self.path(name)?) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let version = link_digest::sha256_of(&mut file)?;
        file.rewind()?;
        std::io::copy(&mut file, out)?;
        Ok(Some(version))
    }
    fn put_file(
        &self,
        name: &str,
        file: &std::fs::File,
        expect: &Expect,
    ) -> anyhow::Result<Version> {
        use std::io::Seek;
        let path = self.path(name)?;
        let _locked = self.lock()?;
        Self::expected(name, Self::version_of(&path).as_ref(), expect)?;
        let expected_size = file.metadata()?.len();
        let capacity = self.begin_capacity(name, Some(expected_size), None)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut source = file.try_clone()?;
        source.rewind()?;
        // The named copy is private and removed even when the write fails.
        let name = self.root.join("tmp").join(link_files::random_token()?);
        let result = (|| -> anyhow::Result<Version> {
            use std::os::unix::fs::OpenOptionsExt;
            let mut target = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&name)?;
            std::io::copy(&mut source, &mut target)?;
            ensure!(
                target.metadata()?.len() == expected_size,
                "the source file changed while it was copied"
            );
            target.rewind()?;
            let version = link_digest::sha256_of(&mut target)?;
            std::fs::rename(&name, &path)?;
            Ok(version)
        })();
        let _ = std::fs::remove_file(&name);
        result.and_then(|version| {
            self.commit_capacity(capacity, &path)?;
            Ok(version)
        })
    }

    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        let path = self.path(name)?;
        match std::fs::read(&path) {
            Ok(bytes) => {
                let v = version(&bytes);
                Ok(Some((bytes, v)))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("reading {name}")),
        }
    }

    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        self.put_with_capacity(name, data, expect, None)
    }

    fn put_bounded(
        &self,
        name: &str,
        data: &[u8],
        expect: &Expect,
        capacity: &Capacity,
    ) -> anyhow::Result<Version> {
        self.put_with_capacity(name, data, expect, Some(capacity))
    }

    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        self.delete_with_capacity(name, expect, None)
    }

    fn delete_bounded(
        &self,
        name: &str,
        expect: &Expect,
        capacity: &Capacity,
    ) -> anyhow::Result<()> {
        self.delete_with_capacity(name, expect, Some(capacity))
    }

    fn size(&self, name: &str) -> anyhow::Result<Option<u64>> {
        match std::fs::symlink_metadata(self.path(name)?) {
            Ok(meta) if meta.is_file() => Ok(Some(meta.len())),
            Ok(_) => Ok(None),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(anyhow::Error::new(err).context(format!("reading {name}"))),
        }
    }

    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        self.list_objects(prefix)
    }
}

#[cfg(feature = "files-bridge")]
impl DirStore {
    fn put_with_capacity(
        &self,
        name: &str,
        data: &[u8],
        expect: &Expect,
        requested: Option<&Capacity>,
    ) -> anyhow::Result<Version> {
        let path = self.path(name)?;
        let _locked = self.lock()?;
        Self::expected(name, Self::version_of(&path).as_ref(), expect)?;
        let capacity = self.begin_capacity(name, Some(data.len() as u64), requested)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let aside = self.root.join("tmp").join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&aside, data).with_context(|| format!("writing {name}"))?;
        std::fs::rename(&aside, &path).inspect_err(|_| {
            let _ = std::fs::remove_file(&aside);
        })?;
        self.commit_capacity(capacity, &path)?;
        Ok(version(data))
    }

    fn delete_with_capacity(
        &self,
        name: &str,
        expect: &Expect,
        requested: Option<&Capacity>,
    ) -> anyhow::Result<()> {
        if let Some(capacity) = requested {
            capacity.check(name)?;
        }
        let path = self.path(name)?;
        let _locked = self.lock()?;
        let now = Self::version_of(&path);
        if now.is_none() {
            return Ok(());
        }
        Self::expected(name, now.as_ref(), expect)?;
        let capacity = self.begin_capacity(name, None, requested)?;
        std::fs::remove_file(&path).with_context(|| format!("removing {name}"))?;
        self.commit_capacity(capacity, &path)?;
        // Its folders, once empty, go too: a bucket has none.
        let objects = self.root.join("objects");
        let mut dir = path.parent();
        while let Some(at) = dir.filter(|d| *d != objects) {
            if std::fs::remove_dir(at).is_err() {
                break;
            }
            dir = at.parent();
        }
        Ok(())
    }

    fn list_objects(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        let objects = self.root.join("objects");
        let mut found = Vec::new();
        let mut folders = vec![objects.clone()];
        while let Some(dir) = folders.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let path = entry.path();
                let name = path.strip_prefix(&objects)?.to_string_lossy().into_owned();
                if kind.is_dir() {
                    // Only folders the prefix may be in.
                    let folder = format!("{name}/");
                    if folder.starts_with(prefix) || prefix.starts_with(&folder) {
                        folders.push(path);
                    }
                } else if kind.is_file() && name.starts_with(prefix) {
                    let bytes = std::fs::read(&path)?;
                    let modified = entry
                        .metadata()?
                        .modified()?
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_nanos() as i64);
                    found.push(Object {
                        path: name,
                        size: bytes.len() as u64,
                        version: version(&bytes),
                        modified,
                    });
                    ensure!(found.len() <= LIST_MAX, "more than {LIST_MAX} objects");
                }
            }
        }
        found.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_test_kit::TempDir;

    #[test]
    fn a_store_refuses_a_write_from_a_version_it_isnt() {
        let dir = TempDir::new("files-store");
        let store = DirStore::new(&dir.join("s")).unwrap();
        let first = store.put("a/b", b"one", &Expect::Absent).unwrap();
        assert!(is_conflict(
            &store.put("a/b", b"two", &Expect::Absent).unwrap_err()
        ));
        let second = store
            .put("a/b", b"two", &Expect::Version(first.clone()))
            .unwrap();
        // Two writers from the first version: the later is refused.
        assert!(is_conflict(
            &store
                .put("a/b", b"three", &Expect::Version(first))
                .unwrap_err()
        ));
        assert_eq!(
            store.get("a/b").unwrap().unwrap(),
            (b"two".to_vec(), second)
        );
        assert!(store.get("../x").is_err());
        assert_eq!(store.list("a/").unwrap().len(), 1);
        assert!(store.list("b/").unwrap().is_empty());
        store.delete("a/b", &Expect::Any).unwrap();
        assert!(store.get("a/b").unwrap().is_none());
        assert!(!dir.join("s/objects/a").exists());
    }
}

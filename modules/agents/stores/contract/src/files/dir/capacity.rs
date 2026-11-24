//! Prefix counts live beside objects under the directory store's cross-process
//! lock. A dirty record precedes every mutation; recovery recounts it once.

use super::*;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Counts {
    dirty: bool,
    prefixes: BTreeMap<String, Count>,
}

link_files::specification!(Counts, include_str!("capacity.schema.toml"));

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Count {
    used: u64,
    most: u64,
}

impl DirStore {
    /// The caller holds this store's flock through the following mutation.
    pub(super) fn begin_capacity(
        &self,
        name: &str,
        next: Option<u64>,
        requested: Option<&Capacity>,
    ) -> anyhow::Result<Option<Counts>> {
        let file = self.root.join(crate::constants::CAPACITY_FILE);
        let mut counts: Counts = match std::fs::read(file) {
            Ok(data) => link_files::saved::parse_json(&data)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Counts::default(),
            Err(error) => return Err(error.into()),
        };
        if counts.dirty {
            for (prefix, count) in &mut counts.prefixes {
                count.used = self.count_prefix(prefix)?;
            }
            counts.dirty = false;
            self.save_counts(&counts)?;
        }
        if let Some(capacity) = requested {
            capacity.check(name)?;
            let changed = counts
                .prefixes
                .get(&capacity.prefix)
                .is_none_or(|count| count.most != capacity.bytes);
            if let Some(count) = counts.prefixes.get_mut(&capacity.prefix) {
                count.most = capacity.bytes;
            } else {
                counts.prefixes.insert(
                    capacity.prefix.clone(),
                    Count {
                        used: self.count_prefix(&capacity.prefix)?,
                        most: capacity.bytes,
                    },
                );
            }
            if changed {
                self.save_counts(&counts)?;
            }
        }
        if !counts
            .prefixes
            .keys()
            .any(|prefix| name.starts_with(prefix))
        {
            return Ok(None);
        }
        let before = self.size(name)?.unwrap_or(0);
        let after = next.unwrap_or(0);
        for (prefix, count) in &mut counts.prefixes {
            if name.starts_with(prefix) {
                let used = count
                    .used
                    .checked_sub(before)
                    .and_then(|used| used.checked_add(after))
                    .context("the directory store's capacity inventory is inconsistent")?;
                ensure!(
                    after <= before || used <= count.most,
                    "the shared object capacity is used: holds at most {} bytes",
                    count.most
                );
                count.used = used;
            }
        }
        counts.dirty = true;
        self.save_counts(&counts)?;
        Ok(Some(counts))
    }

    pub(super) fn commit_capacity(
        &self,
        counts: Option<Counts>,
        object: &Path,
    ) -> anyhow::Result<()> {
        let Some(mut counts) = counts else {
            return Ok(());
        };
        if object.exists() {
            std::fs::File::open(object)?.sync_all()?;
        }
        if let Some(parent) = object.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        counts.dirty = false;
        self.save_counts(&counts)
    }

    fn count_prefix(&self, prefix: &str) -> anyhow::Result<u64> {
        let root = self.root.join("objects");
        let mut folders = vec![root.clone()];
        let mut used = 0u64;
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(folder)? {
                let entry = entry?;
                let path = entry.path();
                let name = path
                    .strip_prefix(&root)?
                    .to_str()
                    .context("an object name is not UTF-8")?;
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    let at = format!("{name}/");
                    if at.starts_with(prefix) || prefix.starts_with(&at) {
                        folders.push(path);
                    }
                } else if name.starts_with(prefix) {
                    ensure!(kind.is_file(), "a bounded object is not a regular file");
                    used = used
                        .checked_add(entry.metadata()?.len())
                        .context("the directory store's capacity exceeds u64")?;
                }
            }
        }
        Ok(used)
    }

    fn save_counts(&self, counts: &Counts) -> anyhow::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let aside = self.root.join("tmp").join(link_files::random_token()?);
        let result = (|| -> anyhow::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&aside)?;
            file.write_all(&link_files::saved::print_json(counts)?)?;
            file.sync_all()?;
            std::fs::rename(&aside, self.root.join(crate::constants::CAPACITY_FILE))?;
            std::fs::File::open(&self.root)?.sync_all()?;
            Ok(())
        })();
        let _ = std::fs::remove_file(aside);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separate_directory_handles_share_capacity_and_replacement_releases_only_its_delta() {
        let dir = link_test_kit::TempDir::new("shared-directory-capacity");
        let a = DirStore::new(&dir).unwrap();
        let b = DirStore::new(&dir).unwrap();
        let capacity = Capacity {
            prefix: "index/".into(),
            bytes: 10,
        };
        let results = std::thread::scope(|threads| {
            let a =
                threads.spawn(|| a.put_bounded("index/a", b"123456", &Expect::Absent, &capacity));
            let b =
                threads.spawn(|| b.put_bounded("index/b", b"123456", &Expect::Absent, &capacity));
            [a.join().unwrap(), b.join().unwrap()]
        });
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let store = DirStore::new(&dir).unwrap();
        let listed = store.list("index/").unwrap();
        assert_eq!(listed.len(), 1);
        let object = &listed[0];
        assert!(store.put("index/extra", b"12345", &Expect::Absent).is_err());
        let current = store
            .put_bounded(
                &object.path,
                b"123",
                &Expect::Version(object.version.clone()),
                &capacity,
            )
            .unwrap();
        assert!(
            store
                .put_bounded(
                    &object.path,
                    b"12345678",
                    &Expect::Version(object.version.clone()),
                    &capacity
                )
                .is_err()
        );
        store
            .put_bounded("index/extra", b"1234567", &Expect::Absent, &capacity)
            .unwrap();
        assert!(
            store
                .put_bounded("index/full", b"1", &Expect::Absent, &capacity)
                .is_err()
        );
        store
            .delete_bounded(&object.path, &Expect::Version(current), &capacity)
            .unwrap();
        store
            .put_bounded("index/full", b"123", &Expect::Absent, &capacity)
            .unwrap();
        assert_eq!(
            store
                .list("index/")
                .unwrap()
                .iter()
                .map(|object| object.size)
                .sum::<u64>(),
            10
        );
    }

    #[test]
    fn an_interrupted_directory_write_recounts_before_admitting_more_bytes() {
        let dir = link_test_kit::TempDir::new("dirty-directory-capacity");
        let store = DirStore::new(&dir).unwrap();
        let capacity = Capacity {
            prefix: "index/".into(),
            bytes: 10,
        };
        store
            .put_bounded("index/a", b"123456", &Expect::Absent, &capacity)
            .unwrap();
        let record = dir.join(crate::constants::CAPACITY_FILE);
        let mut counts: Counts =
            link_files::saved::parse_json(&std::fs::read(&record).unwrap()).unwrap();
        counts.dirty = true;
        // Fault injection: the object committed after the dirty record but the
        // process did not save its completed count.
        std::fs::write(&record, link_files::saved::print_json(&counts).unwrap()).unwrap();
        std::fs::write(dir.join("objects/index/b"), b"1234").unwrap();
        let reopened = DirStore::new(&dir).unwrap();
        assert!(
            reopened
                .put_bounded("index/c", b"1", &Expect::Absent, &capacity)
                .is_err()
        );
        reopened
            .delete_bounded("index/b", &Expect::Any, &capacity)
            .unwrap();
        reopened
            .put_bounded("index/c", b"1234", &Expect::Absent, &capacity)
            .unwrap();
        let counts: Counts =
            link_files::saved::parse_json(&std::fs::read(record).unwrap()).unwrap();
        assert!(!counts.dirty);
        assert_eq!(counts.prefixes["index/"].used, 10);
    }
}

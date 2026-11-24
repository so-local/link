//! An agent's `memory` collection as a bucket of versioned objects
//! ([`crate::files::Store`]), through the store contract's files: what a
//! memory engine keeps its index's copy in ([`crate::files::index`]:
//! segments named by their SHA-256 and a manifest written last) wherever
//! the agent's machine keeps its memory, through the storage bridge.
//!
//! Object `index/<engine>/<name>` is the agent's file
//! `memory/<engine>/<name>`. An object's version is its file's, as a
//! number in decimal: a write that names one is the file's write at that
//! version, so of two writers of one copy, one writes.

use anyhow::{Context, bail};

use super::{Agent, Store, Written};
use crate::files::{Conflict, Expect, Object, Version};

/// The agent's `memory` collection, through `store`.
pub struct MemoryFiles<S> {
    store: S,
    agent: Agent,
}

/// What an object's name is a file of: `index/<rest>` is
/// `memory/<rest>`, and nothing else is.
fn path_of(name: &str) -> anyhow::Result<String> {
    crate::files::check_name(name)?;
    let rest = name
        .strip_prefix(crate::files::INDEX)
        .with_context(|| format!("{} isn't an index's object", link_format::clean(name)))?;
    Ok(format!("memory/{rest}"))
}

fn version(n: u64) -> Version {
    n.to_string()
}

fn number(version: &Version) -> anyhow::Result<u64> {
    version
        .parse()
        .with_context(|| format!("{:?} isn't a file's version", link_format::clean(version)))
}

impl<S: Store> MemoryFiles<S> {
    /// `agent`'s memory, through `store`.
    pub fn new(store: S, agent: Agent) -> Self {
        Self { store, agent }
    }
}

impl<S: Store + Send + Sync> crate::files::Store for MemoryFiles<S> {
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        Ok(self
            .store
            .file_read(&self.agent, &path_of(name)?)?
            .map(|(bytes, n)| (bytes, version(n))))
    }

    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        let path = path_of(name)?;
        let at = match expect {
            Expect::Absent => 0,
            Expect::Version(v) => number(v)?,
            // Named by what they hold: written whatever is there.
            Expect::Any => {
                for _ in 0..16 {
                    let now = self
                        .store
                        .file_read(&self.agent, &path)?
                        .map_or(0, |(_, n)| n);
                    match self.store.file_write(&self.agent, &path, data, now)? {
                        Written::Version(n) => return Ok(version(n)),
                        Written::Stale(_) => continue,
                    }
                }
                bail!(
                    "{} kept changing as it was written",
                    link_format::clean(name)
                );
            }
        };
        match self.store.file_write(&self.agent, &path, data, at)? {
            Written::Version(n) => Ok(version(n)),
            Written::Stale(_) => Err(Conflict { name: name.into() }.into()),
        }
    }

    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        let path = path_of(name)?;
        let Some((_, now)) = self.store.file_read(&self.agent, &path)? else {
            return Ok(());
        };
        let at = match expect {
            Expect::Any => now,
            Expect::Version(v) => number(v)?,
            Expect::Absent => return Err(Conflict { name: name.into() }.into()),
        };
        match self.store.file_remove(&self.agent, &path, at)? {
            true => Ok(()),
            false => Err(Conflict { name: name.into() }.into()),
        }
    }

    /// Each object under `prefix`, a folder's (`index/<engine>/`): a
    /// listing reads no file, so its size and version are left empty;
    /// [`Self::get`] gives an object's.
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        let folder = match prefix.rfind('/') {
            Some(at) => &prefix[..=at],
            None => bail!("{} isn't a folder of the index", link_format::clean(prefix)),
        };
        let path = path_of(folder.trim_end_matches('/'))?;
        Ok(self
            .store
            .file_list(&self.agent, &format!("{path}/"))?
            .into_iter()
            .filter_map(|file| {
                let rest = file.strip_prefix("memory/")?;
                Some(format!("{}{rest}", crate::files::INDEX))
            })
            .filter(|name| name.starts_with(prefix))
            .map(|path| Object {
                path,
                size: 0,
                version: String::new(),
                modified: 0,
            })
            .collect())
    }
}

#[cfg(all(test, feature = "files-bridge"))]
mod tests {
    use super::*;
    use crate::files::DirStore;
    use crate::store::objects::Objects;
    use crate::store::{Reply, Request};

    struct Here(Objects<DirStore>);

    impl Store for Here {
        fn name(&self) -> &str {
            "objects"
        }

        fn call(&self, request: &Request) -> anyhow::Result<Reply> {
            self.0.answer(request)
        }
    }

    fn agent() -> Agent {
        Agent {
            id: "aaaaaaaaaaaa".into(),
            owner: "me@box".into(),
        }
    }

    /// An index's copy written through the agent's memory, read back
    /// whole, its segments larger than an instruction may be; a writer
    /// behind another refused; and the copy removed.
    #[test]
    fn an_index_copy_goes_through_the_agents_memory() {
        let dir = link_test_kit::TempDir::new("memory-files");
        let store = Here(Objects::new(DirStore::new(&dir).unwrap(), agent()));
        let memory = MemoryFiles::new(store, agent());
        let index: Vec<u8> = (0..crate::files::index::SEGMENT + 1000)
            .map(|i| (i % 251) as u8)
            .collect();
        assert_eq!(
            crate::files::index::read(&memory, "link-memory").unwrap(),
            None
        );
        let first = crate::files::index::write(&memory, "link-memory", &index, None).unwrap();
        let (back, version) = crate::files::index::read(&memory, "link-memory")
            .unwrap()
            .unwrap();
        assert_eq!(back, index);
        assert_eq!(version, first);
        // Behind another writer: refused, never written over unseen.
        let mut changed = index.clone();
        changed[0] ^= 1;
        let second =
            crate::files::index::write(&memory, "link-memory", &changed, Some(&first)).unwrap();
        let behind =
            crate::files::index::write(&memory, "link-memory", &index, Some(&first)).unwrap_err();
        assert!(crate::files::is_conflict(&behind), "{behind:#}");
        assert_eq!(
            crate::files::index::read(&memory, "link-memory")
                .unwrap()
                .unwrap(),
            (changed, second)
        );
        // Only the index's objects are memory's: nothing else is named.
        assert!(crate::files::Store::get(&memory, "home/x").is_err());
        crate::files::index::remove(&memory, "link-memory").unwrap();
        assert_eq!(
            crate::files::index::read(&memory, "link-memory").unwrap(),
            None
        );
    }
}

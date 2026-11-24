//! What `lnk storage` needs from the program that talks to clouds. rclone
//! is the one in use ([`crate::rclone`]); anything that can do these can
//! replace it without a command changing. An owned folder a cloud
//! account keeps is the second engine: its pushes and pulls go through
//! the module that keeps its owner ([`crate::hosted`]), and that account
//! keeps its own key, so none of this is asked of it.

use std::path::Path;
use std::time::Duration;

use crate::clouds::Cloud;

/// How one file here compares with its copy in a cloud.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// Same checksum.
    Same,
    /// No copy.
    Missing,
    /// A copy whose checksum differs.
    Differs,
    /// Couldn't tell: reading or hashing failed.
    Error(String),
}

/// What [`Engine::check`] takes to call a copy the same.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Proof {
    /// The checksum the cloud reports: enough to know what to send.
    Checksum,
    /// Enough to delete the file here on: a checksum only where the
    /// storage computed it from what it holds; a copy whose checksum its
    /// uploader could set is read back.
    ToDelete,
}

/// A copy as its cloud lists it, read from no file: its size, and when
/// the storage wrote it. Only the storage sets that time, so a copy
/// written again since shows another.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Written {
    pub size: u64,
    pub at: String,
}

/// A file in a cloud.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub path: String,
    pub size: u64,
    /// Its storage class, where the cloud has them.
    pub class: Option<String>,
}

/// Options for sending files.
#[derive(Debug, Clone, Default)]
pub struct Upload {
    /// Replace copies that differ; otherwise they're left and reported.
    pub replace: bool,
    /// The storage class to store them in (`DEEP_ARCHIVE`).
    pub class: Option<String>,
    /// More flags for the engine, as the user gave them (`--bwlimit 10M`).
    pub flags: Vec<String>,
}

/// Options for getting files back.
#[derive(Debug, Clone, Default)]
pub struct Download {
    /// Replace files here that differ from the copy; otherwise they're
    /// left and reported.
    pub replace: bool,
    /// More flags for the engine, as the user gave them.
    pub flags: Vec<String>,
}

/// One cloud's part of a push: the files to send it, and how.
#[derive(Debug, Clone)]
pub struct Sending<'a> {
    pub cloud: &'a Cloud,
    pub files: Vec<String>,
    pub upload: Upload,
}

/// How one cloud's part of a push went.
#[derive(Debug)]
pub struct Sent {
    /// The engine's error, when some files didn't make it.
    pub failed: Option<anyhow::Error>,
    /// What the check after found, file by file.
    pub checked: anyhow::Result<Vec<(String, Found)>>,
}

/// Sends `sending`'s files with `engine`, then checks them.
pub fn send_and_check_one<E: Engine + ?Sized>(engine: &E, root: &Path, sending: &Sending) -> Sent {
    let failed = engine
        .upload(sending.cloud, root, &sending.files, &sending.upload)
        .err();
    Sent {
        failed,
        checked: engine.check(sending.cloud, root, &sending.files, Proof::Checksum),
    }
}

pub trait Engine {
    /// Each of `sending`, [`Engine::upload`] then [`Engine::check`], in
    /// its order: one cloud after another here, side by side where an
    /// engine can.
    fn send_and_check(&self, root: &Path, sending: &[Sending]) -> Vec<Sent> {
        sending
            .iter()
            .map(|one| send_and_check_one(self, root, one))
            .collect()
    }

    /// `each(engine, i)` for each of `clouds` (`i` its place in them),
    /// with the engine it asks that cloud with: one after another, or
    /// side by side where an engine can.
    fn each_cloud(&self, clouds: &[Cloud], each: &(dyn Fn(&dyn Engine, usize) + Sync));

    /// Every copy in `cloud` below the folder `under` ("" for the whole
    /// bucket), by its path in the bucket, as [`Written`] says it, in one
    /// listing that reads no file and asks nothing of each. None for a cloud whose
    /// checksums [`Proof::ToDelete`] takes as they are: a check there
    /// reads nothing back, so there is nothing to keep it from repeating.
    fn written(
        &self,
        _cloud: &Cloud,
        _under: &str,
    ) -> anyhow::Result<Option<std::collections::HashMap<String, Written>>> {
        Ok(None)
    }

    /// Of `files` in `cloud`, those with an upload in parts begun and not
    /// yet finished on their name: one such, finished after a check, would
    /// be a new copy with the time it began. None when it can't be told.
    fn pending(
        &self,
        _cloud: &Cloud,
        _files: &[String],
    ) -> anyhow::Result<Option<std::collections::HashSet<String>>> {
        Ok(Some(Default::default()))
    }

    /// Sends `files` (paths in the bucket) from `root` to `cloud`,
    /// skipping those already there with the same checksum. An error
    /// means some didn't make it; which is for [`Engine::check`] to say.
    fn upload(
        &self,
        cloud: &Cloud,
        root: &Path,
        files: &[String],
        options: &Upload,
    ) -> anyhow::Result<()>;

    /// Compares each of `files` in `root` with its copy in `cloud`, by
    /// checksum (or, where the cloud has none, or `proof` asks for more
    /// than its checksum, by reading it back).
    fn check(
        &self,
        cloud: &Cloud,
        root: &Path,
        files: &[String],
        proof: Proof,
    ) -> anyhow::Result<Vec<(String, Found)>>;

    /// Everything in `cloud`, except Link's own objects.
    fn list(&self, cloud: &Cloud) -> anyhow::Result<Vec<Listed>>;

    /// Copies `files` from `cloud` into `root`. Without `replace`, a file
    /// here that differs from the copy is left as it is (and fails).
    fn download(
        &self,
        cloud: &Cloud,
        root: &Path,
        files: &[String],
        options: &Download,
    ) -> anyhow::Result<()>;

    /// Asks `cloud` to bring archived `files` back so they can be read.
    fn restore(&self, cloud: &Cloud, files: &[String]) -> anyhow::Result<()>;

    /// Deletes `files` from `cloud`.
    fn delete(&self, cloud: &Cloud, files: &[String]) -> anyhow::Result<()>;

    /// A URL anyone can download `file` from, signed by `cloud` with its
    /// keys here, that stops working after `expires`.
    fn link(&self, cloud: &Cloud, file: &str, expires: Duration) -> anyhow::Result<String>;
}

/// A cloud in memory, for tests of everything above the engine.
#[cfg(test)]
pub mod fake {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// Path -> (contents, storage class).
    type Files = BTreeMap<String, (Vec<u8>, Option<String>)>;

    #[derive(Default)]
    pub struct Fake {
        /// By cloud name.
        pub clouds: RefCell<BTreeMap<String, Files>>,
        /// Restores asked for.
        pub restored: RefCell<Vec<String>>,
        /// Files in an archive tier that can't be read until restored.
        pub archived: RefCell<Vec<String>>,
        /// Clouds whose checksums their uploader sets, as S3's: they say
        /// what's [`Written`], each write a new time.
        pub uploader: RefCell<Vec<String>>,
        /// Each write's number, by cloud and path.
        pub writes: RefCell<BTreeMap<(String, String), u64>>,
        /// The files each check to delete on was given, by cloud.
        pub to_delete: RefCell<Vec<(String, Vec<String>)>>,
        /// A copy written again while it's checked: (cloud, path).
        pub written_while_checked: RefCell<Option<(String, String)>>,
        /// The clouds [`Engine::written`] listed, in turn.
        pub listed: RefCell<Vec<String>>,
        /// Uploads in parts begun and open, by cloud and path; with
        /// `pending_unknown`, none can be told.
        pub pending: RefCell<Vec<(String, String)>>,
        pub pending_unknown: RefCell<bool>,
    }

    impl Fake {
        pub fn put(&self, cloud: &str, path: &str, contents: &[u8]) {
            self.wrote(cloud, path);
            self.clouds
                .borrow_mut()
                .entry(cloud.into())
                .or_default()
                .insert(path.into(), (contents.to_vec(), None));
        }

        fn wrote(&self, cloud: &str, path: &str) {
            *self
                .writes
                .borrow_mut()
                .entry((cloud.into(), path.into()))
                .or_default() += 1;
        }

        pub fn remove(&self, cloud: &str, path: &str) {
            if let Some(files) = self.clouds.borrow_mut().get_mut(cloud) {
                files.remove(path);
            }
        }

        pub fn has(&self, cloud: &str, path: &str) -> bool {
            self.clouds
                .borrow()
                .get(cloud)
                .is_some_and(|files| files.contains_key(path))
        }
    }

    impl Engine for Fake {
        fn each_cloud(&self, clouds: &[Cloud], each: &(dyn Fn(&dyn Engine, usize) + Sync)) {
            for i in 0..clouds.len() {
                each(self, i);
            }
        }

        fn upload(
            &self,
            cloud: &Cloud,
            root: &Path,
            files: &[String],
            options: &Upload,
        ) -> anyhow::Result<()> {
            let mut clouds = self.clouds.borrow_mut();
            let there = clouds.entry(cloud.name.clone()).or_default();
            let mut failed = Vec::new();
            for file in files {
                let contents = std::fs::read(root.join(file))?;
                match there.get(file) {
                    Some((old, _)) if *old != contents && !options.replace => {
                        failed.push(file.clone());
                    }
                    _ => {
                        there.insert(file.clone(), (contents, options.class.clone()));
                        self.wrote(&cloud.name, file);
                    }
                }
            }
            anyhow::ensure!(failed.is_empty(), "differs: {}", failed.join(", "));
            Ok(())
        }

        fn check(
            &self,
            cloud: &Cloud,
            root: &Path,
            files: &[String],
            proof: Proof,
        ) -> anyhow::Result<Vec<(String, Found)>> {
            if proof == Proof::ToDelete {
                self.to_delete
                    .borrow_mut()
                    .push((cloud.name.clone(), files.to_vec()));
                if let Some((name, path)) = self.written_while_checked.take()
                    && name == cloud.name
                {
                    self.wrote(&name, &path);
                }
            }
            let clouds = self.clouds.borrow();
            let there = clouds.get(&cloud.name);
            Ok(files
                .iter()
                .map(|file| {
                    let here = std::fs::read(root.join(file));
                    let found = match (here, there.and_then(|t| t.get(file))) {
                        (Err(e), _) => Found::Error(e.to_string()),
                        (_, None) => Found::Missing,
                        (Ok(here), Some((copy, _))) if here == *copy => Found::Same,
                        _ => Found::Differs,
                    };
                    (file.clone(), found)
                })
                .collect())
        }

        fn pending(
            &self,
            cloud: &Cloud,
            files: &[String],
        ) -> anyhow::Result<Option<std::collections::HashSet<String>>> {
            if *self.pending_unknown.borrow() {
                return Ok(None);
            }
            let open = self.pending.borrow();
            Ok(Some(
                files
                    .iter()
                    .filter(|f| open.iter().any(|(c, p)| *c == cloud.name && p == *f))
                    .cloned()
                    .collect(),
            ))
        }

        fn written(
            &self,
            cloud: &Cloud,
            under: &str,
        ) -> anyhow::Result<Option<std::collections::HashMap<String, Written>>> {
            if !self.uploader.borrow().contains(&cloud.name) {
                return Ok(None);
            }
            self.listed.borrow_mut().push(cloud.name.clone());
            let writes = self.writes.borrow();
            Ok(Some(
                self.list(cloud)?
                    .into_iter()
                    .filter(|l| under.is_empty() || l.path.starts_with(&format!("{under}/")))
                    .map(|l| {
                        let n = writes
                            .get(&(cloud.name.clone(), l.path.clone()))
                            .copied()
                            .unwrap_or_default();
                        let written = Written {
                            size: l.size,
                            at: format!("write {n}"),
                        };
                        (l.path, written)
                    })
                    .collect(),
            ))
        }

        fn list(&self, cloud: &Cloud) -> anyhow::Result<Vec<Listed>> {
            let clouds = self.clouds.borrow();
            Ok(clouds
                .get(&cloud.name)
                .into_iter()
                .flatten()
                .map(|(path, (contents, class))| Listed {
                    path: path.clone(),
                    size: contents.len() as u64,
                    class: class.clone(),
                })
                .collect())
        }

        fn download(
            &self,
            cloud: &Cloud,
            root: &Path,
            files: &[String],
            options: &Download,
        ) -> anyhow::Result<()> {
            let clouds = self.clouds.borrow();
            let mut failed = Vec::new();
            for file in files {
                if self.archived.borrow().contains(file) {
                    failed.push(file.clone());
                    continue;
                }
                let (contents, _) = clouds
                    .get(&cloud.name)
                    .and_then(|t| t.get(file))
                    .ok_or_else(|| anyhow::anyhow!("{file} isn't in {}", cloud.name))?;
                let path = root.join(file);
                if !options.replace && std::fs::read(&path).is_ok_and(|here| here != *contents) {
                    failed.push(file.clone());
                    continue;
                }
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(path, contents)?;
            }
            anyhow::ensure!(failed.is_empty(), "not copied: {}", failed.join(", "));
            Ok(())
        }

        fn restore(&self, _: &Cloud, files: &[String]) -> anyhow::Result<()> {
            self.restored.borrow_mut().extend(files.iter().cloned());
            Ok(())
        }

        fn delete(&self, cloud: &Cloud, files: &[String]) -> anyhow::Result<()> {
            for file in files {
                self.remove(&cloud.name, file);
            }
            Ok(())
        }

        fn link(&self, cloud: &Cloud, file: &str, expires: Duration) -> anyhow::Result<String> {
            anyhow::ensure!(
                self.has(&cloud.name, file),
                "{file} isn't in {}",
                cloud.name
            );
            Ok(format!(
                "https://{}.example/{file}?expires={}",
                cloud.name,
                expires.as_secs()
            ))
        }
    }
}

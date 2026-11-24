//! The `bucket` place's driver (`lnk-bucket-store contract`, `call`,
//! `serve`):
//! an agent's collections as objects in its bucket, the one `lnk storage`
//! connected for it, encrypted, under `.link/agent/` (which a push, a
//! pull and a clean of its Home never touch). The store contract over
//! them is [`link_store_contract::store::objects`]; this is the bucket it's kept
//! in ([`Bucket`]), read and written through the engine.
//!
//! An object's version is the SHA-256 of what it holds. A write that
//! names a version reads the object first, and writes only if it's still
//! that, under a lock on this machine held for the agent's bucket: of two
//! writers here, one writes. An agent runs on one machine at a time, so
//! its bucket is written from one machine at a time.
//!
use std::path::PathBuf;

use anyhow::Context;
use link_store_contract::files::{Conflict, Expect, Object, Version};
use link_store_contract::store::{self, Agent, Reply, Request};

use link_storage::clouds::{Cloud, Clouds};
use link_storage::rclone::Rclone;

/// Where in an agent's bucket its collections are.
pub const ROOT: &str = ".link/agent/";

/// The most of an object read: an event's most, and room for the rest.
const OBJECT_MAX: u64 = store::EVENT_MAX + (1 << 20);

/// An agent's collections in its bucket, through the engine.
pub struct Bucket {
    /// One call at a time: the engine keeps what it learned between them.
    rclone: std::sync::Mutex<Rclone>,
    /// `<remote>.link/agent/`.
    root: String,
    /// Held for each change of an object, by this machine's writers.
    lock: PathBuf,
}

impl Bucket {
    /// Agent `agent`'s bucket: the first encrypted one `lnk storage`
    /// connected for it. An error when there's none.
    pub fn of(agent: &Agent) -> anyhow::Result<Self> {
        link_machine::machine::check_id(&agent.id)
            .with_context(|| format!("{:?} isn't an agent's ID", link_format::clean(&agent.id)))?;
        let clouds = Clouds::load()?;
        let cloud = Clouds::buckets(&clouds.clouds, &agent.id)
            .into_iter()
            .find(|c| c.encrypted)
            .with_context(|| {
                format!(
                    "agent {} has no encrypted bucket here: lnk storage create connects one",
                    agent.id
                )
            })?;
        Self::at(Rclone::installed()?, &cloud)
    }

    /// `cloud`'s bucket through `rclone`.
    pub fn at(rclone: Rclone, cloud: &Cloud) -> anyhow::Result<Self> {
        anyhow::ensure!(
            cloud.encrypted,
            "{}'s bucket isn't encrypted: an agent's collections are kept only encrypted",
            cloud.name
        );
        let locks = link_storage::files::config_dir()?.join("locks");
        link_files::create_private_dir(&locks)?;
        let whose = format!("{}-{}", cloud.name, cloud.folder);
        Ok(Self {
            rclone: std::sync::Mutex::new(rclone),
            root: format!("{}{ROOT}", cloud.remote()),
            lock: locks.join(whose),
        })
    }

    fn engine(&self) -> std::sync::MutexGuard<'_, Rclone> {
        self.rclone.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn path(&self, name: &str) -> anyhow::Result<String> {
        link_store_contract::files::check_name(name)?;
        Ok(format!("{}{name}", self.root))
    }

    /// This machine's lock on the bucket, held while its guard lives.
    fn locked(&self) -> anyhow::Result<std::fs::File> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.lock)
            .with_context(|| format!("opening {}", self.lock.display()))?;
        // SAFETY: flock on a descriptor this function owns.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        anyhow::ensure!(locked == 0, "locking {}", self.lock.display());
        Ok(file)
    }

    /// An error unless object `name` is as `expect` says.
    fn expected(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        let fine = match expect {
            Expect::Any => true,
            Expect::Absent => self.get_unlocked(name)?.is_none(),
            Expect::Version(v) => self.get_unlocked(name)?.is_some_and(|(_, now)| now == *v),
        };
        match fine {
            true => Ok(()),
            false => Err(Conflict { name: name.into() }.into()),
        }
    }

    fn get_unlocked(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        Ok(self
            .engine()
            .object_get(
                &self.path(name)?,
                OBJECT_MAX.max(store::objects::file_record_max(store::MEMORY_FILE_MAX)?),
            )?
            .map(|bytes| {
                let version = link_digest::sha256(&bytes);
                (bytes, version)
            }))
    }
}

impl link_store_contract::files::Store for Bucket {
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        self.get_unlocked(name)
    }

    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        let path = self.path(name)?;
        let _locked = self.locked()?;
        self.expected(name, expect)?;
        self.engine().object_put(&path, data)?;
        Ok(link_digest::sha256(data))
    }

    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        let path = self.path(name)?;
        let _locked = self.locked()?;
        self.expected(name, expect)?;
        self.engine().object_delete(&path)
    }

    /// Each object under `prefix`, its size: a listing reads no object, so
    /// its version is left empty; [`Self::get`] gives an object's.
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        // Listed from the folder `prefix` is in, then held to it.
        let folder = match prefix.rfind('/') {
            Some(at) => &prefix[..=at],
            None => "",
        };
        if !folder.is_empty() {
            link_store_contract::files::check_name(folder.trim_end_matches('/'))?;
        }
        Ok(self
            .engine()
            .object_list(&format!("{}{folder}", self.root))?
            .into_iter()
            .map(|(path, size)| (format!("{folder}{path}"), size))
            .filter(|(path, _)| path.starts_with(prefix))
            .map(|(path, size)| Object {
                path,
                size,
                version: String::new(),
                modified: 0,
            })
            .collect())
    }
}

/// The driver's answer to `request`: the agent's collections in its
/// bucket.
pub fn answer(request: Request) -> anyhow::Result<Reply> {
    let agent = request.agent().clone();
    let objects = store::objects::Objects::new(Bucket::of(&agent)?, agent);
    objects.answer(&request)
}

/// What `lnk-bucket-store contract` says: it serves, and keeps jobs and files.
pub fn contract() -> store::Contract {
    store::Contract {
        jobs: true,
        files: true,
        ..store::Contract::serving()
    }
}

#[cfg(test)]
mod tests;

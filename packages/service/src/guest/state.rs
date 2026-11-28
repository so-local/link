use super::*;
use std::{fs::File, os::fd::AsRawFd, os::unix::fs::OpenOptionsExt};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub pid: u32,
    pub ticks: u64,
    pub boot: String,
}
impl Identity {
    pub fn of(pid: u32) -> anyhow::Result<Self> {
        Ok(Self {
            pid,
            ticks: crate::started(pid).context("reading service process start identity")?,
            boot: std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?,
        })
    }
    pub fn live(&self) -> anyhow::Result<bool> {
        if self.boot != std::fs::read_to_string("/proc/sys/kernel/random/boot_id")? {
            return Ok(false);
        }
        let stat = match std::fs::read_to_string(format!("/proc/{}/stat", self.pid)) {
            Ok(stat) => stat,
            Err(error) if gone(&error) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let (_, fields) = stat
            .rsplit_once(')')
            .context("malformed service process identity")?;
        let fields: Vec<_> = fields.split_whitespace().collect();
        // An ended process's fields aren't its own any more (its group
        // reads -1).
        if ended(&fields) {
            return Ok(false);
        }
        let ticks: u64 = fields
            .get(19)
            .context("missing process start identity")?
            .parse()?;
        Ok(ticks == self.ticks)
    }
    /// Observation only: a dead guardian never authorizes signalling its stored PID.
    pub fn group_empty(&self) -> anyhow::Result<bool> {
        if self.boot != std::fs::read_to_string("/proc/sys/kernel/random/boot_id")? {
            return Ok(true);
        }
        for entry in std::fs::read_dir("/proc")? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
                continue;
            }
            let text = match std::fs::read_to_string(entry.path().join("stat")) {
                Ok(text) => text,
                Err(error) if gone(&error) => continue,
                Err(error) => return Err(error.into()),
            };
            let (_, rest) = text
                .rsplit_once(')')
                .context("malformed process group identity")?;
            let fields: Vec<_> = rest.split_whitespace().collect();
            if ended(&fields) {
                continue;
            }
            let group: u32 = fields.get(2).context("missing process group")?.parse()?;
            if group == self.pid {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
/// Whether a process's `/proc` stat `fields`, from its state on, are an
/// ended one's: a zombie (`Z`) or dead (`X`).
fn ended(fields: &[&str]) -> bool {
    matches!(fields.first(), Some(&"Z" | &"X"))
}
/// Whether reading a process's `/proc` file failed because it's gone:
/// missing, or ended as it was read (`ESRCH`).
fn gone(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ESRCH)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Desired {
    Running,
    Paused,
    Removed,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub job: Job,
    pub desired: Desired,
    pub revision: u64,
    pub epoch: String,
    pub guardian: Option<Identity>,
    pub worker: Option<Identity>,
    pub main: Option<Identity>,
    pub observed: u64,
    pub runs: u32,
    pub error: Option<String>,
}

link_files::specification!(Record, include_str!("state.schema.toml"));

/// Each guest service's state, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Record>()
}
impl Record {
    pub fn advance(&mut self, desired: Desired) -> anyhow::Result<u64> {
        self.revision = self
            .revision
            .checked_add(1)
            .context("service revision exhausted")?;
        self.desired = desired;
        self.error = None;
        Ok(self.revision)
    }
    pub fn guardian_live(&self) -> anyhow::Result<bool> {
        self.guardian.as_ref().map_or(Ok(false), Identity::live)
    }
    pub fn cleanup_confirmed(&self) -> anyhow::Result<bool> {
        self.worker.as_ref().map_or(Ok(true), Identity::group_empty)
    }
}
#[derive(Clone)]
pub(super) struct Store(pub PathBuf);
impl Store {
    pub fn new(root: &Path, unit: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !unit.is_empty()
                && unit.len() <= 128
                && unit
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                && unit != "."
                && unit != "..",
            "invalid guest service unit"
        );
        let path = link_files::layout::Hidden::Services.at(root).join(unit);
        link_files::create_private_dir(&path)?;
        Ok(Self(path))
    }
    /// The record now, read under the lock its writers hold: a write
    /// replaces the file, and a read across one would see two.
    pub fn read(&self) -> anyhow::Result<Option<Record>> {
        let _lock = self.lock("state.lock", true)?;
        self.load()
    }
    fn load(&self) -> anyhow::Result<Option<Record>> {
        let path = self.0.join("state.json");
        match path.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let uid = unsafe { libc::geteuid() };
        let read = link_files::fs::read_trusted(
            &path,
            link_files::fs::Trust::alone(&[uid]),
            MAX_STATE,
            "a guest service's",
        )?;
        let record: Record = link_files::saved::parse_json(read.text.as_bytes())?;
        Ok(Some(record))
    }
    pub fn update<T>(
        &self,
        change: impl FnOnce(&mut Option<Record>) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let _lock = self.lock("state.lock", true)?;
        let mut record = self.load()?;
        let result = change(&mut record)?;
        if let Some(record) = record {
            let bytes = link_files::saved::print_json(&record)?;
            anyhow::ensure!(
                bytes.len() as u64 <= MAX_STATE,
                "guest service state exceeds its limit"
            );
            link_files::write_private(&self.0.join("state.json"), &bytes)?;
        }
        Ok(result)
    }
    pub fn lock(&self, name: &str, wait: bool) -> anyhow::Result<File> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.0.join(name))?;
        let deadline = Instant::now() + CONTROL_TIMEOUT;
        loop {
            // SAFETY: this live descriptor names the non-replaced private lock file.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(file);
            }
            let error = std::io::Error::last_os_error();
            if !wait
                || error.raw_os_error() != Some(libc::EWOULDBLOCK)
                || Instant::now() >= deadline
            {
                return Err(error.into());
            }
            poll();
        }
    }
}

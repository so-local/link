//! Explicitly selected Linux guest services; no missing-systemd fallback.
use crate::{Job, Status};
use anyhow::{Context, bail};
#[cfg(test)]
use clap::ValueEnum;
use link_secretive::process::OwnedChild;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
mod state;
mod supervisor;
#[cfg(test)]
mod tests;
mod worker;
pub use state::saved;
use state::{Desired, Identity, Record, Store};
const MAX_STATE: u64 = 256 * 1024;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);
const RESTART_DELAY: Duration = Duration::from_secs(10);
use crate::cli::{ENTRY, GuestEntry, Role};

/// The guest service's settings, `/etc/lnk/service.toml`, naming
/// its schema.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    mode: Mode,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Guest,
}
link_files::specification!(Settings, include_str!("guest.schema.toml"));
#[derive(Clone)]
pub(crate) struct Driver {
    root: PathBuf,
    #[cfg(test)]
    fixture: bool,
    #[cfg(test)]
    after_owner_observed: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}
/// Recognizes the private service subprocess entry before a caller parses its CLI.
/// Call this from the program's main thread before starting a runtime or threads.
/// `None` leaves ordinary invocation alone; an invalid guest settings refuses.
pub fn guest_service_entry(given: GuestEntry) -> anyhow::Result<()> {
    let driver = selected()?.context("guest service entry requires explicit guest settings")?;
    driver.entry(given)
}
pub(crate) fn selected() -> anyhow::Result<Option<Driver>> {
    if !crate::managed() {
        return Ok(None);
    }
    let path = if cfg!(debug_assertions) {
        std::env::var_os("LNK_TEST_SERVICE_SETTINGS").map(PathBuf::from)
    } else {
        None
    }
    .unwrap_or_else(|| link_files::layout::System::GuestService.path());
    if !configured(&path, &[0])? {
        return Ok(None);
    }
    Ok(Some(Driver {
        root: link_files::try_installation_dir()?,
        #[cfg(test)]
        fixture: false,
        #[cfg(test)]
        after_owner_observed: None,
    }))
}
fn configured(path: &Path, owners: &[u32]) -> anyhow::Result<bool> {
    match path.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            link_files::fs::check_folders(
                path,
                link_files::fs::Trust {
                    owners,
                    deny_mode: 0o022,
                    ancestors: true,
                    no_link: true,
                },
            )?;
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
        Ok(meta) => anyhow::ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "guest service settings must be in a regular file"
        ),
    }
    let read = link_files::fs::read_trusted(
        path,
        link_files::fs::Trust {
            owners,
            deny_mode: 0o022,
            ancestors: true,
            no_link: true,
        },
        4096,
        "guest service settings'",
    )?;
    let settings: Settings = link_files::saved::parse(&read.text)
        .with_context(|| format!("reading {}", path.display()))?;
    anyhow::ensure!(
        matches!(settings.mode, Mode::Guest),
        "unsupported guest service settings"
    );
    Ok(true)
}
#[expect(
    clippy::disallowed_methods,
    reason = "the guest controller polls durable intent and child exit; its bounded interval is independent of test time"
)]
fn poll() {
    std::thread::sleep(Duration::from_millis(20));
}
impl Driver {
    fn command(
        &self,
        role: Role,
        unit: &str,
        epoch: &str,
    ) -> anyhow::Result<std::process::Command> {
        let mut command = link_secretive::process::command(std::env::current_exe()?);
        #[cfg(test)]
        if self.fixture {
            command
                .args(["--exact", "guest::tests::entry", "--ignored", "--nocapture"])
                .env(
                    "LNK_TEST_GUEST_ROLE",
                    role.to_possible_value().expect("declared role").get_name(),
                )
                .env("LNK_TEST_GUEST_UNIT", unit)
                .env("LNK_TEST_GUEST_EPOCH", epoch)
                .env("LNK_TEST_GUEST_ROOT", &self.root);
        } else {
            command.arg(ENTRY).args(link_spec::argv(&GuestEntry {
                role,
                unit: unit.into(),
                epoch: epoch.into(),
            })?);
        }
        #[cfg(not(test))]
        command.arg(ENTRY).args(link_spec::argv(&GuestEntry {
            role,
            unit: unit.into(),
            epoch: epoch.into(),
        })?);
        command.env("LNK_INSTALLATION_DIR", &self.root);
        Ok(command)
    }
    fn entry(&self, given: GuestEntry) -> anyhow::Result<()> {
        let GuestEntry { role, unit, epoch } = given;
        anyhow::ensure!(
            epoch.len() == 32 && epoch.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid guest service generation"
        );
        let store = Store::new(&self.root, &unit)?;
        match role {
            Role::Supervisor => supervisor::run(self, &store, &unit, &epoch),
            Role::Worker => worker::run(&store, &epoch),
        }
    }
    pub fn start(&self, unit: &str, job: &Job, enabled: bool) -> anyhow::Result<()> {
        self.validate(unit, job)?;
        if !enabled {
            return Ok(());
        }
        let store = Store::new(&self.root, unit)?;
        anyhow::ensure!(
            !job.argv.is_empty()
                && job.argv.len() <= 256
                && job.workdir.is_absolute()
                && job.log.is_absolute(),
            "guest service needs bounded argv and absolute paths"
        );
        let revision = store.update(|saved| {
            if let Some(record) = saved {
                record.job = job.clone();
                record.advance(Desired::Running)
            } else {
                *saved = Some(Record {
                    job: job.clone(),
                    desired: Desired::Running,
                    revision: 1,
                    epoch: link_files::random_token()?,
                    guardian: None,
                    worker: None,
                    main: None,
                    observed: 0,
                    runs: 0,
                    error: None,
                });
                Ok(1)
            }
        })?;
        self.ensure(&store, unit, revision)?;
        Ok(())
    }
    pub fn validate(&self, unit: &str, job: &Job) -> anyhow::Result<()> {
        Store::new(&self.root, unit)?;
        anyhow::ensure!(
            !job.argv.is_empty()
                && job.argv.len() <= 256
                && job.workdir.is_absolute()
                && job.log.is_absolute(),
            "guest service needs bounded argv and absolute paths"
        );
        anyhow::ensure!(
            serde_json::to_vec(job)?.len() < MAX_STATE as usize / 2,
            "guest service job exceeds its limit"
        );
        Ok(())
    }
    fn change(&self, unit: &str, desired: Desired, wait: bool) -> anyhow::Result<()> {
        let store = Store::new(&self.root, unit)?;
        let changed = store.update(|saved| {
            saved
                .as_mut()
                .map(|record| {
                    let guardian = record.guardian.clone();
                    Ok((record.advance(desired)?, guardian))
                })
                .transpose()
        })?;
        let Some((revision, guardian)) = changed else {
            return Ok(());
        };
        if desired == Desired::Running {
            return self.ensure(&store, unit, revision);
        }
        if wait {
            let deadline = Instant::now() + CONTROL_TIMEOUT;
            self.wait(&store, revision, false, None, deadline)?;
            if desired == Desired::Removed
                && let Some(guardian) = guardian
            {
                while guardian.live()? {
                    anyhow::ensure!(
                        Instant::now() < deadline,
                        "guest controller did not finish cleanup"
                    );
                    poll();
                }
            }
            store.update(|saved| {
                let record = saved.as_mut().context("missing stopped guest service")?;
                anyhow::ensure!(
                    record.revision == revision,
                    "guest service operation was superseded"
                );
                if !record.guardian_live()? && record.cleanup_confirmed()? {
                    record.guardian = None;
                    record.worker = None;
                    record.main = None;
                    record.observed = revision;
                }
                Ok(())
            })?;
        }
        Ok(())
    }
    fn ensure(&self, store: &Store, unit: &str, revision: u64) -> anyhow::Result<()> {
        let deadline = Instant::now() + CONTROL_TIMEOUT;
        loop {
            anyhow::ensure!(
                Instant::now() < deadline,
                "guest service owner recovery timed out"
            );
            let epoch = store.update(|saved| {
                let record = saved.as_mut().context("missing guest service")?;
                anyhow::ensure!(
                    record.revision == revision && record.desired == Desired::Running,
                    "guest service operation was superseded"
                );
                if record.guardian_live()? {
                    return Ok(None);
                }
                anyhow::ensure!(
                    record.cleanup_confirmed()?,
                    "guest service cleanup is uncertain; refusing to replace its owner"
                );
                record.epoch = link_files::random_token()?;
                record.guardian = None;
                record.worker = None;
                record.main = None;
                Ok(Some(record.epoch.clone()))
            })?;
            #[cfg(test)]
            if epoch.is_none()
                && let Some(observed) = &self.after_owner_observed
            {
                observed();
            }
            let mut owner = if let Some(epoch) = epoch {
                let mut command = self.command(Role::Supervisor, unit, &epoch)?;
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                Some(OwnedChild::spawn(&mut command)?)
            } else {
                None
            };
            if !self.wait(store, revision, true, owner.as_ref(), deadline)? {
                // The last observation of a guardian being alive is not ownership.
                // Its death during readiness is recovered only after its group is
                // confirmed gone, with the same revision and overall deadline.
                drop(owner);
                continue;
            }
            if let Some(mut owner) = owner.take() {
                let recorded = store.read()?.context("missing service owner")?.guardian;
                if recorded
                    .as_ref()
                    .is_some_and(|id| id.pid == owner.child().id())
                {
                    // The durable controller owns its worker group. This parent only reaps
                    // the controller when it eventually exits; it never signals its PID.
                    let mut child = owner.release();
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
            }
            return Ok(());
        }
    }
    fn wait(
        &self,
        store: &Store,
        revision: u64,
        running: bool,
        pending: Option<&OwnedChild>,
        deadline: Instant,
    ) -> anyhow::Result<bool> {
        loop {
            let record = store.read()?.context("missing guest service state")?;
            anyhow::ensure!(
                record.revision == revision,
                "guest service operation was superseded"
            );
            if let Some(error) = &record.error {
                bail!("guest service: {error}");
            }
            if !record.guardian_live()? {
                let cleaned = record.cleanup_confirmed()?;
                if !running {
                    anyhow::ensure!(cleaned, "guest service owner lost; cleanup is uncertain");
                    return Ok(true);
                }
                let starting = pending.is_some_and(|owner| !owner.exited().unwrap_or(true))
                    && record.guardian.is_none();
                if !starting && cleaned {
                    return Ok(false);
                }
            }
            if record.observed == revision
                && if running {
                    record.guardian_live()?
                        && record
                            .main
                            .as_ref()
                            .is_some_and(|id| id.live().unwrap_or(false))
                } else {
                    record.worker.is_none() && record.main.is_none()
                }
            {
                return Ok(true);
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "guest service did not acknowledge its requested state"
            );
            poll();
        }
    }
    pub fn status(&self, unit: &str) -> Status {
        let read = || -> anyhow::Result<Status> {
            let store = Store::new(&self.root, unit)?;
            let Some(record) = store.read()? else {
                return Ok(Status::default());
            };
            let live = record.desired == Desired::Running
                && record.observed == record.revision
                && record.guardian_live()?
                && record.main.as_ref().map_or(Ok(false), Identity::live)?;
            Ok(Status {
                running: live,
                pid: if live {
                    record.main.map(|id| id.pid)
                } else {
                    None
                },
                runs: Some(record.runs),
            })
        };
        read().unwrap_or_default()
    }
}
pub(crate) fn pause(driver: &Driver, unit: &str) -> anyhow::Result<()> {
    driver.change(unit, Desired::Paused, true)
}
pub(crate) fn resume(driver: &Driver, unit: &str) -> anyhow::Result<()> {
    driver.change(unit, Desired::Running, true)
}
pub(crate) fn stop(driver: &Driver, unit: &str, wait: bool) -> anyhow::Result<()> {
    driver.change(unit, Desired::Removed, wait)
}

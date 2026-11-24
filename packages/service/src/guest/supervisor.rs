use super::*;
use std::{io::Write, os::fd::AsRawFd};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Launch {
    pub job: Job,
    pub revision: u64,
    pub ready: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ready {
    pub revision: u64,
    pub worker: Identity,
    pub main: Identity,
}
/// This thread is the child's kernel parent. It keeps running during pause and
/// owns the unreaped worker leader until the complete group is stopped.
pub(super) fn run(driver: &Driver, store: &Store, unit: &str, epoch: &str) -> anyhow::Result<()> {
    let _owner = store.lock("owner.lock", true)?;
    let guardian = Identity::of(std::process::id())?;
    store.update(|saved| {
        let record = saved.as_mut().context("missing service intent")?;
        anyhow::ensure!(
            record.epoch == epoch && record.desired == Desired::Running,
            "superseded guest controller"
        );
        anyhow::ensure!(
            record.cleanup_confirmed()?,
            "previous guest group cleanup is uncertain"
        );
        record.guardian = Some(guardian.clone());
        Ok(())
    })?;
    let mut active: Option<OwnedChild> = None;
    let mut active_revision = 0;
    let mut ready_path = None;
    let mut restart = Instant::now();
    let result = (|| -> anyhow::Result<()> {
        'supervise: loop {
            let record = store.read()?.context("missing guest service intent")?;
            if record.epoch != epoch {
                return Ok(());
            }
            let exited = active
                .as_ref()
                .map(OwnedChild::exited)
                .transpose()?
                .unwrap_or(false);
            if active.is_none() && record.revision != active_revision {
                restart = Instant::now();
            }
            if active.is_some()
                && (exited
                    || record.revision != active_revision
                    || record.desired != Desired::Running)
            {
                active.take().expect("active worker").finish()?;
                if let Some(path) = ready_path.take() {
                    let _ = std::fs::remove_file(path);
                }
                store.update(|saved| {
                    let record = current(saved, epoch)?;
                    record.worker = None;
                    record.main = None;
                    if record.desired != Desired::Running {
                        record.observed = record.revision;
                    }
                    Ok(())
                })?;
                restart = if exited && record.revision == active_revision {
                    Instant::now() + RESTART_DELAY
                } else {
                    Instant::now()
                };
            }
            if record.desired != Desired::Running {
                if record.desired == Desired::Paused
                    && record.observed == record.revision
                    && record.worker.is_none()
                {
                    poll();
                    continue;
                }
                let removed = store.update(|saved| {
                    let record = current(saved, epoch)?;
                    if record.worker.is_none() {
                        record.main = None;
                        record.observed = record.revision;
                    }
                    let removed = record.desired == Desired::Removed;
                    if removed {
                        record.guardian = None;
                    }
                    Ok(removed)
                })?;
                if removed {
                    return Ok(());
                }
            } else if active.is_none() && Instant::now() >= restart {
                let ready = link_files::random_token()?;
                let path = store.0.join(format!("ready-{ready}.json"));
                let mut command = driver.command(Role::Worker, unit, epoch)?;
                command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                link_secretive::process::ends_with_parent(&mut command, libc::SIGTERM);
                let mut child = OwnedChild::spawn(&mut command)?;
                let worker = Identity::of(child.child().id())?;
                let launch_revision = record.revision;
                let admitted = store.update(|saved| {
                    let record = current(saved, epoch)?;
                    if record.revision != launch_revision || record.desired != Desired::Running {
                        return Ok(false);
                    }
                    record.worker = Some(worker.clone());
                    record.main = None;
                    record.runs = record
                        .runs
                        .checked_add(1)
                        .context("service start count exhausted")?;
                    Ok(true)
                })?;
                if !admitted {
                    child.finish()?;
                    continue;
                }
                // Re-read intent, then commit only the sampled revision below. The
                // worker cannot start a job until its identity is durably recorded.
                let latest = store.read()?.context("missing guest launch")?;
                if latest.epoch != epoch
                    || latest.revision != record.revision
                    || latest.desired != Desired::Running
                {
                    child.finish()?;
                    store.update(|saved| {
                        let record = current(saved, epoch)?;
                        record.worker = None;
                        record.main = None;
                        Ok(())
                    })?;
                    continue;
                }
                let launch = Launch {
                    job: record.job.clone(),
                    revision: record.revision,
                    ready,
                };
                let bytes = serde_json::to_vec(&launch)?;
                anyhow::ensure!(
                    bytes.len() as u64 <= MAX_STATE,
                    "guest launch exceeds its limit"
                );
                let mut input = child
                    .child()
                    .stdin
                    .take()
                    .context("missing guest worker input")?;
                // A stopped worker must not block its unpaused guardian in a
                // full input pipe. Parent death and intent cleanup stay usable.
                let fd = input.as_raw_fd();
                // SAFETY: this live pipe descriptor is exclusively held here.
                let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
                anyhow::ensure!(
                    flags >= 0
                        && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
                    "configuring bounded guest worker input"
                );
                let deadline = Instant::now() + CONTROL_TIMEOUT;
                let mut remaining = bytes.as_slice();
                while !remaining.is_empty() {
                    let latest = store.read()?.context("missing guest worker admission")?;
                    if latest.epoch != epoch
                        || latest.revision != launch_revision
                        || latest.desired != Desired::Running
                    {
                        drop(input);
                        child.finish()?;
                        store.update(|saved| {
                            let record = current(saved, epoch)?;
                            record.worker = None;
                            record.main = None;
                            Ok(())
                        })?;
                        continue 'supervise;
                    }
                    match input.write(remaining) {
                        Ok(0) => bail!("guest worker closed its input"),
                        Ok(count) => remaining = &remaining[count..],
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            anyhow::ensure!(
                                Instant::now() < deadline && !child.exited()?,
                                "guest worker input timed out"
                            );
                            poll();
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                drop(input);
                active_revision = record.revision;
                ready_path = Some(path);
                active = Some(child);
            }
            if let Some(path) = &ready_path {
                match path.symlink_metadata() {
                    Ok(_) => {
                        let uid = unsafe { libc::geteuid() };
                        let text = link_files::fs::read_trusted(
                            path,
                            link_files::fs::Trust::alone(&[uid]),
                            8192,
                            "guest readiness's",
                        )?;
                        let ready: Ready = serde_json::from_str(&text.text)?;
                        store.update(|saved| {
                            let record = current(saved, epoch)?;
                            if record.revision == ready.revision
                                && record.desired == Desired::Running
                                && record.worker.as_ref() == Some(&ready.worker)
                                && ready.main.live()?
                            {
                                record.main = Some(ready.main);
                                record.observed = ready.revision;
                            }
                            Ok(())
                        })?;
                        std::fs::remove_file(path)?;
                        ready_path = None;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            poll();
        }
    })();
    // Finish before clearing identities, even on malformed intent or I/O failure.
    let cleanup = active.take().map(OwnedChild::finish).transpose();
    if let Some(path) = ready_path {
        let _ = std::fs::remove_file(path);
    }
    let failure = result
        .as_ref()
        .err()
        .map(|_| "guest controller failed".to_owned());
    let _ = store.update(|saved| {
        let record = current(saved, epoch)?;
        if cleanup.is_ok() && record.cleanup_confirmed()? {
            record.worker = None;
            record.main = None;
        }
        record.guardian = None;
        record.error = failure;
        Ok(())
    });
    cleanup?;
    result
}
fn current<'a>(saved: &'a mut Option<Record>, epoch: &str) -> anyhow::Result<&'a mut Record> {
    let record = saved.as_mut().context("missing guest service")?;
    anyhow::ensure!(
        record.epoch == epoch,
        "guest controller generation was superseded"
    );
    Ok(record)
}

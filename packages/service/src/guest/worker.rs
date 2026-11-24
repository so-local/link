use super::*;
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
};
use supervisor::{Launch, Ready};
pub(super) const MAX_LOG: u64 = 8 * 1024 * 1024;
/// Only this fresh worker group is signalled. No persisted PID is used. The
/// guardian remains unpaused and the kernel delivers parent death even while
/// this worker's main thread is waiting on its job or its log pumps.
extern "C" fn end_group(_: libc::c_int) {
    // SAFETY: kill and _exit are async-signal-safe; entry verified our own PGID.
    unsafe {
        libc::kill(0, libc::SIGKILL);
        libc::_exit(1);
    }
}
pub(super) fn run(store: &Store, epoch: &str) -> anyhow::Result<()> {
    // SAFETY: these calls inspect this process's own group.
    anyhow::ensure!(
        unsafe { libc::getpgrp() } == unsafe { libc::getpid() },
        "guest worker must own its group"
    );
    // SAFETY: the handler uses only async-signal-safe operations in its own group.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = end_group as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut signals: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut signals);
        libc::sigaddset(&mut signals, libc::SIGTERM);
        if libc::sigprocmask(libc::SIG_UNBLOCK, &signals, std::ptr::null_mut()) != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    // Every failure after group setup also kills descendants before leaving.
    let launch = launch();
    let revision = launch.as_ref().ok().map(|launch| launch.revision);
    let result = launch.and_then(|launch| execute(store, epoch, &launch));
    if result.is_err() {
        let _ = store.update(|saved| {
            let record = saved.as_mut().context("missing failed guest launch")?;
            // A launch a later stop or start revoked failed as it should:
            // its failure isn't the intent now's.
            if record.epoch == epoch
                && revision.is_none_or(|revision| revision == record.revision)
                && record
                    .worker
                    .as_ref()
                    .is_some_and(|id| id.pid == std::process::id())
            {
                record.error = Some("guest worker failed to start or collect its job".into());
            }
            Ok(())
        });
    }
    end_group(0);
    result
}
/// What the supervisor asked this worker to launch, on its stdin.
fn launch() -> anyhow::Result<Launch> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_STATE + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_STATE,
        "guest launch exceeds its limit"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
fn execute(store: &Store, epoch: &str, launch: &Launch) -> anyhow::Result<()> {
    anyhow::ensure!(
        launch.ready.len() == 32 && launch.ready.bytes().all(|c| c.is_ascii_hexdigit()),
        "invalid guest readiness identity"
    );
    let record = store.read()?.context("missing guest service")?;
    let worker = Identity::of(std::process::id())?;
    anyhow::ensure!(
        record.epoch == epoch
            && record.revision == launch.revision
            && record.desired == Desired::Running
            && record.worker.as_ref() == Some(&worker),
        "guest launch was revoked"
    );
    let program = launch
        .job
        .argv
        .first()
        .context("missing guest service program")?;
    let log = crate::open_log(&launch.job.log)?;
    if log.metadata()?.len() > MAX_LOG {
        log.set_len(0)?;
        link_files::write_private(&crate::start_path(&launch.job.log), b"0")?;
    }
    let mut command = link_secretive::process::command(program);
    command
        .args(&launch.job.argv[1..])
        .envs(&launch.job.env)
        .current_dir(&launch.job.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The job stays in this worker's group, which the guardian owns unreaped.
    let mut child = command.spawn().context("starting guest service job")?;
    let main = Identity::of(child.id())?;
    let ready = Ready {
        revision: launch.revision,
        worker,
        main,
    };
    link_files::write_private(
        &store.0.join(format!("ready-{}.json", launch.ready)),
        &serde_json::to_vec(&ready)?,
    )?;
    let log = Arc::new(Mutex::new(log));
    let (drained, completed) = std::sync::mpsc::channel();
    for reader in [
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let log = log.clone();
        let drained = drained.clone();
        let offset = crate::start_path(&launch.job.log);
        std::thread::spawn(move || {
            if pump(reader, log, &offset).is_err() {
                end_group(0);
            }
            let _ = drained.send(());
        });
    }
    drop(drained);
    child.wait()?;
    // A descendant can retain either pipe after the main process exits. Drain
    // completed output, then stop the whole owned group on a bounded deadline.
    let deadline = Instant::now() + Duration::from_secs(1);
    for _ in 0..2 {
        if completed
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .is_err()
        {
            break;
        }
    }
    Ok(())
}
pub(super) fn pump(
    mut reader: impl Read,
    log: Arc<Mutex<std::fs::File>>,
    offset: &Path,
) -> anyhow::Result<()> {
    let mut bytes = [0u8; 8192];
    loop {
        let count = match reader.read(&mut bytes) {
            Ok(0) => return Ok(()),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        let mut file = log
            .lock()
            .map_err(|_| anyhow::anyhow!("guest service log lock failed"))?;
        if file.metadata()?.len().saturating_add(count as u64) > MAX_LOG {
            file.set_len(0)?;
            link_files::write_private(offset, b"0")?;
        }
        file.write_all(&bytes[..count])?;
        file.flush()?;
    }
}

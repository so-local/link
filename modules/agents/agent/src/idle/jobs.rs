//! The agent's job started and stopped, its service's or its terminal's,
//! and a start in a terminal run ([`run_in_terminal`]).

use super::*;

/// Starts an agent's job: its service, or its start's child in a
/// terminal.
pub(super) fn start_job(job: &Job) -> anyhow::Result<()> {
    match job {
        Job::Service { label, unit } => link_service::resume(label, unit),
        Job::Terminal(control) => tell(control, "wake"),
    }
}

/// Stops an agent's job, its service kept for the next start.
pub(super) fn stop_job(job: &Job) -> anyhow::Result<()> {
    match job {
        Job::Service { label, unit } => link_service::pause(label, unit),
        Job::Terminal(control) => tell(control, "sleep"),
    }
}

/// Tells an agent's start in a terminal, at `control`, to `sleep` or
/// `wake`: answered `ok` once done.
fn tell(control: &Path, what: &str) -> anyhow::Result<()> {
    let mut stream = UnixStream::connect(control)
        .map_err(|err| anyhow::anyhow!("its start in a terminal isn't there: {err}"))?;
    stream.write_all(format!("{what}\n").as_bytes())?;
    let mut answer = String::new();
    std::io::BufReader::new(stream).read_line(&mut answer)?;
    anyhow::ensure!(answer.trim() == "ok", "its start said {:?}", answer.trim());
    Ok(())
}

/// Starts the harness an owner's agents share, unless it runs: its
/// service where it has one, else in a session of its own.
pub(super) fn start_shared(shared: &Shared) {
    let running = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(shared.dir.join("serve.lock"))
        .is_ok_and(|lock| lock.try_lock().is_err());
    if running {
        return;
    }
    if link_service::managed()
        && link_service::unit_path_of(&shared.unit).exists()
        && link_service::systemctl(&["start", &shared.unit]).is_ok()
    {
        return;
    }
    if let Err(err) =
        crate::bridges::spawn_detached(&shared.argv, &shared.dir, &shared.dir.join("shared.log"))
    {
        link_format::daemon::warn!("starting its owner's harness: {err:#}");
    }
}

/// Runs `argv` in a terminal's agent start (`lnk agent start
/// --foreground`), stopped and started again as the bridge says at
/// `control` (`sleep`, `wake`), until it ends of its own accord, or,
/// once `stopped` (a SIGTERM or SIGHUP the start caught), ends while
/// asleep. `children` hears each child's process ID as it starts, and
/// none as it's stopped, so a stop is passed on to the one running.
/// Each child's output is shown and appended to `log`
/// ([`crate::service::spawn_logged`]).
#[expect(
    clippy::disallowed_methods,
    reason = "starts the job with its own environment, and polls it"
)]
pub fn run_in_terminal(
    argv: &[String],
    env: &std::collections::BTreeMap<String, String>,
    dir: &Path,
    log: &crate::service::Log,
    control: &Path,
    children: fn(Option<u32>),
    stopped: &'static AtomicBool,
) -> anyhow::Result<std::process::ExitStatus> {
    use std::os::unix::process::ExitStatusExt;
    let (argv, env, dir) = (argv.to_vec(), env.clone(), dir.to_path_buf());
    let log = crate::service::Log {
        file: log.file.try_clone()?,
        agent: log.agent.clone(),
    };
    let spawn = move || {
        // Ended with the start, however it ends: killed outright, it can
        // pass nothing on. Each thread starting one lives as long as it.
        let child = crate::service::spawn_logged(
            link_secretive::process::ends_with_parent(
                std::process::Command::new(&argv[0])
                    .args(&argv[1..])
                    .env_clear()
                    .envs(&env)
                    .current_dir(&dir),
                libc::SIGTERM,
            ),
            &log,
        )?;
        children(Some(child.id()));
        Ok::<_, std::io::Error>(child)
    };
    let child = Arc::new(Mutex::new(Some(spawn()?)));
    let listener = link_sandbox_bridge::socket::listen_private(control)?;
    let told = child.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if std::io::BufReader::new(&stream)
                .read_line(&mut line)
                .is_err()
            {
                continue;
            }
            let mut held = told.lock().unwrap_or_else(|e| e.into_inner());
            let done = match line.trim() {
                "sleep" => {
                    if let Some(mut running) = held.take() {
                        children(None);
                        end_tree(&mut running);
                    }
                    true
                }
                // Stopping: nothing more is started.
                "wake" if stopped.load(Ordering::SeqCst) => false,
                "wake" => match held.as_ref() {
                    Some(_) => true,
                    None => match spawn() {
                        Ok(running) => {
                            *held = Some(running);
                            true
                        }
                        Err(_) => false,
                    },
                },
                _ => false,
            };
            let mut stream = stream;
            let _ = stream.write_all(if done { b"ok\n" } else { b"no\n" });
        }
    });
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let mut held = child.lock().unwrap_or_else(|e| e.into_inner());
        let status = match held.as_mut() {
            Some(running) => running.try_wait()?,
            // Stopped while asleep: nothing to wait for.
            None if stopped.load(Ordering::SeqCst) => Some(std::process::ExitStatus::from_raw(0)),
            None => None,
        };
        if let Some(status) = status {
            let _ = std::fs::remove_file(control);
            return Ok(status);
        }
    }
}

/// Ends `child` and every process under it: told first, then killed
/// after a few seconds.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a stopped tree until it ends"
)]
fn end_tree(child: &mut std::process::Child) {
    let mut pids = Vec::new();
    let mut todo = vec![child.id()];
    while let Some(pid) = todo.pop() {
        pids.push(pid);
        todo.extend(children_of(pid));
    }
    let signal = |pids: &[u32], signal: i32| {
        for &pid in pids {
            // SAFETY: a signal to processes found under our own child.
            unsafe { libc::kill(pid as i32, signal) };
        }
    };
    signal(&pids, libc::SIGTERM);
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    signal(&pids[1..], libc::SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
}

/// Process `pid`'s children: from `/proc` on Linux, else `pgrep -P`.
fn children_of(pid: u32) -> Vec<u32> {
    let parse = |text: &str| -> Vec<u32> {
        text.split_whitespace()
            .filter_map(|p| p.parse::<u32>().ok())
            .collect()
    };
    if Path::new("/proc/self/task").exists() {
        return std::fs::read_dir(format!("/proc/{pid}/task"))
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|task| {
                parse(&std::fs::read_to_string(task.path().join("children")).unwrap_or_default())
            })
            .collect();
    }
    link_secretive::process::command("pgrep")
        .args(["-P", &pid.to_string()])
        .output()
        .map(|output| parse(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

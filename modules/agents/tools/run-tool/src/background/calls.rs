//! Starting, listing and stopping: the calls.

use super::*;

/// A command to start in the background, as the call gave it.
pub struct Starting {
    pub context: Option<Arc<link_sandbox_client::RunWith>>,
    /// Its command, as the agent gave it.
    pub command: String,
    /// What runs: the sandbox, then the shell, `-c` and the command.
    pub argv: Vec<String>,
    /// The longest it runs: the call's timeout, at most the background's.
    pub timeout: Duration,
    /// The PATH it runs with.
    pub path: String,
    /// The files folder its output goes in.
    pub files: PathBuf,
    /// The conversation that started it, and the call's hops.
    pub thread: String,
    pub hops: u32,
    /// How much of its output's end its ending says.
    pub tail: u64,
}

/// Starts `starting` under a supervisor, refused past the caps. Its id.
pub fn start(background: &Background, starting: Starting) -> anyhow::Result<String> {
    let Starting {
        context,
        command,
        argv,
        timeout,
        path,
        files,
        thread,
        hops,
        tail,
    } = starting;
    ensure!(
        background.at_once > 0,
        "background commands are off for you ([tool.run] background is 0)"
    );
    ensure!(
        hops < background.hops,
        "this conversation is {hops} messages down a chain, the most there may be: no \
         background command starts here"
    );
    link_files::create_private_dir(&background.state)
        .with_context(|| format!("making {}", background.state.display()))?;
    // One start at a time, so two turns can't both take the last place.
    let starting = open_lock(&background.state, "start")?;
    starting.lock().context("waiting for another start")?;
    sweep_quietly(&background.state);
    let running = records(&background.state)
        .iter()
        .filter(|r| r.state.running())
        .count();
    ensure!(
        (running as u32) < background.at_once,
        "{running} background commands run already, the most you may run at once: wait for \
         one to end, or stop one (task__stop)"
    );
    // Each one's ending starts a turn: a day's, counted once it started,
    // all under the start's lock, so two starts never both take the last.
    let started = started_today(&background.state)?;
    ensure!(
        (started.len() as u32) < background.per_day,
        "{} background commands started in the last day, the most you may start in one: \
         no more until a day after the first of them",
        started.len()
    );
    let id = link_id::candidate()?;
    let record = Record {
        id: id.clone(),
        command,
        argv,
        path,
        files,
        thread,
        hops,
        way_in: background.way_in.clone(),
        caller: background.caller.clone(),
        timeout_ms: timeout.min(background.timeout).as_millis() as u64,
        output_max: background.output,
        tail,
        keep_secs: background.keep.as_secs(),
        started: now(),
        ended: None,
        took_ms: None,
        state: State::Running,
        exit: None,
        signal: None,
        why: None,
        group: None,
        leader_start: None,
        boot: boot(),
        dropped: 0,
        output_file: 0,
        told: false,
    };
    // Counted before it starts, still under the start's lock: a start
    // whose count can't be written is refused, never left uncounted.
    note_started(&background.state, &started)?;
    save(&background.state, &record)?;
    // The lock, taken here and handed to the supervisor as its input, so
    // no sweep ever sees it free while it starts.
    let lock = open_lock(&background.state, &id)?;
    lock.try_lock().context("a new command's lock is held")?;
    let mut command = link_secretive::process::command(&background.program);
    command
        .args(
            link_run_tool_contract::cli::RunToolSuperviseArgs {
                with_fd: context
                    .as_ref()
                    .and_then(|_| u32::try_from(crate::CONTEXT_FD).ok()),
                id: id.clone(),
                state: background.state.clone(),
            }
            .argv(),
        )
        .stdin(lock)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let handoff = match context
        .as_ref()
        .map(|context| context.hand_to(&mut command, crate::CONTEXT_FD))
        .transpose()
    {
        Ok(handoff) => handoff,
        Err(error) => {
            let _ = std::fs::remove_file(record_path(&background.state, &id));
            let _ = std::fs::remove_file(lock_path(&background.state, &id));
            return Err(error);
        }
    };
    let child = command.spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => {
            let _ = std::fs::remove_file(record_path(&background.state, &id));
            let _ = std::fs::remove_file(lock_path(&background.state, &id));
            bail!("starting its supervisor: {err}");
        }
    };
    if let Some(handoff) = handoff
        && let Err(error) = handoff.send()
    {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(record_path(&background.state, &id));
        let _ = std::fs::remove_file(lock_path(&background.state, &id));
        return Err(error);
    }
    // Waited on, so it's never left a zombie while this server runs.
    std::thread::spawn(move || child.wait());
    drop(starting);
    Ok(id)
}

/// When each background command started in the last day did, oldest
/// first, from `state`'s `started` file: one second since 1970 a line.
/// None yet without the file; an error when it can't be read, so the cap
/// never opens on a count it couldn't see.
fn started_today(state: &Path) -> anyhow::Result<Vec<u64>> {
    let day_ago = now().saturating_sub(86_400);
    let text = match std::fs::read_to_string(state.join("started")) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(err).context("reading the day's count of background commands started");
        }
    };
    Ok(text
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .filter(|at| *at > day_ago)
        .collect())
}

/// Notes a start in `state`'s `started` file, beside the last day's
/// (`started`).
fn note_started(state: &Path, started: &[u64]) -> anyhow::Result<()> {
    let mut started = started.to_vec();
    started.push(now());
    let text: String = started.iter().map(|at| format!("{at}\n")).collect();
    link_files::write_private(&state.join("started"), text.as_bytes())
        .context("counting its start, so it wasn't started")
}

/// Every background command conversation `thread` started, each as a
/// value, and in words.
pub fn list(state: &Path, thread: &str) -> (Value, String) {
    sweep_quietly(state);
    let mut records = records(state);
    records.retain(|r| r.thread == thread);
    let text = match records.is_empty() {
        true => "No background commands.".to_string(),
        false => records
            .iter()
            .map(|r| {
                format!(
                    "{}  {}  {}  {} bytes  {}",
                    r.id,
                    r.how(),
                    took(r.ran()),
                    r.bytes(),
                    r.line()
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    (
        json!({"commands": records.iter().map(Record::value).collect::<Vec<_>>()}),
        text,
    )
}

/// Asks command `id`'s supervisor to stop it, and waits until it's
/// recorded ended. Its record.
pub fn stop(state: &Path, id: &str) -> anyhow::Result<Record> {
    stop_for(state, id, "")
}

/// [`stop`]; with a reason (`why`), it's recorded lost for it and not
/// told: the agent itself is stopping, moving or being deleted.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a stopping program until it ends"
)]
pub(super) fn stop_for(state: &Path, id: &str, why: &str) -> anyhow::Result<Record> {
    let record = load(state, id)?;
    if !record.state.running() {
        return Ok(record);
    }
    std::fs::write(stop_path(state, id), why.as_bytes())
        .with_context(|| format!("asking {id} to stop"))?;
    let deadline = Instant::now() + link_run_tool_contract::limits().stop_wait();
    loop {
        sweep_quietly(state);
        let record = load(state, id)?;
        if !record.state.running() {
            return Ok(record);
        }
        ensure!(
            Instant::now() < deadline,
            "{id} didn't stop within {}s",
            link_run_tool_contract::limits().stop_wait().as_secs()
        );
        std::thread::sleep(link_run_tool_contract::limits().tick());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A start whose count can't be read or written is refused, before
    /// anything starts: the day's cap never fails open.
    #[test]
    fn a_start_it_cannot_count_is_refused() {
        let place = link_test_kit::TempDir::new("run-uncounted");
        let state = place.join("state");
        // The count is a folder: neither read nor written.
        std::fs::create_dir_all(state.join("started")).unwrap();
        // A supervisor that starts and ends at once, were it started.
        let supervisor = place.join("supervisor");
        link_test_kit::write_program(&supervisor, "#!/bin/sh\nexit 0\n").unwrap();
        let background = Background {
            program: supervisor,
            state: state.clone(),
            way_in: place.join("way-in"),
            caller: Caller::default(),
            at_once: 2,
            timeout: Duration::from_secs(60),
            output: 1024,
            keep: Duration::from_secs(60),
            per_day: 10,
            hops: 4,
        };
        let starting = Starting {
            context: None,
            command: "true".into(),
            argv: vec!["true".into()],
            timeout: Duration::from_secs(60),
            path: String::new(),
            files: place.to_path_buf(),
            thread: "test:1".into(),
            hops: 0,
            tail: 512,
        };
        let refused = start(&background, starting).unwrap_err();
        assert!(format!("{refused:#}").contains("started"), "{refused:#}");
        assert!(records(&state).is_empty(), "a record of an uncounted start");
    }
}

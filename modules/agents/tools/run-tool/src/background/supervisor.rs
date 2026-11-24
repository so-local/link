//! The supervisor: runs its command in the sandbox, gated, writes its
//! output, stops it at its timeout, and records how it ended.

use super::*;

/// Set by SIGTERM, SIGINT or SIGHUP: the agent's service is stopping.
static ENDING_NOW: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    ENDING_NOW.store(true, Ordering::SeqCst);
}

/// Supervises command `id` of `state` to its end, then tells the agent;
/// its lock is its input, taken by the server that started it.
pub fn supervise(state: &Path, id: &str) -> anyhow::Result<()> {
    supervise_with(state, id, None)
}

pub fn supervise_with(
    state: &Path,
    id: &str,
    context: Option<&link_sandbox_client::RunWith>,
) -> anyhow::Result<()> {
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(signal, on_signal as *const () as libc::sighandler_t);
        }
    }
    // Its lock: its input, held for its whole life.
    // SAFETY: descriptor 0, which it owns from here.
    let lock = unsafe { File::from_raw_fd(0) };
    let mut record = load(state, id)?;
    let ran = run_supervised(state, &mut record, context);
    if let Err(err) = &ran {
        record.state = State::Lost;
        record.why = Some(format!("{err:#}"));
        record.ended.get_or_insert(now());
    }
    save(state, &record)?;
    let _ = std::fs::remove_file(stop_path(state, id));
    if record.state == State::Lost && ENDING_NOW.load(Ordering::SeqCst) {
        // The agent is stopping: told when it's back, by a sweep.
        return Ok(());
    }
    let _ = tell(state, &mut record);
    drop(lock);
    Ok(())
}

/// The descriptor a command's gate is on, as [`run_supervised`] starts it.
const GATE_FD: libc::c_int = 3;

/// `gate -- <argv>`, as its supervisor starts a command: waits on its
/// gate for the word that its record names this process's group, then
/// becomes `argv`, the same process. Without the word (its supervisor
/// ended first) it runs nothing.
pub fn gate(argv: &[String]) -> anyhow::Result<()> {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    let (program, args) = argv.split_first().context("no command")?;
    // SAFETY: the gate, its own from here; closed before it becomes
    // `argv`, so nothing it runs holds it.
    let mut gate = unsafe { File::from_raw_fd(GATE_FD) };
    let mut word = [0u8; 1];
    let opened = matches!(gate.read(&mut word), Ok(1));
    drop(gate);
    ensure!(opened, "its supervisor ended before it started");
    let err = link_secretive::process::command(program).args(args).exec();
    Err(err).with_context(|| format!("starting {program}"))
}

#[expect(clippy::disallowed_methods, reason = "the supervisor's own period")]
fn run_supervised(
    state: &Path,
    record: &mut Record,
    context: Option<&link_sandbox_client::RunWith>,
) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut output = Output::create(&record.files, &record.id, record.output_max)?;
    let (reader, writer) = std::io::pipe().context("a pipe for its output")?;
    let errors = writer.try_clone().context("a pipe for its output")?;
    // The command runs through this program's gate ([`gate`]), opened
    // once its record names its group: a supervisor killed before that
    // leaves nothing of it running that a sweep wouldn't find.
    let (gate_out, mut gate_in) = std::io::pipe().context("a pipe for its gate")?;
    let gate_fd = gate_out.as_raw_fd();
    // The files folder, opened; it starts there by its descriptor, not
    // its path.
    let folder = Dir::open(&record.files)
        .with_context(|| format!("opening the files folder {}", record.files.display()))?;
    let folder_fd = std::os::fd::AsFd::as_fd(&folder).as_raw_fd();
    let mut command =
        link_secretive::process::command(std::env::current_exe().context("this program")?);
    command
        .args(
            link_run_tool_contract::cli::RunToolGateArgs {
                command: record.argv.clone(),
            }
            .argv(),
        )
        .env("PATH", &record.path)
        .stdin(std::process::Stdio::null())
        .stdout(writer)
        .stderr(errors)
        .process_group(0);
    // It and its sandbox die with this supervisor, however it ends.
    link_secretive::process::ends_with_parent(&mut command, libc::SIGKILL);
    // SAFETY: only fchdir, fcntl and dup2 between fork and exec.
    unsafe {
        command.pre_exec(move || {
            if libc::fchdir(folder_fd) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Its gate, where it waits: past stderr, never inherited past
            // the gate, which closes it.
            let moved = match gate_fd == GATE_FD {
                // Already there: only kept past exec.
                true => libc::fcntl(GATE_FD, libc::F_SETFD, 0),
                false => libc::dup2(gate_fd, GATE_FD),
            };
            if moved < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let handoff = context
        .map(|context| context.hand_to(&mut command, crate::CONTEXT_FD))
        .transpose()?;
    let started = Instant::now();
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {}", record.argv[0]))?;
    // The pipes' other ends are the child's alone now.
    drop(command);
    drop(gate_out);
    drop(folder);
    let group = child.id() as i32;
    record.group = Some(group);
    record.leader_start = started_at(group);
    record.output_file = output.file_id();
    save(state, record)?;
    // Its group named: it runs.
    gate_in
        .write_all(b"1")
        .context("opening its gate: it ended")?;
    drop(gate_in);
    if let Some(handoff) = handoff
        && let Err(error) = handoff.send()
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    // Its output, read on a thread of its own; past its most, the
    // record says what was dropped.
    let (dropped_tx, dropped_rx) = std::sync::mpsc::channel::<(u64, u64)>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0u8; 65536];
        while let Ok(n @ 1..) = std::io::Read::read(&mut reader, &mut buffer) {
            match output.push(&buffer[..n]) {
                Ok(true) => {
                    let _ = dropped_tx.send((output.dropped, output.file_id()));
                }
                Ok(false) => {}
                Err(_) => break,
            }
        }
        let _ = done_tx.send(());
    });
    let kill = || crate::kill_group(group);
    let deadline = started + Duration::from_millis(record.timeout_ms);
    let stop = stop_path(state, &record.id);
    let mut lost_for = None;
    // Its leader is reaped only once its group is killed: till then its
    // ID, the group's, can't be another's.
    let ended_as = loop {
        if crate::exited(group) {
            break None;
        }
        let asked = if ENDING_NOW.load(Ordering::SeqCst) {
            Some(State::Lost)
        } else if let Ok(why) = std::fs::read_to_string(&stop) {
            match why.trim() {
                "" => Some(State::Stopped),
                why => {
                    // The agent going: lost for that, and not told.
                    ENDING_NOW.store(true, Ordering::SeqCst);
                    lost_for = Some(why.to_string());
                    Some(State::Lost)
                }
            }
        } else if Instant::now() >= deadline {
            Some(State::TimedOut)
        } else {
            None
        };
        if let Some(state) = asked {
            kill();
            break Some(state);
        }
        while let Ok((dropped, file)) = dropped_rx.try_recv() {
            (record.dropped, record.output_file) = (dropped, file);
            save(state, record)?;
        }
        std::thread::sleep(link_run_tool_contract::limits().tick());
    };
    let _ = done_rx.recv_timeout(link_run_tool_contract::limits().drain());
    // What it left running goes with it, then its leader is reaped.
    kill();
    let status = child.wait().ok();
    while let Ok((dropped, file)) = dropped_rx.try_recv() {
        (record.dropped, record.output_file) = (dropped, file);
    }
    record.took_ms = Some(started.elapsed().as_millis() as u64);
    record.ended = Some(now());
    // The agent stopping ends it whatever its status says: SIGTERM
    // reaches its whole service at once.
    let ended_as = match ENDING_NOW.load(Ordering::SeqCst) {
        true => Some(State::Lost),
        false => ended_as,
    };
    match ended_as {
        Some(state) => {
            record.state = state;
            if state == State::Lost {
                record.why = Some(lost_for.unwrap_or_else(|| "the agent stopped".into()));
            }
        }
        None => {
            use std::os::unix::process::ExitStatusExt;
            let status = status.context("its status")?;
            match status.code() {
                Some(code) => {
                    record.state = State::Exited;
                    record.exit = Some(code);
                }
                None => {
                    record.state = State::Signalled;
                    record.signal = status.signal();
                }
            }
        }
    }
    Ok(())
}

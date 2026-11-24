//! What a sandbox runs, watched from outside on macOS: each program,
//! learned or refused, read from Seatbelt's reports in the system log
//! ([`super::reports`]).

use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

use anyhow::Context;

use super::reports::{self, stream};

/// `lnk sandbox learn` on macOS: runs `argv` (wrapped to report each
/// program, `env` its whole environment), and returns its exit status
/// and each program it started, as Seatbelt reports them in the system
/// log, a line each.
pub fn learn(argv: &[String], env: &[(String, String)]) -> anyhow::Result<(ExitStatus, String)> {
    let watch = stream::Stream::start().context(
        "macOS's log, where Seatbelt reports each program a sandbox runs, can't be read \
         here: log stream needs an admin user",
    )?;
    let mut child = link_secretive::process::command(&argv[0])
        .args(&argv[1..])
        .env_clear()
        .envs(env.iter().cloned())
        .spawn()
        .with_context(|| format!("running {}", argv[0]))?;
    let root = child.id();
    watch.run_is(root);
    let status = child.wait()?;
    let all = watch.finish(Duration::from_secs(1));
    let programs = reports::programs(&all, true, |pid| stream::ours(pid, root));
    Ok((status, reports::learn_lines(&programs)))
}

/// `sandbox refusals` (macOS): runs `argv` (`sandbox-exec` and the
/// program), says each program `programs` refuses it, as Seatbelt reports
/// it in the system log, and returns its exit status. Without the log
/// (`log stream` needs an admin user), it only runs it.
pub fn refusals(programs: &[PathBuf], argv: &[String]) -> anyhow::Result<ExitStatus> {
    /// The most said in one run, as on Linux.
    const MOST_SAID: usize = 50;
    let first = argv.first().context("nothing to run")?;
    let watch = stream::Stream::start();
    let mut child = link_secretive::process::carried(first)
        .args(&argv[1..])
        .spawn()
        .with_context(|| format!("running {first}"))?;
    let root = child.id();
    if let Some(watch) = &watch {
        watch.run_is(root);
    }
    link_secretive::process::stand_in_front(root, link_secretive::process::Terminal::Reaches);
    // Waited on directly: its end wakes the wait for reports at once.
    let (done, exited) = std::sync::mpsc::channel();
    let waker = watch.as_ref().map(stream::Stream::waker);
    std::thread::spawn(move || {
        let _ = done.send(child.wait());
        if let Some(waker) = waker {
            let _ = waker.send(None);
        }
    });
    let mut said: Vec<PathBuf> = Vec::new();
    let refused = std::cell::Cell::new(false);
    let mut say = |report: &reports::Report| {
        let Some(program) = report.program() else {
            return;
        };
        refused.set(refused.get() || !report.allowed);
        if report.allowed
            || said.len() >= MOST_SAID
            || said.iter().any(|p| p == program)
            || crate::programs::listed(programs, program)
            || !stream::ours(report.pid, root)
        {
            return;
        }
        link_format::aside(link_format::Note(link_format::Sentence::adopt(
            &reports::refusal(program),
        )));
        said.push(program.to_path_buf());
    };
    let status = loop {
        match &watch {
            Some(watch) => {
                if let Some(report) = watch.next(Duration::from_secs(1)) {
                    say(&report);
                }
            }
            None => break exited.recv()??,
        }
        if let Ok(status) = exited.try_recv() {
            break status?;
        }
    };
    if let Some(watch) = watch {
        // A second more for reports on their way, unless the program
        // ended well with none refused.
        let late = match status.success() && !refused.get() {
            true => Duration::ZERO,
            false => Duration::from_secs(1),
        };
        for report in watch.finish(late) {
            say(&report);
        }
    }
    Ok(status)
}

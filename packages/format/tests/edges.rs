//! Format's edge cases: Ctrl-C ends a counted flow at its step, with exit
//! 130 and no record, and a flag a command declares secret shows as
//! `<secret>` in its record's command line.

link_test_kit::program!(system);

use std::time::Duration;

use link_format::{Machine, Out, Record, Report, Steps, phrase, sentence};
use link_test_kit::{TempDir, env, process, wait};

const CHILD: &str = "flow_that_waits_for_ctrl_c";
const RECORDS: &str = "LNK_FLOW_RECORDS";
const READY: &str = "LNK_FLOW_READY";

/// A flow that has reached its second step of three and waits there: run
/// only by [`ctrl_c_ends_a_flow_at_its_step`], which starts it.
#[test]
#[ignore = "the child of ctrl_c_ends_a_flow_at_its_step"]
fn flow_that_waits_for_ctrl_c() {
    let records = std::env::var_os(RECORDS).map(std::path::PathBuf::from);
    let ready = std::env::var_os(READY)
        .map(std::path::PathBuf::from)
        .unwrap();
    let mut out = Out::with(Machine::new(Default::default(), records));
    let mut steps = Steps::new(3);
    out.step(&mut steps, phrase!("Checking the model"));
    out.step(&mut steps, phrase!("Pulling {}", "qwen3:8b"));
    std::fs::write(ready, "").unwrap();
    // Ctrl-C ends the process here, as it would a person's wait.
    loop {
        std::thread::park();
    }
}

const MODE: &str = "LNK_FLOW_MODE";
const OBSERVED: &str = "LNK_FLOW_OBSERVED";
static OLD_RAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn old_handler(_: libc::c_int) {
    OLD_RAN.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// A flow in the mode `LNK_FLOW_MODE` names, with Ctrl-C in the state that
/// mode starts it in: run only by the tests below, which start it.
/// `handed-over` lends the terminal after its step; `own-handler` starts
/// with a handler of its own; `ignored` is started with Ctrl-C ignored.
#[test]
#[ignore = "the child of the Ctrl-C tests below"]
fn flow_with_ctrl_c_in_a_state() {
    let mode = std::env::var(MODE).unwrap();
    let ready = std::path::PathBuf::from(std::env::var_os(READY).unwrap());
    let observed = std::path::PathBuf::from(std::env::var_os(OBSERVED).unwrap());
    if mode == "own-handler" {
        // SAFETY: the handler only stores to an atomic.
        unsafe {
            libc::signal(
                libc::SIGINT,
                old_handler as extern "C" fn(libc::c_int) as usize,
            )
        };
    }
    let mut out = Out::with(Machine::new(Default::default(), None));
    let mut steps = Steps::new(2);
    out.step(&mut steps, phrase!("Checking"));
    out.step(&mut steps, phrase!("Running"));
    if mode != "ignored" {
        let _lent = out.hand_over();
    }
    // SAFETY: sigaction with no new action only reads the current one.
    let ignored = unsafe {
        let mut now: libc::sigaction = std::mem::zeroed();
        libc::sigaction(libc::SIGINT, std::ptr::null(), &mut now);
        now.sa_sigaction == libc::SIG_IGN
    };
    std::fs::write(&ready, "").unwrap();
    if mode == "ignored" {
        std::fs::write(&observed, if ignored { "ignored" } else { "taken" }).unwrap();
        return;
    }
    while !OLD_RAN.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    std::fs::write(&observed, "old handler").unwrap();
}

/// Runs [`flow_with_ctrl_c_in_a_state`] in `mode`, and interrupts it once
/// ready when `interrupt`; its status, standard error and what it observed.
fn run_flow(
    mode: &str,
    interrupt: bool,
    start_ignoring: bool,
) -> (std::process::ExitStatus, String, String) {
    use std::os::unix::process::CommandExt;
    let home = TempDir::new("flow-state");
    let ready = home.join("ready");
    let observed = home.join("observed");
    let mut command = env::command(std::env::current_exe().unwrap(), home.path());
    command
        .args([
            "--ignored",
            "--exact",
            "flow_with_ctrl_c_in_a_state",
            "--nocapture",
        ])
        .env(MODE, mode)
        .env(READY, &ready)
        .env(OBSERVED, &observed)
        .stderr(std::process::Stdio::piped())
        .process_group(0);
    if start_ignoring {
        // SAFETY: signal is async-signal-safe, as pre_exec needs.
        unsafe {
            command.pre_exec(|| {
                libc::signal(libc::SIGINT, libc::SIG_IGN);
                Ok(())
            });
        }
    }
    let mut child = Child(command.spawn().unwrap());
    wait::appears("the flow's last step", &ready, Duration::from_secs(30));
    if interrupt {
        process::interrupt(&child.0);
    }
    let status = wait::exited("the flow ending", &mut child.0, Duration::from_secs(30));
    let stderr = std::io::read_to_string(child.0.stderr.take().unwrap()).unwrap();
    let observed = std::fs::read_to_string(&observed).unwrap_or_default();
    (status, stderr, observed)
}

#[test]
fn ctrl_c_after_the_terminal_is_lent_is_not_a_stop_of_the_flow() {
    use std::os::unix::process::ExitStatusExt;
    let (status, stderr, _) = run_flow("handed-over", true, false);
    assert_eq!(status.signal(), Some(libc::SIGINT), "{status:?}");
    assert!(!stderr.contains("Stopped at step"), "{stderr}");
}

#[test]
fn ctrl_c_goes_to_the_handler_the_process_had() {
    let (status, stderr, observed) = run_flow("own-handler", true, false);
    assert_eq!(status.code(), Some(0), "{status:?} {stderr}");
    assert_eq!(observed, "old handler");
}

#[test]
fn ctrl_c_started_ignored_stays_ignored() {
    let (status, stderr, observed) = run_flow("ignored", false, true);
    assert_eq!(status.code(), Some(0), "{status:?} {stderr}");
    assert_eq!(observed, "ignored");
}

/// Kills the child's group when a failed assertion leaves it running.
struct Child(std::process::Child);

impl Drop for Child {
    fn drop(&mut self) {
        process::kill_group(&mut self.0);
    }
}

#[test]
fn ctrl_c_ends_a_flow_at_its_step() {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    let home = TempDir::new("flow-ctrl-c");
    let records = home.join("records");
    let ready = home.join("ready");
    let mut command = env::command(std::env::current_exe().unwrap(), home.path());
    command
        .args(["--ignored", "--exact", CHILD, "--nocapture"])
        .env(RECORDS, &records)
        .env(READY, &ready)
        .stderr(std::process::Stdio::piped())
        .process_group(0);
    let mut child = Child(command.spawn().unwrap());
    wait::appears("the flow's second step", &ready, Duration::from_secs(30));
    process::interrupt(&child.0);
    let status = wait::exited("the flow ending", &mut child.0, Duration::from_secs(30));
    assert_eq!(status.code(), Some(130), "{status:?} {:?}", status.signal());
    let stderr = std::io::read_to_string(child.0.stderr.take().unwrap()).unwrap();
    assert!(
        stderr.contains("Stopped at step 2 of 3: Pulling qwen3:8b."),
        "{stderr}"
    );
    assert!(stderr.contains("start again"), "{stderr}");
    assert!(!records.exists() || std::fs::read_dir(&records).unwrap().next().is_none());
}

#[test]
fn a_secret_flags_value_never_reaches_a_record() {
    let home = TempDir::new("secret-record");
    let (mut out, _) = Out::capture(false, &["lnk", "agent", "connect", "--token", "s3cret"]);
    out.secret(&["--token"]);
    assert_eq!(out.typed().text(), "lnk agent connect --token <secret>");
    let report = Report::error(sentence!("Couldn't log in."));
    let path = Record::new(&report, out.typed())
        .write(&home.join("records"))
        .unwrap();
    let record = std::fs::read_to_string(path).unwrap();
    assert!(record.contains("<secret>"), "{record}");
    assert!(!record.contains("s3cret"), "{record}");
}

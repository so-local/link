//! A system test program's runner: each test in a process of its own.
//!
//! The program, declared `program!(system)`, is started by Cargo as any
//! test program is. Before libtest's `main`, [`run`] lists its tests by
//! starting the program again (`--list`), then starts it once more for
//! each selected test, `--exact <test>`, with [`ONE`](super::ONE) naming
//! the test, so that process is libtest as usual, running only it. As
//! many run at once as libtest would run threads.
//!
//! Each test's process is the leader of a process group of its own, with
//! a home and runtime folder made new for it. Its temp folder is the
//! run's, where libtest's thread would have its own, since every
//! `TempDir` there is named apart. When it ends,
//! or reaches its deadline, its group is ended, and on Linux every
//! process that still names its home too. The runner then prints what
//! libtest prints, a line per test, each failure's output and the one
//! summary, so whoever reads a run's output reads it as any other.

use std::collections::{BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::{ONE, settings};

/// What a run was asked, in libtest's own flags.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    filters: Vec<String>,
    skip: Vec<String>,
    exact: bool,
    list: bool,
    ignored: bool,
    include_ignored: bool,
    show_output: bool,
    nocapture: bool,
    quiet: bool,
    threads: Option<usize>,
}

impl Options {
    /// libtest's flags as Cargo and a person pass them; a flag this runner
    /// doesn't run as libtest would is refused by name, never ignored.
    pub fn parse(args: &[String]) -> Result<Options, String> {
        let mut options = Options::default();
        let mut at = 0;
        while at < args.len() {
            let arg = &args[at];
            at += 1;
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_string())),
                _ => (arg.as_str(), None),
            };
            let mut value = || -> Result<String, String> {
                if let Some(value) = inline.clone() {
                    return Ok(value);
                }
                let value = args
                    .get(at)
                    .cloned()
                    .ok_or(format!("{flag} needs a value"))?;
                at += 1;
                Ok(value)
            };
            match flag {
                "--exact" => options.exact = true,
                "--list" => options.list = true,
                "--ignored" => options.ignored = true,
                "--include-ignored" => options.include_ignored = true,
                "--show-output" => options.show_output = true,
                "--nocapture" | "--no-capture" => options.nocapture = true,
                "-q" | "--quiet" => options.quiet = true,
                "--skip" => options.skip.push(value()?),
                "--test-threads" => {
                    let given = value()?;
                    let threads = given
                        .parse::<usize>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or(format!(
                            "--test-threads must be a positive number, not {given}"
                        ))?;
                    options.threads = Some(threads);
                }
                "--format" => match value()?.as_str() {
                    "pretty" => {}
                    "terse" => options.quiet = true,
                    other => return Err(format!("a system test program has no --format {other}")),
                },
                "--color" => {
                    value()?;
                }
                flag if flag.starts_with('-') => {
                    return Err(format!("a system test program has no {flag}"));
                }
                _ => options.filters.push(arg.clone()),
            }
        }
        Ok(options)
    }

    /// Whether these select at most one test, by its whole name.
    fn one_named(&self) -> bool {
        self.exact && self.filters.len() == 1 && !self.list
    }

    /// Whether `name` is selected by the filters and not skipped.
    fn selects(&self, name: &str) -> bool {
        let matches = |pattern: &String| {
            if self.exact {
                name == pattern
            } else {
                name.contains(pattern.as_str())
            }
        };
        (self.filters.is_empty() || self.filters.iter().any(matches))
            && !self.skip.iter().any(matches)
    }
}

/// Whether the program was started for at most one test, by its whole
/// name (`--exact` and one name): that test is alone in this process
/// already, so libtest runs it as usual.
pub fn one_named() -> bool {
    Options::parse(&arguments()).is_ok_and(|options| options.one_named())
}

/// Runs the program's selected tests, each in a process of its own, and
/// answers the exit code libtest would: 0 when none failed, else 101.
pub fn run() -> i32 {
    let args = arguments();
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(error) => {
            eprintln!("error: this test program can't name itself: {error}");
            return 101;
        }
    };
    let options = match Options::parse(&args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("error: {error}");
            return 101;
        }
    };
    if options.list || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return relay(&program, &args);
    }
    let (all, ignored) = match (listed(&program, false), listed(&program, true)) {
        (Ok(all), Ok(ignored)) => (all, ignored.into_iter().collect::<BTreeSet<_>>()),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("error: {error}");
            return 101;
        }
    };
    let started = Instant::now();
    let selected: Vec<&String> = all.iter().filter(|name| options.selects(name)).collect();
    let mut counts = crate::libtest::Counts {
        filtered: all.len() - selected.len(),
        ..Default::default()
    };
    let mut queue = VecDeque::new();
    let mut out = std::io::stdout();
    let plural = if selected.len() == 1 { "" } else { "s" };
    let _ = writeln!(out, "\nrunning {} test{plural}", selected.len());
    for name in selected {
        let is_ignored = ignored.contains(name);
        let runs = if options.ignored {
            is_ignored
        } else {
            options.include_ignored || !is_ignored
        };
        if runs {
            queue.push_back(name.clone());
        } else if options.ignored {
            counts.filtered += 1;
        } else {
            counts.ignored += 1;
            line(&mut out, &options, name, "ignored");
        }
    }
    let threads = options
        .threads
        .or_else(|| std::env::var("RUST_TEST_THREADS").ok()?.parse().ok())
        .filter(|n: &usize| *n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let outcomes = run_all(&program, queue, threads, &options);
    let mut successes = Vec::new();
    let mut failures = Vec::new();
    for outcome in outcomes {
        if outcome.passed {
            counts.passed += 1;
            successes.push(outcome);
        } else {
            counts.failed += 1;
            failures.push(outcome);
        }
    }
    if options.show_output && !options.nocapture {
        section(&mut out, "successes", &successes);
    }
    if !options.nocapture {
        section(&mut out, "failures", &failures);
    }
    let _ = writeln!(out, "\n{}\n", counts.summary(started.elapsed()));
    let _ = out.flush();
    if counts.failed == 0 { 0 } else { 101 }
}

/// One test's run.
struct Outcome {
    name: String,
    passed: bool,
    /// What its process printed, both streams in order.
    said: String,
    /// Why the runner failed it, past what the test said, such as its
    /// deadline.
    note: Option<String>,
}

/// Runs `queue` on `threads` workers, printing each test's line as it
/// ends; the outcomes in the order they ended.
fn run_all(
    program: &Path,
    queue: VecDeque<String>,
    threads: usize,
    options: &Options,
) -> Vec<Outcome> {
    let total = queue.len();
    let queue = Arc::new(Mutex::new(queue));
    let (sent, ended) = mpsc::channel();
    let workers: Vec<_> = (0..threads.min(total))
        .map(|_| {
            let queue = Arc::clone(&queue);
            let sent = sent.clone();
            let program = program.to_path_buf();
            std::thread::spawn(move || {
                loop {
                    let next = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                    let Some(name) = next else { break };
                    if sent.send(one(&program, &name)).is_err() {
                        break;
                    }
                }
            })
        })
        .collect();
    drop(sent);
    let mut out = std::io::stdout();
    let mut outcomes = Vec::with_capacity(total);
    for outcome in ended {
        let verdict = if outcome.passed { "ok" } else { "FAILED" };
        line(&mut out, options, &outcome.name, verdict);
        if options.nocapture {
            let _ = write!(out, "{}", outcome.said);
        }
        // A failure's note is printed with its output; a passing test's,
        // such as what it left running, is printed at once.
        if let Some(note) = outcome
            .note
            .as_ref()
            .filter(|_| options.nocapture || outcome.passed)
        {
            let _ = writeln!(out, "{note}");
        }
        outcomes.push(outcome);
    }
    for worker in workers {
        let _ = worker.join();
    }
    outcomes
}

/// Runs `name` in a process of its own and ends whatever it started.
fn one(program: &Path, name: &str) -> Outcome {
    let settings = settings();
    let home = crate::TempDir::short_out_of_tmp("t");
    let failed = |note: String| Outcome {
        name: name.to_string(),
        passed: false,
        said: String::new(),
        note: Some(note),
    };
    let runtime = crate::env::made_runtime(home.path());
    let (mut reader, writer) = match std::io::pipe() {
        Ok(pipe) => pipe,
        Err(error) => return failed(format!("its output pipe can't be made: {error}")),
    };
    let copy = match writer.try_clone() {
        Ok(copy) => copy,
        Err(error) => return failed(format!("its output pipe can't be made: {error}")),
    };
    let mut command = itself(program);
    command
        .args(["--exact", name, "--nocapture", "--test-threads", "1"])
        .arg("--include-ignored")
        .env(ONE, name)
        .env("HOME", home.path())
        .env("XDG_RUNTIME_DIR", &runtime)
        .stdin(std::process::Stdio::null())
        .stdout(copy)
        .stderr(writer);
    let group = crate::ChildGroup::spawn(&mut command);
    // The runner's copies of the pipe's writer go with the command, so its
    // end is the test's processes ending.
    drop(command);
    let group = match group {
        Ok(group) => group,
        Err(error) => return failed(format!("its process can't start: {error}")),
    };
    let said = Arc::new(Mutex::new(Vec::new()));
    let (read_all, drained) = mpsc::channel::<()>();
    {
        let said = Arc::clone(&said);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                said.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .extend_from_slice(&chunk[..n]);
            }
            let _ = read_all.send(());
        });
    }
    let exited = exit_of(group.id());
    let deadline = crate::wait::scaled(Duration::from_secs(settings.deadline));
    let slow = Duration::from_secs(settings.slow).min(deadline);
    let mut note = None;
    if exited.recv_timeout(slow).is_err() {
        let mut out = std::io::stdout();
        let _ = writeln!(
            out,
            "test {name} has been running for over {} seconds",
            slow.as_secs()
        );
        if exited.recv_timeout(deadline - slow).is_err() {
            note = Some(format!(
                "the runner ended {name} at its deadline of {:.0}s (program.toml's deadline, times LNK_TEST_TIME_SCALE)",
                deadline.as_secs_f64()
            ));
        }
    }
    let status = group.try_status().ok().flatten();
    drop(group);
    #[cfg(target_os = "linux")]
    let left = crate::process::end_under(home.path());
    #[cfg(not(target_os = "linux"))]
    let left: Vec<String> = Vec::new();
    let _ = drained.recv_timeout(Duration::from_secs(settings.drain));
    let raw = std::mem::take(&mut *said.lock().unwrap_or_else(|e| e.into_inner()));
    drop(home);
    let raw = String::from_utf8_lossy(&raw);
    let ran = raw
        .lines()
        .any(|line| line.starts_with("test result: ") && line.contains(" 1 passed"));
    let said = own_words(&raw, name);
    let succeeded = status.is_some_and(|status| status.success());
    if note.is_none() && succeeded && !ran {
        note = Some(format!("{name}'s process ran no test"));
    }
    let passed = succeeded && ran && note.is_none();
    if !left.is_empty() {
        let line = format!(
            "{name} left these running, ended by the runner:\n  {}",
            left.join("\n  ")
        );
        note = Some(note.map_or(line.clone(), |note| format!("{note}\n{line}")));
    }
    Outcome {
        name: name.to_string(),
        passed,
        said,
        note,
    }
}

/// What a test's process printed, less libtest's own lines around it: its
/// `running 1 test`, its verdict, its failures list and its summary, which
/// the runner prints once for the whole run.
fn own_words(raw: &str, name: &str) -> String {
    let opening = format!("test {name} ... ");
    let mut said = String::new();
    for line in raw.lines() {
        if line == "failures:" || line.starts_with("test result: ") {
            break;
        }
        let line = line.strip_prefix(&opening).unwrap_or(line);
        if line == "running 1 test" || line == "ok" || line == "FAILED" {
            continue;
        }
        said.push_str(line);
        said.push('\n');
    }
    said.trim_matches('\n').to_string() + "\n"
}

/// A channel that receives once the process `pid`, this runner's child,
/// has ended, without reaping it: its group stays the test's until the
/// runner ends it.
fn exit_of(pid: u32) -> mpsc::Receiver<()> {
    let (sent, exited) = mpsc::channel();
    std::thread::spawn(move || {
        loop {
            // SAFETY: siginfo_t is a C record with no invalid all-zero fields.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: waits on this process's own child; WNOWAIT leaves
            // its exit status and process ID to its group's guard.
            let waited = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOWAIT,
                )
            };
            if waited == 0
                || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                break;
            }
        }
        let _ = sent.send(());
    });
    exited
}

/// The program's tests by name, or only its ignored ones, as libtest
/// lists them.
fn listed(program: &Path, ignored: bool) -> Result<Vec<String>, String> {
    let mut command = itself(program);
    command
        .args(["--list", "--format", "terse"])
        .env(ONE, "--list");
    if ignored {
        command.arg("--ignored");
    }
    let output = command
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("this test program couldn't list its tests: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "this test program couldn't list its tests: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .map(str::to_string)
        .collect())
}

/// Runs the program once as libtest, with `args`, for what the runner
/// leaves to it: listing and help.
fn relay(program: &Path, args: &[String]) -> i32 {
    match itself(program).args(args).env(ONE, "--list").status() {
        Ok(status) => status.code().unwrap_or(101),
        Err(error) => {
            eprintln!("error: this test program couldn't start again: {error}");
            101
        }
    }
}

/// This test program, to start again with the run's whole environment,
/// as libtest's own thread for the test would have it.
#[expect(
    clippy::disallowed_methods,
    reason = "the test program started again for its own test keeps the run's environment whole, as a libtest thread would"
)]
fn itself(program: &Path) -> std::process::Command {
    std::process::Command::new(program)
}

/// libtest's line for a test's verdict, or its one character when quiet.
fn line(out: &mut impl Write, options: &Options, name: &str, verdict: &str) {
    if options.quiet {
        let mark = match verdict {
            "ok" => ".",
            "ignored" => "i",
            _ => "F",
        };
        let _ = write!(out, "{mark}");
    } else {
        let _ = writeln!(out, "test {name} ... {verdict}");
    }
    let _ = out.flush();
}

/// libtest's section of each test's output, then their names.
fn section(out: &mut impl Write, title: &str, outcomes: &[Outcome]) {
    if outcomes.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n{title}:\n");
    for outcome in outcomes {
        let _ = writeln!(out, "---- {} stdout ----", outcome.name);
        let _ = write!(out, "{}", outcome.said);
        if let Some(note) = &outcome.note {
            let _ = writeln!(out, "{note}");
        }
        let _ = writeln!(out);
    }
    let _ = writeln!(out, "\n{title}:");
    for outcome in outcomes {
        let _ = writeln!(out, "    {}", outcome.name);
    }
}

/// The arguments the program was started with, past its name. Read from
/// the system, since a constructor runs before Rust's runtime keeps them
/// everywhere.
fn arguments() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read("/proc/self/cmdline")
            .map(|bytes| {
                bytes
                    .split(|byte| *byte == 0)
                    .skip(1)
                    .filter(|arg| !arg.is_empty())
                    .map(|arg| String::from_utf8_lossy(arg).into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        given()
            .into_iter()
            .skip(1)
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }
}

#[cfg(not(target_os = "linux"))]
#[expect(
    clippy::disallowed_methods,
    reason = "the testing kit's runner reads libtest's own arguments before libtest's main"
)]
fn given() -> Vec<std::ffi::OsString> {
    std::env::args_os().collect()
}

#[cfg(test)]
mod tests {
    use super::Options;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn libtests_flags_read_as_libtest_reads_them() {
        let options = parse(&["boxes::", "--exact", "--skip", "slow", "--test-threads=4"]).unwrap();
        assert_eq!(options.filters, ["boxes::"]);
        assert_eq!(options.skip, ["slow"]);
        assert!(options.exact);
        assert_eq!(options.threads, Some(4));
        assert!(parse(&["--format", "terse"]).unwrap().quiet);
    }

    #[test]
    fn a_flag_the_runner_does_not_run_is_refused_by_name() {
        let error = parse(&["--format", "json"]).unwrap_err();
        assert!(error.contains("json"), "{error}");
        let error = parse(&["--bench"]).unwrap_err();
        assert!(error.contains("--bench"), "{error}");
        assert!(parse(&["--test-threads", "0"]).is_err());
    }

    #[test]
    fn a_tests_own_words_leave_libtests_lines_out() {
        let raw = "\nrunning 1 test\ntest a::b ... thread 'a::b' panicked at x.rs:1:\nboom\nFAILED\n\nfailures:\n\nfailures:\n    a::b\n\ntest result: FAILED. 0 passed; 1 failed\n";
        assert_eq!(
            super::own_words(raw, "a::b"),
            "thread 'a::b' panicked at x.rs:1:\nboom\n"
        );
    }

    #[test]
    fn one_test_named_whole_runs_as_libtest_runs_it() {
        assert!(
            parse(&["--exact", "a::b", "--nocapture"])
                .unwrap()
                .one_named()
        );
        assert!(!parse(&["a::b"]).unwrap().one_named());
        assert!(!parse(&["--exact", "a::b", "a::c"]).unwrap().one_named());
        assert!(!parse(&["--exact", "a::b", "--list"]).unwrap().one_named());
    }

    #[test]
    fn a_test_is_selected_as_libtest_selects_it() {
        let options = parse(&["moving::"]).unwrap();
        assert!(options.selects("moving::boxes::one_box"));
        assert!(!options.selects("ids::moved"));
        let exact = parse(&["--exact", "a::b", "--skip", "a::b"]).unwrap();
        assert!(!exact.selects("a::b"));
        assert!(parse(&[]).unwrap().selects("anything"));
    }
}

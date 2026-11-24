//! What Seatbelt reports of the programs a sandbox runs (macOS), read
//! from macOS's unified log: `log stream`, while the sandbox runs.
//!
//! Seatbelt writes a line there for each operation a profile refuses
//! (`Sandbox: bash(41520) deny(1) process-exec* /usr/bin/id`), and for one
//! it allows `(with report)`, as `lnk sandbox learn`'s profile has every
//! program run. Its lines name the process and its id, not the sandbox,
//! so a line is taken for this run's only if its process is in this
//! run's tree, or gone by the time it's read (a program refused usually
//! exits at once): another sandbox's refusal at the same moment can be
//! named too. The reading of each line is here, on every OS, so it's
//! tested everywhere; the log itself is the Mac's ([`Stream`]).

use std::path::{Path, PathBuf};

/// One of Seatbelt's reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The process's name, as the kernel keeps it (16 characters at most).
    pub process: String,
    pub pid: u32,
    /// Allowed (a rule `(with report)`), or refused.
    pub allowed: bool,
    /// `process-exec*`, `file-read-data`, `network-outbound`, ...
    pub operation: String,
    /// What it was for: a path, an address; the rest of the line.
    pub target: String,
}

impl Report {
    /// The program it starts, for a report of running one.
    pub fn program(&self) -> Option<&Path> {
        (self.operation.starts_with("process-exec") && self.target.starts_with('/'))
            .then(|| Path::new(&self.target))
    }
}

/// The report in one line of `log stream` (any style: `syslog`,
/// `compact`, `default`, or `ndjson`'s `eventMessage`), if it holds one.
pub fn parse(line: &str) -> Option<Report> {
    let line = line.trim_end_matches(['\r', '\n']);
    let message = match line.trim_start().starts_with('{') {
        true => {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            value.get("eventMessage")?.as_str()?.to_string()
        }
        false => line.to_string(),
    };
    let at = message.find("Sandbox: ")?;
    let rest = &message[at + "Sandbox: ".len()..];
    // `<name>(<pid>) <deny|allow>(<count>) <operation> <target>`: the name
    // may hold spaces and parentheses, the target spaces.
    let (verdict_at, allowed) = [(") deny(", false), (") allow(", true)]
        .iter()
        .filter_map(|(word, allowed)| rest.find(word).map(|i| (i, *allowed)))
        .min_by_key(|(i, _)| *i)?;
    let who = &rest[..verdict_at];
    let open = who.rfind('(')?;
    let pid: u32 = who[open + 1..].parse().ok()?;
    let process = who[..open].to_string();
    let after = &rest[verdict_at + 2..];
    let after = after
        .strip_prefix("deny(")
        .or_else(|| after.strip_prefix("allow("))?;
    let close = after.find(')')?;
    after[..close].parse::<u32>().ok()?;
    let after = after[close + 1..].trim_start();
    let (operation, target) = match after.split_once(' ') {
        Some((op, target)) => (op, target.trim()),
        None => (after, ""),
    };
    (!operation.is_empty()).then(|| Report {
        process,
        pid,
        allowed,
        operation: operation.to_string(),
        target: target.to_string(),
    })
}

/// What a refused program is told, as on Linux: which file, and the fix.
pub fn refusal(program: &Path) -> String {
    format!(
        "lnk sandbox: refused {}: add it to [commands] allow",
        program.display()
    )
}

/// The `log` arguments streaming Seatbelt's reports of programs run, as
/// one line each: only Seatbelt's own (its sender), never a line another
/// program wrote to the log in its words (`logger`).
pub fn stream_args() -> Vec<String> {
    [
        "stream",
        "--style",
        "syslog",
        "--predicate",
        r#"sender == "Sandbox" AND eventMessage CONTAINS "Sandbox: " AND eventMessage CONTAINS " process-exec""#,
    ]
    .map(String::from)
    .to_vec()
}

/// Of `reports` (in the order read), the programs this run started, by
/// `ours` (whether a process id is this run's, or gone), each once:
/// those it refused (`allowed` false) or every one (true, for `learn`).
pub fn programs(reports: &[Report], allowed: bool, ours: impl Fn(u32) -> bool) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for report in reports {
        let Some(program) = report.program() else {
            continue;
        };
        if (allowed || !report.allowed) && ours(report.pid) && !out.iter().any(|p| p == program) {
            out.push(program.to_path_buf());
        }
    }
    out
}

/// As [`crate::programs::learned`] reads them: one line per program,
/// the path given and the file, here the same.
pub fn learn_lines(programs: &[PathBuf]) -> String {
    programs
        .iter()
        .map(|p| format!("{}\t{}\n", p.display(), p.display()))
        .collect()
}

/// macOS's log, streaming Seatbelt's reports of programs run while a
/// sandbox runs, and whose processes are this run's.
#[cfg(target_os = "macos")]
pub mod stream {
    use super::{Report, parse, stream_args};
    use std::io::BufRead;
    use std::process::{Child, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    /// The most reports kept for [`Stream::finish`]: a long run's log,
    /// which holds every sandbox's on this Mac, can't grow without end.
    const MAX_KEPT: usize = 10_000;

    /// `log stream` running, its reports as they're read.
    pub struct Stream {
        child: Child,
        /// Each report read; None: woken ([`Stream::waker`]).
        reports: mpsc::Receiver<Option<Report>>,
        waker: mpsc::Sender<Option<Report>>,
        /// What it has read so far, kept for [`Stream::finish`].
        kept: Arc<Mutex<Vec<Report>>>,
        /// The run's first process, once started ([`Stream::run_is`]): 0
        /// before, when every report is kept.
        root: Arc<AtomicU32>,
    }

    impl Stream {
        /// Starts `log stream` and waits until it reads (its first line,
        /// or two seconds): a sandbox started before would go unseen. None
        /// where it can't (`log stream` needs an admin user).
        pub fn start() -> Option<Stream> {
            let mut child = link_secretive::process::command("/usr/bin/log")
                .args(stream_args())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            let stdout = child.stdout.take()?;
            let (lines, reports) = mpsc::channel();
            let waker = lines.clone();
            let (ready, started) = mpsc::channel();
            let kept = Arc::new(Mutex::new(Vec::new()));
            let keep = kept.clone();
            let root = Arc::new(AtomicU32::new(0));
            let run = root.clone();
            std::thread::spawn(move || {
                let mut first = Some(ready);
                for line in std::io::BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if let Some(ready) = first.take() {
                        let _ = ready.send(());
                    }
                    if let Some(report) = parse(&line) {
                        // Another run's never takes the room of this one's.
                        let root = run.load(Ordering::Relaxed);
                        if (root == 0 || ours(report.pid, root))
                            && let Ok(mut kept) = keep.lock()
                            && kept.len() < MAX_KEPT
                        {
                            kept.push(report.clone());
                        }
                        let _ = lines.send(Some(report));
                    }
                }
            });
            let _ = started.recv_timeout(Duration::from_secs(2));
            if matches!(child.try_wait(), Ok(Some(_))) {
                return None;
            }
            Some(Stream {
                child,
                reports,
                waker,
                kept,
                root,
            })
        }

        /// The run's first process is `pid`: from now on, only reports
        /// of it or a process under it (or gone) are kept, and those
        /// kept before of another's let go.
        pub fn run_is(&self, pid: u32) {
            self.root.store(pid, Ordering::Relaxed);
            if let Ok(mut kept) = self.kept.lock() {
                kept.retain(|report| ours(report.pid, pid));
            }
        }

        /// The next report read, waiting at most `wait`, or None at once
        /// when woken.
        pub fn next(&self, wait: Duration) -> Option<Report> {
            self.reports.recv_timeout(wait).ok().flatten()
        }

        /// What wakes [`Stream::next`]: sending it None.
        pub fn waker(&self) -> mpsc::Sender<Option<Report>> {
            self.waker.clone()
        }

        /// Every report read, after `late` more for those still on their
        /// way, and `log stream` stopped.
        #[expect(
            clippy::disallowed_methods,
            reason = "the system log writes a report some time after the program it reports, and \
                      says nothing when it has none left: only time lets the late ones arrive"
        )]
        pub fn finish(mut self, late: Duration) -> Vec<Report> {
            if !late.is_zero() {
                std::thread::sleep(late);
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.kept.lock().map(|k| k.clone()).unwrap_or_default()
        }
    }

    /// Whether process `pid` is `root` or under it, or is gone (a program
    /// refused, or one that ran and exited, before its line was read).
    pub fn ours(pid: u32, root: u32) -> bool {
        let mut at = pid;
        for _ in 0..64 {
            if at == root {
                return true;
            }
            match parent(at) {
                Some(0) | Some(1) => return false,
                Some(next) => at = next,
                // Gone (or not ours to see): the first one only.
                None => return at == pid,
            }
        }
        false
    }

    /// The parent of process `pid`, if it's there.
    fn parent(pid: u32) -> Option<u32> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: `info` is a proc_bsdinfo of `size` bytes.
        let read = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        (read == size).then_some(info.pbi_ppid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_program_is_read_from_each_style() {
        let lines = [
            // `--style syslog`
            "2026-10-02 10:11:12.345678+0200  localhost kernel[0]: (Sandbox) Sandbox: bash(41520) deny(1) process-exec* /usr/bin/id",
            // `--style compact`
            "2026-10-02 10:11:12.345 E  kernel[0:1a2b3] (Sandbox) Sandbox: bash(41520) deny(1) process-exec* /usr/bin/id",
            // `--style default`
            "2026-10-02 10:11:12.345678+0200 0x1a2b3    Error       0x0                  0      0    kernel: (Sandbox) Sandbox: bash(41520) deny(1) process-exec* /usr/bin/id",
            // `--style ndjson`
            r#"{"eventMessage":"Sandbox: bash(41520) deny(1) process-exec* /usr/bin/id","processID":0,"senderImagePath":"/System/Library/Extensions/Sandbox.kext/Contents/MacOS/Sandbox"}"#,
        ];
        for line in lines {
            let report = parse(line).unwrap_or_else(|| panic!("{line}"));
            assert_eq!(
                report,
                Report {
                    process: "bash".into(),
                    pid: 41520,
                    allowed: false,
                    operation: "process-exec*".into(),
                    target: "/usr/bin/id".into(),
                }
            );
            assert_eq!(report.program(), Some(Path::new("/usr/bin/id")));
        }
    }

    #[test]
    fn names_and_paths_with_spaces_and_counts_are_read() {
        let report = parse(
            "kernel[0]: (Sandbox) Sandbox: Code Helper (R)(512) deny(3) process-exec* /Users/me/My Tools/run (2).sh",
        )
        .unwrap();
        assert_eq!(report.process, "Code Helper (R)");
        assert_eq!(report.pid, 512);
        assert_eq!(report.target, "/Users/me/My Tools/run (2).sh");
        let allowed = parse(
            "kernel[0]: (Sandbox) Sandbox: npm(77) allow(1) process-exec* /usr/local/bin/node",
        )
        .unwrap();
        assert!(allowed.allowed);
        assert_eq!(allowed.program(), Some(Path::new("/usr/local/bin/node")));
    }

    #[test]
    fn other_lines_are_no_program() {
        for line in [
            "Filtering the log data using \"eventMessage CONTAINS ...\"",
            "kernel[0]: (Sandbox) Sandbox: curl(5) deny(1) network-outbound 1.1.1.1:443",
            "kernel[0]: (Sandbox) Sandbox: 1 duplicate report for bash deny process-exec* /usr/bin/id",
            "kernel[0]: (Sandbox) Sandbox: bash(x) deny(1) process-exec* /usr/bin/id",
            "kernel[0]: System Policy: bash(5) deny(1) file-read-data /x",
            "{\"eventMessage\": 5}",
            "",
        ] {
            assert!(
                parse(line)
                    .and_then(|r| r.program().map(Path::to_path_buf))
                    .is_none(),
                "{line}"
            );
        }
    }

    #[test]
    fn this_runs_programs_each_once() {
        let line = |pid: u32, verdict: &str, path: &str| {
            parse(&format!(
                "Sandbox: sh({pid}) {verdict}(1) process-exec* {path}"
            ))
            .unwrap()
        };
        let reports = [
            line(10, "allow", "/bin/sh"),
            line(11, "deny", "/usr/bin/id"),
            line(12, "deny", "/usr/bin/id"),
            line(99, "deny", "/usr/bin/other"),
            line(13, "deny", "/usr/bin/git"),
        ];
        let ours = |pid: u32| pid != 99;
        assert_eq!(
            programs(&reports, false, ours),
            [PathBuf::from("/usr/bin/id"), "/usr/bin/git".into()]
        );
        let learned = programs(&reports, true, ours);
        assert_eq!(
            learn_lines(&learned),
            "/bin/sh\t/bin/sh\n/usr/bin/id\t/usr/bin/id\n/usr/bin/git\t/usr/bin/git\n"
        );
        assert_eq!(
            refusal(Path::new("/usr/bin/id")),
            "lnk sandbox: refused /usr/bin/id: add it to [commands] allow"
        );
        assert!(stream_args().iter().any(|a| a.contains("process-exec")));
        // Seatbelt's own lines only, not one another program logged.
        assert!(
            stream_args()
                .iter()
                .any(|a| a.starts_with(r#"sender == "Sandbox" AND"#))
        );
    }
}

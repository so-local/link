//! A running `lnk env here` on a box, asked over SSH: what it says, line
//! by line, each step's outcome, and stopping it when this process is
//! asked to stop.

use super::*;
use crate::stage::{Outcome, Step, Timed};
use anyhow::bail;
use link_format::{Elapsed, Name, Note, aside, sentence};

/// What `lnk env here` said about one step.
#[derive(Clone, Debug, PartialEq)]
pub enum Said {
    Stage(String, f64, crate::stage::Outcome),
    Service(String, f64),
    Running,
    Failed(String),
    /// A job's stage reached its timeout.
    TimedOut(String),
    Reset(String),
    Sample(Box<crate::profile::Sample>),
}

/// A copy of what a session shows, kept to at most `most` bytes: past
/// them it's still shown, never kept, and the copy ends saying so.
pub struct Log {
    pub file: std::fs::File,
    pub most: u64,
}

impl Log {
    /// `shown` kept as far as `most` allows; whether any was left out.
    fn keep(&mut self, shown: &[u8]) -> bool {
        if self.most == 0 {
            return !shown.is_empty();
        }
        let room = shown
            .len()
            .min(usize::try_from(self.most).unwrap_or(usize::MAX));
        let _ = self.file.write_all(&shown[..room]);
        self.most -= room as u64;
        if room < shown.len() {
            let _ = self.file.write_all(b"\n[output past this was not kept]\n");
            return true;
        }
        false
    }
}

/// One of `lnk env here`'s lines, after `mark`.
pub fn said(mark: &str, line: &str) -> Option<Said> {
    let rest = line.strip_prefix(mark)?.strip_prefix(' ')?;
    if let Some(json) = rest.strip_prefix("sample ") {
        return serde_json::from_str(json).ok().map(Said::Sample);
    }
    let mut words = rest.split(' ');
    let (word, name, secs) = (words.next()?, words.next(), words.next());
    let secs = || secs.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    Some(match (word, name) {
        ("stage", Some(name)) => {
            let outcome = match words.next() {
                Some("failed") => crate::stage::Outcome::Failed,
                Some("timed-out") => crate::stage::Outcome::TimedOut,
                _ => crate::stage::Outcome::Passed,
            };
            Said::Stage(name.into(), secs(), outcome)
        }
        ("service", Some(name)) => Said::Service(name.into(), secs()),
        ("running", None) => Said::Running,
        ("failed", Some(name)) => Said::Failed(name.into()),
        ("timed-out", Some(name)) => Said::TimedOut(name.into()),
        ("reset", Some(name)) => Said::Reset(name.into()),
        _ => return None,
    })
}

/// The most of a line held before it's shown in pieces.
const LINE_MAX: usize = 64 * 1024;

/// `bytes` as a terminal can show them without being driven: each control
/// character (an escape, a carriage return) and each one that shows
/// nothing left out, but tabs and the line's end.
fn printable(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let (body, end) = match text.strip_suffix('\n') {
        Some(body) => (body, "\n"),
        None => (text.as_ref(), ""),
    };
    let kept: Vec<String> = body.split('\t').map(clean).collect();
    format!("{}{end}", kept.join("\t")).into_bytes()
}

/// A random word of 16 hex digits.
pub fn random_hex(bytes: usize) -> anyhow::Result<String> {
    let mut buf = vec![0u8; bytes];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut buf)
        .map_err(|_| anyhow::anyhow!("no randomness"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// `lnk env here` on a box, its request sent, its output passing through
/// blanked, and what it said kept.
pub struct Session {
    child: Child,
    said: mpsc::Receiver<Said>,
    readers: Vec<std::thread::JoinHandle<()>>,
    /// Kept open: its closing stops what runs there.
    input: Option<std::process::ChildStdin>,
}

/// How a [`Session`] ended.
#[derive(Debug, Default)]
pub struct Ended {
    /// Its exit code; none when it was stopped here.
    pub code: Option<i32>,
    /// Its setup, services and stages, each timed.
    pub stages: Vec<crate::stage::Timed>,
    /// The step that failed, if one did.
    pub failed: Option<String>,
    /// Seconds from its start to its command's.
    pub setup_seconds: Option<f64>,
    pub timed_out: bool,
    pub cancelled: bool,
    /// Its profile's samples, in order, when it was profiled.
    pub samples: Vec<crate::profile::Sample>,
}

impl Session {
    pub fn start(
        machine: &str,
        read: &Read,
        action: Action,
        given: Given,
    ) -> anyhow::Result<Session> {
        let file = read.path.file_name().within("the file's name")?;
        Self::with_log(
            Some(machine),
            &read.file.name,
            Some(Path::new(file)),
            action,
            given,
            None,
        )
    }

    /// [`Session::start`] for environment `name`, its file `file` in its
    /// folder there (none, a job with no environment), what it prints
    /// written to `log` too, blanked; on this machine when `machine` is
    /// none.
    pub fn with_log(
        machine: Option<&str>,
        name: &str,
        file: Option<&Path>,
        action: Action,
        given: Given,
        log: Option<std::fs::File>,
    ) -> anyhow::Result<Session> {
        let log = log.map(|file| Log {
            file,
            most: u64::MAX,
        });
        Self::with_workdir(machine, name, file, action, given, log, None)
    }

    /// The existing executor with an explicit private home-relative workspace.
    pub fn with_workdir(
        machine: Option<&str>,
        name: &str,
        file: Option<&Path>,
        action: Action,
        given: Given,
        log: Option<Log>,
        workdir: Option<std::path::PathBuf>,
    ) -> anyhow::Result<Session> {
        Self::open(machine, name, file, action, given, log, workdir, false)
    }

    /// Environment `read` on this machine itself, as `lnk env setup
    /// --here` runs it: its own runs in their sandboxes, or `direct`ly
    /// in none ([`Request::direct`]).
    pub fn here(
        read: &Read,
        action: Action,
        given: Given,
        direct: bool,
    ) -> anyhow::Result<Session> {
        let file = read.path.file_name().within("the file's name")?;
        Self::open(
            None,
            &read.file.name,
            Some(Path::new(file)),
            action,
            given,
            None,
            None,
            direct,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "each is a separate part of the request, which the callers choose apart"
    )]
    fn open(
        machine: Option<&str>,
        name: &str,
        file: Option<&Path>,
        action: Action,
        given: Given,
        log: Option<Log>,
        workdir: Option<std::path::PathBuf>,
        direct: bool,
    ) -> anyhow::Result<Session> {
        let mark = format!("lnk-env-{}", random_hex(8)?);
        let blank = Blank::new(given.values());
        let mut command = link_core::lnk();
        if let Some(machine) = machine {
            command.args(["box", "run", machine, "--"]);
        }
        command
            .args(["env", "here"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn().within("running lnk box run")?;
        let mut input = child.stdin.take().within("no input to lnk box run")?;
        // What it's given is sealed to the key that run of `lnk env here`
        // made and said, so the stream carries none of it readable.
        let stdout = child.stdout.take().within("no output")?;
        let (seal_to, stdout) = match link_core::announced(stdout) {
            Ok(announced) => announced,
            Err(err) => {
                let _ = child.kill();
                let mut said = String::new();
                if let Some(stderr) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(
                        &mut std::io::Read::take(stderr, 4096),
                        &mut said,
                    );
                }
                let _ = child.wait();
                let said = clean(said.trim());
                bail!(
                    "lnk env here: {err:#}{}",
                    match said.is_empty() {
                        true => String::new(),
                        false => format!(": {said}"),
                    }
                );
            }
        };
        let name = name.to_string();
        let sealed = link_secretive::Sealed::seal_json(&given, &seal_to, here::about(&name))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        drop(given);
        let request = Request {
            workdir,
            name,
            file: file.map(Path::to_path_buf),
            action,
            given: sealed,
            mark: mark.clone(),
            direct,
        };
        let mut line = serde_json::to_vec(&request)?;
        line.push(b'\n');
        input
            .write_all(&line)
            .and_then(|_| input.flush())
            .within("sending the request")?;
        drop(line);
        let (tx, said) = mpsc::channel();
        let log = log.map(|f| std::sync::Arc::new(std::sync::Mutex::new(f)));
        let mut readers = Vec::new();
        let stderr = child.stderr.take().within("no errors")?;
        for (stream, is_err) in [
            (Box::new(stdout) as Box<dyn std::io::Read + Send>, false),
            (Box::new(stderr), true),
        ] {
            let (tx, mark, blank, log) = (tx.clone(), mark.clone(), blank.duplicate(), log.clone());
            // On a terminal, what the box sends can't move the cursor,
            // set the clipboard or pass for Link's own lines.
            let terminal = match is_err {
                true => link_format::streams::stderr_is_terminal(),
                false => link_format::streams::stdout_is_terminal(),
            };
            readers.push(std::thread::spawn(move || {
                let show = |shown: &[u8]| {
                    if let Some(log) = &log
                        && let Ok(mut log) = log.lock()
                    {
                        log.keep(shown);
                    }
                    let shown = match terminal {
                        true => std::borrow::Cow::Owned(printable(shown)),
                        false => std::borrow::Cow::Borrowed(shown),
                    };
                    let _ = match is_err {
                        true => link_format::streams::passed(link_format::streams::Stream::Err)
                            .write_all(&shown),
                        false => link_format::streams::passed(link_format::streams::Stream::Out)
                            .write_all(&shown),
                    };
                };
                let mut reader = BufReader::new(stream);
                let mut line: Vec<u8> = Vec::new();
                while let Ok(buf) = reader.fill_buf() {
                    if buf.is_empty() {
                        if !line.is_empty() {
                            show(&blank.bytes(&line));
                        }
                        break;
                    }
                    let Some(at) = buf.iter().position(|b| *b == b'\n') else {
                        let n = buf.len();
                        line.extend_from_slice(buf);
                        reader.consume(n);
                        // A line with no end yet (a progress bar, a
                        // binary) is shown a piece at a time, not held.
                        if line.len() > LINE_MAX {
                            let (shown, took, _) = blank.some(&line, false);
                            show(&shown);
                            line.drain(..took);
                        }
                        continue;
                    };
                    line.extend_from_slice(&buf[..at]);
                    reader.consume(at + 1);
                    let text = String::from_utf8_lossy(&line);
                    if is_err && let Some(what) = self::said(&mark, &text) {
                        let _ = tx.send(what);
                        line.clear();
                        continue;
                    }
                    let mut shown = blank.bytes(&line);
                    shown.push(b'\n');
                    show(&shown);
                    line.clear();
                }
            }));
        }
        Ok(Session {
            child,
            said,
            readers,
            input: Some(input),
        })
    }

    /// Waits for it to end, `timeout` or `cancelled` ending it first,
    /// saying each step as it's done.
    pub fn wait(
        &mut self,
        timeout: Option<Duration>,
        cancelled: &dyn Fn() -> bool,
    ) -> anyhow::Result<Ended> {
        self.wait_running(timeout, cancelled, &mut || {})
    }

    /// [`Session::wait`], `running` called once its command starts.
    #[expect(clippy::disallowed_methods, reason = "polls a starting session")]
    pub fn wait_running(
        &mut self,
        timeout: Option<Duration>,
        cancelled: &dyn Fn() -> bool,
        running: &mut dyn FnMut(),
    ) -> anyhow::Result<Ended> {
        let started = Instant::now();
        let mut ended = Ended::default();
        let status = loop {
            while let Ok(what) = self.said.try_recv() {
                if what == Said::Running {
                    running();
                }
                self.note(&mut ended, what, started);
            }
            if let Some(status) = self.child.try_wait()? {
                break Some(status);
            }
            if timeout.is_some_and(|t| started.elapsed() > t) {
                ended.timed_out = true;
                break None;
            }
            if cancelled() {
                ended.cancelled = true;
                break None;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        if status.is_none() {
            self.stop();
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        while let Ok(what) = self.said.try_recv() {
            self.note(&mut ended, what, started);
        }
        ended.code = status.and_then(|s| s.code());
        Ok(ended)
    }

    fn note(&self, ended: &mut Ended, what: Said, started: Instant) {
        match what {
            Said::Stage(name, secs, outcome) => {
                let how = match outcome {
                    Outcome::Passed => String::new(),
                    other => format!(", {}", other.word()),
                };
                aside(Note(sentence!(
                    "Stage {} took {}{}.",
                    Name::new("stage", &name),
                    Elapsed(Duration::from_secs_f64(secs)),
                    how
                )));
                ended.stages.push(Timed::stage(name, secs, outcome));
            }
            Said::Service(name, secs) => {
                aside(Note(sentence!(
                    "Service {} was ready after {}.",
                    Name::new("service", &name),
                    Elapsed(Duration::from_secs_f64(secs))
                )));
                ended.stages.push(Timed {
                    name,
                    kind: Step::Service,
                    seconds: secs,
                    outcome: Outcome::Passed,
                });
            }
            Said::Running => {
                let secs = started.elapsed().as_secs_f64();
                ended.setup_seconds = Some(secs);
                ended.stages.insert(
                    0,
                    Timed {
                        name: "setup".into(),
                        kind: Step::Setup,
                        seconds: secs,
                        outcome: Outcome::Passed,
                    },
                );
            }
            Said::Failed(name) => ended.failed = Some(name),
            Said::TimedOut(name) => {
                ended.timed_out = true;
                ended.failed = Some(name);
            }
            Said::Reset(_) => {}
            Said::Sample(sample) => ended.samples.push(*sample),
        }
    }

    /// Ends it: its input closed, so what runs there stops, then the
    /// connection itself.
    #[expect(
        clippy::disallowed_methods,
        reason = "polls a stopping session until it ends"
    )]
    pub fn stop(&mut self) {
        drop(self.input.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        #[cfg(unix)]
        // SAFETY: kill sends a signal to the group `lnk box run` leads.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGTERM)
        };
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_keeps_what_fits_and_says_what_it_left_out() {
        let dir = link_test_kit::TempDir::new("session-log");
        let path = dir.path().join("output.txt");
        let mut log = Log {
            file: std::fs::File::create(&path).unwrap(),
            most: 5,
        };
        assert!(!log.keep(b"abc"));
        assert!(log.keep(b"defg"));
        assert!(log.keep(b"more"));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"abcde\n[output past this was not kept]\n"
        );
    }

    #[test]
    fn what_reaches_a_terminal_cant_drive_it() {
        let shown = printable(b"\x1b]52;c;aGk=\x07ok\tthere\r\x1b[2Jdone\n");
        assert_eq!(shown, b"]52;c;aGk=ok\tthere[2Jdone\n");
        assert_eq!(printable(b"no end"), b"no end");
    }
}

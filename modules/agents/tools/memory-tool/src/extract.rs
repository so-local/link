//! The text of documents that aren't text: PDFs and Word documents in
//! `memory/`, extracted for the index by Link Document Converter
//! (`lnk-document-converter text show <pdf|docx>`,
//! `link_document_converter_contract`), a module of its own: never in the process holding agents' keys, and
//! never compiled into this one. The child is given none of this
//! process's secrets and no environment at all, a deadline, and limits
//! on its memory and its time on the processor; one that crashes or runs
//! past them fails that document alone. The file stays as it is; only its
//! text is indexed. A document whose text can't be had, Link Document Converter
//! not installed among the reasons, is indexed by its name alone, and
//! [`crate::Index::status`] says why.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
pub use link_document_converter_contract::{Extracted, Format, MODULE, limits as document_limits};

/// How long the child may take over a document, from its start: then
/// it's killed (`extract-deadline-secs`, [`crate::limits::Limits`]).
pub fn deadline() -> Duration {
    crate::limits::limits().extract_deadline()
}

/// The most of the child's answer read: `text-max-bytes` ([`document_limits`]) of
/// text, every byte escaped as JSON escapes the most (`\u0000`), and its
/// framing.
fn answer_max() -> u64 {
    document_limits().text_max_bytes as u64 * 6 + 1024
}

/// Why a document is found by its name alone while its text is
/// extracted in the background ([`crate::Index::refresh`]).
pub const PENDING: &str = "its text is being read, in the background";

/// The format of `path`, by its extension; none for one read as text.
pub fn format_of(path: &str) -> Option<Format> {
    Format::of(path)
}

/// Why a document is read by its name alone when Link Document Converter isn't
/// installed: the module that reads it, named.
pub fn not_installed() -> String {
    format!("reading it needs the {MODULE} module: 'lnk module add {MODULE}'")
}

/// What extracts a document's text: a program run as a child for each
/// document ([`Extractor::text`]).
#[derive(Clone, Debug)]
pub struct Extractor {
    /// Run as `<program> text <format>`; none, Link Document Converter where it's
    /// installed, found again for each document.
    program: Option<PathBuf>,
    deadline: Duration,
    /// The sandbox it runs the program in, where it's given one.
    sandbox: Option<Sandbox>,
}

/// Where the program runs with nothing granted: `lnk sandbox run`, the
/// program alone read and run, no network, no folder of the user's, only
/// its stdin and stdout; `lnk` given `home` as its HOME, and nothing
/// else of this process's environment.
#[derive(Clone, Debug)]
pub struct Sandbox {
    pub lnk: PathBuf,
    pub home: PathBuf,
}

impl Default for Extractor {
    fn default() -> Self {
        Self::installed()
    }
}

impl Extractor {
    /// Link Document Converter, `lnk-document-converter`, where it's installed, within
    /// [`deadline`].
    pub fn installed() -> Self {
        Self {
            program: None,
            deadline: deadline(),
            sandbox: None,
        }
    }

    /// `program` in its place, within `deadline`: a test's.
    pub fn program(program: impl Into<PathBuf>, deadline: Duration) -> Self {
        Self {
            program: Some(program.into()),
            deadline,
            sandbox: None,
        }
    }

    /// The same, its program run in `sandbox`: for a process that isn't
    /// confined itself, as the service serving every agent isn't.
    pub fn sandboxed(mut self, sandbox: Sandbox) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// What the service serving every agent extracts with: Link Document
    /// Converter where it's installed, run in `lnk sandbox run` by `lnk`
    /// with `home` as its HOME, since the service isn't confined itself.
    pub fn for_service(lnk: PathBuf, home: PathBuf) -> Self {
        Self::installed().sandboxed(Sandbox { lnk, home })
    }

    /// The program it runs, if it's there.
    fn found(&self) -> Option<PathBuf> {
        match &self.program {
            Some(program) => Some(program.clone()).filter(|p| p.is_file()),
            None => link_core::find_module(MODULE),
        }
    }

    /// Whether it can read a document: its program is there.
    pub fn available(&self) -> bool {
        self.found().is_some()
    }

    /// The text of `bytes`, a document of `format`, at most `text-max-bytes` ([`document_limits`]),
    /// from a child; an error saying why there's none, a child that
    /// crashed or ran past its deadline or limits included.
    pub fn text(&self, format: Format, bytes: &[u8]) -> Result<String> {
        match self.extract(format, bytes) {
            Outcome::Text(text) => Ok(text),
            Outcome::Unread(why) | Outcome::Retry(why) => bail!("{why}"),
        }
    }

    /// What became of `bytes`, a document of `format`: its text, why it
    /// has none, or why it should be tried again (its sandbox didn't
    /// start, which says nothing of the document).
    pub fn extract(&self, format: Format, bytes: &[u8]) -> Outcome {
        match self.run(format, bytes) {
            Ok(text) => Outcome::Text(text),
            Err(Failed::Document(why)) => Outcome::Unread(format!("{why:#}")),
            Err(Failed::Sandbox(why)) => Outcome::Retry(format!("{why:#}")),
        }
    }

    /// The command that reads a document with `program`: the program
    /// alone, or `lnk sandbox run` granting it nothing but itself.
    fn command(&self, program: &std::path::Path) -> Command {
        // None of this process's secrets, and no environment at all: the
        // parser needs none. In a sandbox, `lnk` is given its HOME alone.
        match &self.sandbox {
            None => {
                let mut command = link_secretive::process::command(program);
                command.env_clear();
                command
            }
            Some(sandbox) => {
                let program = program.display().to_string();
                let mut command = link_secretive::process::command(&sandbox.lnk);
                command
                    .env_clear()
                    .env("HOME", &sandbox.home)
                    .args(["sandbox", "run", "--files.read", &program])
                    .args(["--commands.allow", &program, "--", &program]);
                command
            }
        }
    }

    fn run(&self, format: Format, bytes: &[u8]) -> std::result::Result<String, Failed> {
        let document = |why: anyhow::Error| Failed::Document(why);
        if bytes.len() as u64 > document_limits().document_max_bytes {
            return Err(document(anyhow::anyhow!(
                "it's over {} MiB",
                document_limits().document_max_bytes >> 20
            )));
        }
        let Some(program) = self.found() else {
            return Err(document(anyhow::anyhow!("{}", not_installed())));
        };
        let mut command = self.command(&program);
        command
            .args(link_document_converter_contract::text_argv(format))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        limit(&mut command);
        link_secretive::process::ends_with_parent(&mut command, libc::SIGKILL);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                // Never the document's: its parser didn't start (being
                // installed, say), so it's tried again.
                let why = anyhow::Error::new(err).context("starting its parser");
                return Err(Failed::Sandbox(why));
            }
        };
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(document(anyhow::anyhow!(
                "its parser has no stdin or stdout"
            )));
        };
        let (told, heard) = std::sync::mpsc::channel();
        let answer = std::thread::scope(|scope| {
            // A child that stops reading, or ends, ends the write.
            scope.spawn(move || {
                let _ = stdin.write_all(bytes);
            });
            scope.spawn(move || {
                let mut answer = Vec::new();
                let _ = stdout.take(answer_max() + 1).read_to_end(&mut answer);
                let _ = told.send(answer);
            });
            let answer = heard.recv_timeout(self.deadline).ok();
            // Past its deadline, or answering more than it may: ended,
            // which ends both threads.
            if answer
                .as_ref()
                .is_none_or(|answer| answer.len() as u64 > answer_max())
            {
                let _ = child.kill();
            }
            answer
        });
        let status = child
            .wait()
            .context("waiting for its parser")
            .map_err(document)?;
        let Some(answer) = answer else {
            return Err(document(anyhow::anyhow!(
                "its parser took over {} s on it",
                self.deadline.as_secs_f32()
            )));
        };
        match serde_json::from_slice(&answer) {
            Ok(Extracted::Text(read)) if read.text.len() <= document_limits().text_max_bytes => {
                Ok(read.text)
            }
            Ok(Extracted::Unread(why)) => Err(document(anyhow::anyhow!(
                "{}",
                link_format::clean(&why.unread)
            ))),
            // The sandbox couldn't run the program, or find it: nothing
            // read, nothing learned of the document.
            _ if self.sandbox.is_some() && sandbox_failed(status) => Err(Failed::Sandbox(
                anyhow::anyhow!("its sandbox didn't start ({status})"),
            )),
            _ => Err(document(anyhow::anyhow!(
                "its parser failed on it ({status})"
            ))),
        }
    }
}

/// What became of a document given to [`Extractor::extract`].
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Its text.
    Text(String),
    /// Why it has none: kept until the document changes.
    Unread(String),
    /// Why it wasn't read this time: it stays waiting, to be tried again.
    Retry(String),
}

/// Why a run read no text.
enum Failed {
    /// Of the document, or of its parser.
    Document(anyhow::Error),
    /// Before its parser ran: its sandbox, or its program not starting.
    Sandbox(anyhow::Error),
}

/// Whether `status` is `lnk sandbox run`'s for a program it couldn't run
/// or find, as its exit codes say.
fn sandbox_failed(status: std::process::ExitStatus) -> bool {
    let codes = [
        link_format::Exit::SandboxCantRun.code(),
        link_format::Exit::SandboxNotFound.code(),
    ];
    status
        .code()
        .is_some_and(|code| codes.iter().any(|c| i32::from(*c) == code))
}

/// The child's limits, set in it before it runs: a limit already lower
/// stays, and a system that doesn't enforce one (memory, on a Mac) has
/// the deadline.
fn limit(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // Read here, never in the child, where reading them first would not
    // be async-signal-safe.
    let limits = crate::limits::limits();
    let cpu = limits.extract_deadline_secs as libc::rlim_t;
    let memory = limits.extract_memory_max_bytes as libc::rlim_t;
    let most = |most: libc::rlim_t| libc::rlimit {
        rlim_cur: most,
        rlim_max: most,
    };
    // SAFETY: in the child, before it runs the program: setrlimit alone,
    // which is async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            libc::setrlimit(libc::RLIMIT_AS, &most(memory));
            libc::setrlimit(libc::RLIMIT_CPU, &most(cpu));
            // A crash leaves no core: it would hold the document.
            libc::setrlimit(libc::RLIMIT_CORE, &most(0));
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The service, serving every agent unconfined, runs each document's
    /// parser in `lnk sandbox run`, never directly.
    #[test]
    fn the_services_extractor_is_sandboxed() {
        let extractor = Extractor::for_service("/opt/lnk".into(), "/home/ada".into());
        let sandbox = extractor.sandbox.as_ref().expect("sandboxed");
        assert_eq!(
            (sandbox.lnk.as_path(), sandbox.home.as_path()),
            (
                std::path::Path::new("/opt/lnk"),
                std::path::Path::new("/home/ada")
            )
        );
        let command = extractor.command(std::path::Path::new("/x/lnk-document-converter"));
        assert_eq!(command.get_program(), "/opt/lnk");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            [
                "sandbox",
                "run",
                "--files.read",
                "/x/lnk-document-converter",
                "--commands.allow",
                "/x/lnk-document-converter",
                "--",
                "/x/lnk-document-converter"
            ]
        );
        let envs: Vec<_> = command.get_envs().collect();
        assert_eq!(
            envs,
            [(
                std::ffi::OsStr::new("HOME"),
                Some(std::ffi::OsStr::new("/home/ada"))
            )]
        );
    }
}

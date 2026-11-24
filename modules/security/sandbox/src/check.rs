//! `lnk sandbox check` (and `lnk agent harness check` through it): tries, from
//! inside a real sandbox, the things it shouldn't allow, and says which
//! got through.
//!
//! Each probe is a line of bash run under the confinement being checked
//! (its folders, its ports, its network), against something Link sets up
//! for it: a file in the home folder, a server on a localhost port that
//! isn't its own, a program with a secret in its environment.

mod bait;
mod dot_files;
mod probes;
mod run;
mod scripts;
mod wildcards;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

pub use bait::Bait;
pub use probes::{ipv6_forms, probes};
pub use run::run;
pub use wildcards::shared_wildcards;

/// What a probe should find.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    Blocked,
    Allowed,
    /// Reachable, and known to be: the sandbox can't close it.
    Gap(&'static str),
}

pub struct Probe {
    pub what: String,
    pub script: &'static str,
    pub expect: Expect,
    /// A file the probe writes (`$LNK_PROBE_TARGET`), checked from outside: a
    /// write the sandbox redirects (Linux's empty home folder) didn't
    /// reach the real one.
    pub lands: Option<PathBuf>,
    /// What it tries (`$LNK_PROBE_ARG`): a system call's name, a port, a macOS
    /// service.
    pub arg: Option<String>,
    /// The file it reads (`$LNK_PROBE_PATH`), for the hidden folders' probes.
    pub path: Option<PathBuf>,
    /// Asked for by name ([`Expected`]): allowed but not reached is a
    /// failure, not a warning.
    pub required: bool,
}

/// What a probe found.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Blocked,
    Allowed,
    /// Allowed, but it didn't work (no internet here, say).
    AllowedFailed,
    /// Asked for by name, and it didn't happen.
    Missed,
    Gap(&'static str),
    Leak,
}

impl Probe {
    pub fn verdict(&self, reached: bool) -> Verdict {
        match (self.expect, reached) {
            (Expect::Blocked, false) => Verdict::Blocked,
            (Expect::Blocked, true) => Verdict::Leak,
            (Expect::Allowed, true) => Verdict::Allowed,
            (Expect::Allowed, false) if self.required => Verdict::Missed,
            (Expect::Allowed, false) => Verdict::AllowedFailed,
            (Expect::Gap(_), false) => Verdict::Blocked,
            (Expect::Gap(why), true) => Verdict::Gap(why),
        }
    }
}

/// `host[:port]` (`[v6]:port`) as a `CONNECT` target, port 443 if none.
pub fn target(text: &str) -> anyhow::Result<String> {
    let (host, port) = link_sandbox_proxy::http::host_port(text, Some(443))
        .ok_or_else(|| anyhow::anyhow!("{text:?} isn't a host[:port]"))?;
    Ok(match host.contains(':') {
        true => format!("[{host}]:{port}"),
        false => format!("{host}:{port}"),
    })
}

/// What a caller asks `check` to try besides its own probes (a spec's
/// cases, `tests/specs/sandbox`), each a failure unless it comes out
/// as asked.
#[derive(Clone, Debug, Default)]
pub struct Expected {
    /// `host:port`s it must reach: its proxy takes them, or without one,
    /// they answer.
    pub reaches: Vec<String>,
    /// `host:port`s it must not.
    pub refuses: Vec<String>,
    /// Programs (absolute) it must run.
    pub runs: Vec<PathBuf>,
    /// Programs (absolute) it must not.
    pub refused_programs: Vec<PathBuf>,
    /// Folders it writes (absolute, resolved) whose dot files that run
    /// outside ([`crate::dotfiles`]) it must write too.
    pub dot_files_open: Vec<PathBuf>,
    /// Folders it writes whose dot files it must not.
    pub dot_files_closed: Vec<PathBuf>,
    /// Folders it must read (a team's it's on), and those it must not
    /// (another team's).
    pub reads: Vec<PathBuf>,
    pub unread: Vec<PathBuf>,
}

/// A folder the policy writes, whose dot files that run outside the
/// probes try to write: what they should find, and whether it was asked
/// for by name.
#[derive(Clone, Debug)]
pub struct DotFiles {
    pub folder: PathBuf,
    pub expect: Expect,
    pub required: bool,
}

/// What a probe writes in each folder the policy may write.
const CHECK_FILE: &str = "lnk-check";

/// Where a probe tries to write beside `settings`, the bait in Link's
/// settings.
fn settings_write(settings: &Path) -> PathBuf {
    settings.with_extension("write")
}

/// A path as a string, for an environment value.
fn disp(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}

/// `path`, with the home folder as `~`.
fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

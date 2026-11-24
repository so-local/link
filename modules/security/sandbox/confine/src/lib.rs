//! The sandbox's wall: what confines a program, as tightly as it can
//! still run, on each OS. A library only `lnk-sandbox` links.
//!
//! By default a program reads the system's programs and libraries, reads
//! and writes the folders its confinement names and its temp folder, and
//! connects to the localhost ports it names (macOS; Linux can't tell
//! ports apart). Nothing else: not the rest of the home folder (not even
//! whether a file exists there), not the Keychain, not other apps, never
//! Link's settings. What its [`Confinement`] grants opens one thing more
//! each: the host's network, every localhost port, the home folder to
//! read, Xcode's tools.
//!
//! A [`Confinement`] is resolved here once, to [`Rules`], and handed to
//! this OS's [`Backend`]: [`linux`] (bubblewrap, Landlock, seccomp) or
//! [`macos`] (Seatbelt). Both are built on every OS, so each is tested on
//! any; `cfg` appears only where [`here`] picks one. What both share: the
//! dot files found at the top of a written folder ([`dotfiles`]) and the
//! programs judged against a spec's `[commands]` ([`programs`]).
//!
//! [`Confinement`]: link_sandbox_client::Confinement

mod constants;
pub mod dotfiles;
pub mod linux;
pub mod macos;
pub mod programs;
mod rules;
pub mod system;
#[cfg(test)]
mod tests;

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

pub use rules::{
    Common, GrantRefused, Linux, Macos, Rules, lexical, ports, resolve, resolve_files,
    resolve_grant,
};

/// The file grants the in-sandbox first program hands Landlock for its
/// second, independent file wall ([`linux::landlock::restrict_files`]): the
/// paths a program may read, and those it may read and write. Present only
/// where the kernel has Landlock; the system's own read-only folders are
/// added inside, beside these ([`linux::enter`]).
pub struct Wall<'a> {
    pub read: &'a [PathBuf],
    pub write: &'a [PathBuf],
}

/// The OS a backend confines on: what `check`'s table of probes reads
/// to know what each should find, never `cfg`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
}

/// One OS's way of confining a program.
pub trait Backend: Sync {
    /// The OS it confines on.
    fn os(&self) -> Os;

    /// `argv` wrapped to run inside `rules`. What it writes for that (macOS's
    /// profile) goes at `kept`, in Link's settings, where nothing
    /// sandboxed edits it.
    fn wrap(&self, rules: &Rules, kept: &Path, argv: &[String]) -> anyhow::Result<Vec<String>>;

    /// Inside the sandbox, as its first program (`sandbox confine`): what
    /// it was handed closed, the file grants held again by Landlock where
    /// the kernel has it (`wall`), what it may run held to `only` (files,
    /// or folders of them) when given, then `argv` in its place. Returns
    /// only what stopped it.
    fn enter(
        &self,
        wall: Option<&Wall>,
        only: Option<&[PathBuf]>,
        argv: &[String],
    ) -> anyhow::Result<Infallible>;

    /// The watch, for `lnk sandbox learn`: runs `argv` (wrapped, `env` its
    /// whole environment) and returns its exit status and each program it
    /// started, a line each: the path given, a tab, the file.
    fn learn(
        &self,
        argv: &[String],
        env: &[(String, String)],
    ) -> anyhow::Result<(ExitStatus, String)>;

    /// The watch, from outside (`sandbox refusals`): runs `argv` (wrapped)
    /// and says each program it was refused that `programs` doesn't list;
    /// its exit status. Where the sandbox names them itself (Linux), an
    /// error.
    fn refusals(&self, programs: &[PathBuf], argv: &[String]) -> anyhow::Result<ExitStatus>;
}

/// This OS's backend.
pub fn here() -> anyhow::Result<&'static dyn Backend> {
    #[cfg(target_os = "linux")]
    return Ok(&linux::Linux);
    #[cfg(target_os = "macos")]
    return Ok(&macos::MacOs);
    #[allow(
        unreachable_code,
        reason = "unreachable on Linux and macOS, which return above; reached on any other OS"
    )]
    Err(anyhow::anyhow!("Link has no sandbox for this OS"))
}

/// `argv` wrapped to run inside `rules` on this OS ([`Backend::wrap`]).
pub fn wrap(rules: &Rules, kept: &Path, argv: &[String]) -> anyhow::Result<Vec<String>> {
    here()?.wrap(rules, kept, argv)
}

#[cfg(test)]
link_test_kit::program!(system);

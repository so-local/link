//! Which sources a Link program built from source was built from, so a
//! module built from other sources than the program running it is named
//! rather than run. Two builds from source at different commits both say
//! their package's version, so without it one refuses the other in its
//! own words, or answers a command it no longer has.
//!
//! A program's build script records its checkout's commit
//! (`link_component::SOURCE_VARIABLE`), [`source!`] declares it as the
//! program starts, and a module says its own in its descriptor
//! (`--link-describe`, `Resolved::source`). A release build records none,
//! its version being exact, and neither does a program outside a Git
//! checkout: only two known and different sources are refused.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// How long a module has to describe itself: a Link program answers at
/// once, before any startup of its own.
const DESCRIBE_WAIT: Duration = Duration::from_secs(5);

static THIS: OnceLock<Option<&'static str>> = OnceLock::new();

/// Declares this program's sources, once, as it starts: what its build
/// script handed it. The modules' `describe_component!` does it for them.
#[macro_export]
macro_rules! source {
    () => {
        $crate::source::declare(option_env!("LNK_BUILT_FROM"))
    };
}

/// [`source!`]'s work: the first declaration stands.
pub fn declare(source: Option<&'static str>) {
    let _ = THIS.set(source.filter(|s| !s.is_empty()));
}

/// The sources this program was built from, once declared.
pub fn this() -> Option<&'static str> {
    THIS.get().copied().flatten()
}

/// A module built from other sources than the program that found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skew {
    pub module: String,
    pub path: PathBuf,
    /// This program's sources.
    pub ours: String,
    /// The module's.
    pub theirs: String,
}

impl Skew {
    /// The checkout this program was built from, where every program is
    /// built again together: its path, when it knows it.
    pub fn checkout(&self) -> String {
        crate::CHECKOUT
            .map(|c| c.root().display().to_string())
            .unwrap_or_else(|| "<checkout>".into())
    }
}

impl std::fmt::Display for Skew {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let checkout = self.checkout();
        write!(
            f,
            "lnk-{} at {} was built from commit {}, and this program from {}: build them together with cargo build --release --workspace --bins --manifest-path {checkout}/Cargo.toml, or lnk up --source {checkout}",
            self.module,
            self.path.display(),
            short(&self.theirs),
            short(&self.ours),
        )
    }
}

impl std::error::Error for Skew {}

/// A commit as people read one: its first twelve characters.
fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

/// Whether `lnk-<module>`, at `path`, was built from this program's
/// sources: refused only when both are known and differ.
pub fn check(module: &str, path: &Path) -> Result<(), Skew> {
    compare(this(), module, path, of)
}

/// [`check`] of `ours` against what `describe` says of `path`, asked only
/// when `ours` is known.
fn compare(
    ours: Option<&str>,
    module: &str,
    path: &Path,
    describe: impl FnOnce(&Path) -> Option<String>,
) -> Result<(), Skew> {
    let Some(ours) = ours else {
        return Ok(());
    };
    match describe(path) {
        Some(theirs) if theirs != ours => Err(Skew {
            module: module.to_string(),
            path: path.to_path_buf(),
            ours: ours.to_string(),
            theirs,
        }),
        _ => Ok(()),
    }
}

/// The variable an asker hands the module answering its question in
/// ([`crate::answers::Answerer::command`]): the sources it was built from,
/// so the answerer compares them with its own rather than the asker
/// running it once more to ask.
pub const ASKER: &str = "LNK_ASKER_BUILT_FROM";

/// Whether this program, answering a question, was built from the asker's
/// sources ([`ASKER`]): refused, naming this program, only when both are
/// known and differ, as [`check`] would refuse it.
pub fn answering() -> Result<(), Skew> {
    let asker = std::env::var(ASKER).ok();
    let path = std::env::current_exe().unwrap_or_default();
    answered(asker.as_deref(), &path, this())
}

/// [`answering`] for the program at `path`, built from `ours`, asked by
/// one built from `asker`: the [`Skew`] names this program as the module.
fn answered(asker: Option<&str>, path: &Path, ours: Option<&str>) -> Result<(), Skew> {
    let module = path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(|name| name.strip_prefix("lnk-").unwrap_or(name))
        .unwrap_or_default();
    let ours = ours.filter(|s| !s.is_empty()).map(String::from);
    compare(asker.filter(|s| !s.is_empty()), module, path, |_| ours)
}

/// The sources the program at `path` says it was built from, asked once
/// a process: none for a script, which no build made, or a program that
/// doesn't describe itself.
pub fn of(path: &Path) -> Option<String> {
    static SEEN: OnceLock<Mutex<HashMap<PathBuf, Option<String>>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if let Some(known) = seen.lock().ok()?.get(path) {
        return known.clone();
    }
    let source = ask(path);
    seen.lock().ok()?.insert(path.to_path_buf(), source.clone());
    source
}

/// What `path --link-describe` says of its sources, within
/// [`DESCRIBE_WAIT`].
fn ask(path: &Path) -> Option<String> {
    sources_in(&described(path).ok()?)
}

/// Why a program gave no description.
#[derive(Debug)]
pub(crate) enum Unanswered {
    /// A script (`#!`), which is no Link program, or one that couldn't start.
    NotLink,
    /// It didn't answer within [`DESCRIBE_WAIT`], and was stopped.
    Late,
    /// It failed, with what it last said: its refusal of this program's
    /// description protocol among them.
    Failed(String),
}

impl std::fmt::Display for Unanswered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unanswered::NotLink => f.write_str("isn't a Link program"),
            Unanswered::Late => f.write_str("didn't answer --link-describe in time"),
            Unanswered::Failed(said) if said.is_empty() => {
                f.write_str("failed to answer --link-describe")
            }
            Unanswered::Failed(said) => write!(f, "refused --link-describe: {said}"),
        }
    }
}

/// What `path --link-describe` answers, within [`DESCRIBE_WAIT`], asked
/// stating this program's description protocol (`link_component::asking`):
/// nothing from a script (`#!`), which is no Link program, or from one that
/// doesn't answer in time, which is stopped; what it last said, from one
/// that fails.
pub(crate) fn described(path: &Path) -> Result<Vec<u8>, Unanswered> {
    use std::io::Read;
    let mut head = [0u8; 2];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .map_err(|_| Unanswered::NotLink)?;
    if &head == b"#!" {
        return Err(Unanswered::NotLink);
    }
    let mut command = link_secretive::process::command(path);
    link_component::asking(&mut command);
    let mut child = command
        .arg("--link-describe")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| Unanswered::NotLink)?;
    // Read as they're written, so a long descriptor never fills the pipe;
    // of its stderr, only what names a failure.
    let reader = |pipe: Box<dyn Read + Send>, whole: bool| {
        let (sent, read) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut pipe = pipe;
            let out = match whole {
                true => {
                    let mut out = Vec::new();
                    let _ = pipe.read_to_end(&mut out);
                    out
                }
                false => crate::answers::Said::shipped().read(pipe),
            };
            let _ = sent.send(out);
        });
        read
    };
    let stdout = child.stdout.take().map(|p| reader(Box::new(p), true));
    let stderr = child.stderr.take().map(|p| reader(Box::new(p), false));
    let (Some(stdout), Some(stderr)) = (stdout, stderr) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Unanswered::NotLink);
    };
    // Its answer ends as it exits; one that doesn't answer in time is stopped.
    match stdout.recv_timeout(DESCRIBE_WAIT) {
        Ok(out) => match child.wait() {
            Ok(status) if status.success() => Ok(out),
            _ => {
                let said = stderr.recv_timeout(DESCRIBE_WAIT).unwrap_or_default();
                let said = String::from_utf8_lossy(&said);
                Err(Unanswered::Failed(link_format::clean(said.trim())))
            }
        },
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(Unanswered::Late)
        }
    }
}

/// The sources a descriptor says, read for that field alone, so an older
/// or newer descriptor still says it.
fn sources_in(descriptor: &[u8]) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Described {
        #[serde(default)]
        source: Option<String>,
    }
    serde_json::from_slice::<Described>(descriptor)
        .ok()?
        .source
        .filter(|s| !s.is_empty())
}

/// Says `skew` aside, once a process for each module's path: for a
/// caller that only asks whether a module is there.
pub(crate) fn say(skew: &Skew) {
    static SAID: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let said = SAID.get_or_init(Default::default);
    if said
        .lock()
        .map(|mut s| s.insert(skew.path.clone()))
        .unwrap_or(false)
    {
        link_format::aside(link_format::Note(link_format::Sentence::adopt(
            &skew.to_string(),
        )));
    }
}

#[cfg(test)]
mod tests;

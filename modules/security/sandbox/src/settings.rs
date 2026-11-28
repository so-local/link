//! The sandbox module's own opinions: how long a question waits for
//! your answer, and how many tool servers its bridge starts at once and
//! keeps, and for how long. Yours are `~/.lnk/safe/sandboxes/settings.toml`, which no
//! sandbox reaches; what they leave out is Link's, the file this module
//! comes with (`defaults.toml`), never a constant.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;

use anyhow::ensure;
use link_format::WithinExt;
use serde::Deserialize;

/// Link's own settings, as this module comes with them.
pub const DEFAULTS: &str = include_str!("../defaults.toml");

/// The settings as written: yours, or Link's, each key optional.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Written {
    #[serde(default)]
    ask_wait: Option<String>,
    #[serde(default)]
    tool_starts: Option<usize>,
    #[serde(default)]
    tool_sessions: Option<usize>,
    #[serde(default)]
    tool_idle: Option<String>,
    #[serde(default)]
    shared_idle: Option<String>,
    #[serde(default)]
    tool_stop: Option<String>,
}

/// The settings the sandbox runs with: yours over Link's.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// How long a question waits for your answer; silence is no.
    pub ask_wait: Duration,
    /// Tool servers starting at once, across a proxy's tools.
    pub tool_starts: usize,
    /// Sessions one sandbox keeps of each tool.
    pub tool_sessions: usize,
    /// How long a tool's session stays with no call: its server, or its
    /// place in a shared tool's hub, with it.
    pub tool_idle: Duration,
    /// How long a shared tool's server stays up with no session open.
    pub shared_idle: Duration,
    /// How long a tool's server has to end once its input closes, and
    /// again once asked to stop, before it's killed ([`crate::bridges::stop`]).
    pub tool_stop: Duration,
}

/// The longest a question may wait: a longer `ask_wait` is refused.
const ASK_WAIT_MAX: Duration = Duration::from_secs(24 * 60 * 60);

/// The longest a server may take to stop: twice it stays under the five
/// seconds the next shared tool's hub waits for the last one to end.
const TOOL_STOP_MAX: Duration = Duration::from_secs(2);

/// The folder the settings are read from, when set ([`read_from`]).
static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Reads this process's settings from `dir` (Link's `sandbox/`, as handed
/// to a proxy or a shared tool's hub), not from the `HOME` it runs with:
/// a proxy runs with its harness's, a folder the harness writes. Call
/// before the first [`get`]; a later call changes nothing.
pub fn read_from(dir: &Path) {
    let _ = DIR.set(dir.to_path_buf());
}

/// Why yours couldn't be read, when Link's are used instead: for the
/// command to say once it ends.
static PROBLEM: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Why yours couldn't be read, if they were tried and couldn't be.
pub fn problem() -> Option<&'static str> {
    PROBLEM.get().map(String::as_str)
}

/// This process's settings, read once: yours over Link's, or Link's
/// alone, said, when yours can't be read.
pub fn get() -> &'static Settings {
    static SETTINGS: LazyLock<Settings> = LazyLock::new(|| load(DIR.get()));
    &SETTINGS
}

/// The settings in `dir`, else in Link's settings as `HOME` finds them:
/// yours over Link's, or Link's alone, said, when yours can't be read.
fn load(dir: Option<&PathBuf>) -> Settings {
    let dir = match dir {
        Some(dir) => Ok(dir.clone()),
        None => link_files::layout::Hidden::Sandboxes.try_path(),
    };
    let yours = dir.map(|dir| dir.join("settings.toml")).and_then(|path| {
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some((path, text))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).within(format!("reading {}", path.display())),
        }
    });
    let read = yours.and_then(|yours| match yours {
        Some((path, text)) => link_files::saved::parse::<Written>(&text)
            .and_then(resolve)
            .within(format!("in {}", path.display())),
        None => resolve(Written::default()),
    });
    read.unwrap_or_else(|err| {
        let _ = PROBLEM.set(format!("{err:#}"));
        resolve(Written::default()).expect("Link's sandbox settings read")
    })
}

link_files::specification!(Written, include_str!("settings.schema.toml"));

/// Your sandbox settings' file, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Written>()
}

/// `yours` (a settings file's, or none) over Link's.
fn resolve(yours: Written) -> anyhow::Result<Settings> {
    let links: Written = toml::from_str(DEFAULTS).map_err(|e| anyhow::anyhow!("{e}"))?;
    let wait = yours.ask_wait.or(links.ask_wait).within("no ask_wait")?;
    let ask_wait = Duration::from_millis(link_spec::duration_ms(&wait).within("ask_wait")?);
    ensure!(
        ask_wait >= Duration::from_secs(1),
        "ask_wait is at least 1s"
    );
    ensure!(ask_wait <= ASK_WAIT_MAX, "ask_wait is at most 24h");
    let tool_starts = yours
        .tool_starts
        .or(links.tool_starts)
        .within("no tool_starts")?;
    let tool_sessions = yours
        .tool_sessions
        .or(links.tool_sessions)
        .within("no tool_sessions")?;
    ensure!(tool_starts >= 1, "tool_starts is at least 1");
    ensure!(tool_sessions >= 1, "tool_sessions is at least 1");
    let idle = yours.tool_idle.or(links.tool_idle).within("no tool_idle")?;
    let tool_idle = Duration::from_millis(link_spec::duration_ms(&idle).within("tool_idle")?);
    ensure!(
        tool_idle >= Duration::from_secs(1),
        "tool_idle is at least 1s"
    );
    let idle = yours
        .shared_idle
        .or(links.shared_idle)
        .within("no shared_idle")?;
    let shared_idle = Duration::from_millis(link_spec::duration_ms(&idle).within("shared_idle")?);
    ensure!(
        shared_idle >= Duration::from_secs(1),
        "shared_idle is at least 1s"
    );
    let stop = yours.tool_stop.or(links.tool_stop).within("no tool_stop")?;
    let tool_stop = Duration::from_millis(link_spec::duration_ms(&stop).within("tool_stop")?);
    ensure!(tool_stop <= TOOL_STOP_MAX, "tool_stop is at most 2s");
    Ok(Settings {
        ask_wait,
        tool_starts,
        tool_sessions,
        tool_idle,
        shared_idle,
        tool_stop,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A settings file's text, read and resolved over Link's.
    fn read(text: &str) -> anyhow::Result<Settings> {
        link_files::saved::parse::<Written>(text).and_then(resolve)
    }

    #[test]
    fn yours_over_links() {
        let links = resolve(Written::default()).unwrap();
        assert_eq!(
            links,
            Settings {
                ask_wait: Duration::from_secs(120),
                tool_starts: 2,
                tool_sessions: 8,
                tool_idle: Duration::from_secs(300),
                shared_idle: Duration::from_secs(600),
                tool_stop: Duration::from_secs(2),
            }
        );
        let yours = read("schema = 1\nask_wait = \"10m\"\ntool_sessions = 20\n").unwrap();
        assert_eq!(yours.ask_wait, Duration::from_secs(600));
        assert_eq!((yours.tool_starts, yours.tool_sessions), (2, 20));
        for bad in [
            "ask_wait = \"0s\"",
            "tool_starts = 0",
            "shared_idle = \"0s\"",
            "tool_idle = \"0s\"",
            "tool_stop = \"3s\"",
            "waits = 1",
            "ask_wait = 3",
            "ask_wait = \"25d\"",
        ] {
            assert!(read(&format!("schema = 1\n{bad}")).is_err(), "{bad}");
        }
    }

    /// Read from the folder handed in, not from the `HOME` the process
    /// runs with, which a proxy shares with its harness.
    #[test]
    fn read_from_the_folder_handed_in() {
        let dir = link_test_kit::TempDir::new("settings");
        std::fs::write(dir.join("settings.toml"), "schema = 1\ntool_sessions = 3\n").unwrap();
        assert_eq!(load(Some(&dir.to_path_buf())).tool_sessions, 3);
    }
}

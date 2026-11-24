//! The sandbox's policy contract. It resolves authored permissions and
//! network predicates without the CLI, confinement backends or proxy.

pub mod box_users;
pub mod box_workers;
pub mod bubblewrap;
mod checks;
pub mod constants;
pub mod dot_files;
pub mod isolation;
pub mod net;
pub mod spec;
pub mod system;
pub mod toolchains;
use serde::{Deserialize, Serialize};
pub use spec::Spec;
use std::path::{Path, PathBuf};

/// The programs a sandbox may run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Programs {
    /// Any (`[commands] allow = ["*"]`).
    Any,
    /// Only these, by path: a file, or a folder of them; none, nothing.
    Only(Vec<PathBuf>),
}

impl Programs {
    /// The effective command policy used by operation requirements.
    pub fn requirement_policy(&self) -> &'static str {
        match self {
            Self::Any => "unrestricted",
            Self::Only(_) => "restricted",
        }
    }

    /// Its list, None for any.
    pub fn listed(self) -> Option<Vec<PathBuf>> {
        match self {
            Programs::Any => None,
            Programs::Only(programs) => Some(programs),
        }
    }
}

/// Variables a caller can't give a sandboxed program: those `lnk sandbox
/// run` keeps or sets itself, the dynamic loader's (`LD_*`, `DYLD_*`)
/// and a proxy's. Startup hooks and interpreter search paths are refused
/// too: an environment grant must not install code before the requested
/// command starts.
pub fn check_given_variable(name: &str) -> anyhow::Result<()> {
    let plain = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    anyhow::ensure!(
        plain,
        "{:?} can't be a variable's name",
        name.chars().filter(|c| !c.is_control()).collect::<String>()
    );
    let upper = name.to_ascii_uppercase();
    let own = [
        "PATH",
        "HOME",
        "TERM",
        "COLORTERM",
        "LANG",
        "TZ",
        "USER",
        "LOGNAME",
        "SHELL",
        "TMPDIR",
        "BASH_ENV",
        "ENV",
        "BASHOPTS",
        "SHELLOPTS",
        "NODE_OPTIONS",
        "NODE_PATH",
        "PYTHONPATH",
        "PYTHONHOME",
        "PYTHONSTARTUP",
        "PERL5OPT",
        "PERL5LIB",
        "RUBYOPT",
        "RUBYLIB",
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
        "JDK_JAVA_OPTIONS",
        "GCONV_PATH",
        "LOCPATH",
    ];
    anyhow::ensure!(
        !own.contains(&upper.as_str())
            && !upper.starts_with("LC_")
            && !upper.starts_with("LD_")
            && !upper.starts_with("DYLD_")
            && !upper.ends_with("_PROXY")
            && !upper.starts_with("LNK_SANDBOX"),
        "{name} is a variable the sandbox sets or keeps"
    );
    Ok(())
}

/// `~/Link/Agents`: a folder per agent, which
/// every sandbox but an agent's own hides.
pub fn agents_dir() -> PathBuf {
    link_files::layout::Home::Agents.path()
}

/// `~/Link/Teams`: a folder per team, its shared
/// folder and what its tools keep, which every sandbox hides but for what
/// a member is granted of it.
pub fn teams_dir() -> PathBuf {
    link_files::layout::Home::Teams.path()
}

/// `~/Link/Archive`: the archives you export and those of deleted
/// things (`<name>-<date>.tar.gz`), which every sandbox hides.
pub fn archive_dir() -> PathBuf {
    link_files::layout::Home::Archive.path()
}

/// The folders every sandbox hides, whatever it's allowed, but for what
/// it names in them: [`agents_dir`], [`archive_dir`] and [`teams_dir`],
/// other agents' and teams' files and keys.
pub fn hidden_dirs() -> Vec<PathBuf> {
    vec![
        agents_dir(),
        archive_dir(),
        teams_dir(),
        link_files::layout::Home::Safe.path(),
    ]
}

/// Why no sandbox may write `files`, if it mustn't: where writing is
/// a way out of a sandbox (programs and settings that run or are read
/// outside it) or holds the user's keys.
pub fn unsafe_files(files: &Path) -> Option<String> {
    let home = link_files::home_dir();
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let files = [files.to_path_buf(), real(files)];
    // Never holding these: the home folder itself, where lnk and its
    // modules are, and the system.
    let mut containing = vec![
        (home.clone(), "it holds your whole home folder"),
        (PathBuf::from("/"), "it holds the whole system"),
    ];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| real(&exe).parent().map(Path::to_path_buf))
    {
        containing.push((dir, "lnk runs its modules from there"));
    }
    // Neither holding nor inside these.
    let guarded = [
        (
            link_files::installation_dir(),
            "Link's settings and sandbox rules are there",
        ),
        (
            home.join("Library/LaunchAgents"),
            "programs there start at login",
        ),
        (home.join(".ssh"), "your SSH keys are there"),
        (
            link_files::layout::Home::Safe.path(),
            "Link's editable configuration is there",
        ),
    ];
    for files in &files {
        for (dir, why) in &containing {
            if dir.starts_with(files) {
                return Some(why.to_string());
            }
        }
        for (dir, why) in &guarded {
            let dir = real(dir);
            if files.starts_with(&dir) || dir.starts_with(files) {
                return Some(why.to_string());
            }
        }
        // Dot folders in the home folder: settings other programs run
        // (~/.config, ~/.local/bin, shells' startup files' folders).
        if let Ok(rest) = files
            .strip_prefix(real(&home))
            .or(files.strip_prefix(&home))
            && rest
                .components()
                .next()
                .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        {
            return Some("other programs' settings are in the home folder's dot folders".into());
        }
    }
    None
}

#[cfg(test)]
link_test_kit::program!(unit);

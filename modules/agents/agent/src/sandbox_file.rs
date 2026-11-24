//! `~/Link/Safe/Agents/<agent>/sandbox.toml`: what you grant an agent,
//! whichever harness runs it (`lnk agent permission add`, `deny`, or `lnk agent
//! start --sandbox`), a sandbox's spec in the agent's own terms
//! ([`link_agent_contract::agent_sandbox`]).
//!
//! It's one of two layers. This one is yours and follows the agent from
//! harness to harness, so a switch never asks for it again. The other is
//! each harness's ([`crate::grants`]): only what that harness said it
//! needs, held while it runs, never given to another.

use link_format::WithinExt;
use std::path::{Path, PathBuf};

use link_sandbox_contract::spec::Os;

pub use link_agent_contract::agent_sandbox::AgentSandbox as Sandbox;
pub use link_agent_contract::agent_sandbox::LINKS_OWN_TOOLS;
pub use link_agent_contract::agent_sandbox::check_tool;

use crate::settings::configuration_of;

/// The PATH a `[commands]` name is found on, by `run` and the harness
/// alike, and its programs are looked for on at a start: the system's,
/// and on a Mac Homebrew's.
pub const PATH: &str = match cfg!(target_os = "macos") {
    true => "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
    false => "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
};

/// Agent `agent`'s file.
pub fn path(agent: &str) -> PathBuf {
    configuration_of(agent).join("sandbox.toml")
}

/// Agent `agent`'s, closed without a file; an error for a file it can't
/// read.
pub fn load(agent: &str) -> anyhow::Result<Sandbox> {
    load_at(&path(agent))
}

fn load_at(path: &Path) -> anyhow::Result<Sandbox> {
    let Some(read) =
        link_files::fs::read_control_optional(path, link_spec::MAX_BYTES, "an agent's grants'")?
    else {
        return Ok(Sandbox::none());
    };
    let text = read.text;
    Sandbox::parse(&text).within(format!("in {}", path.display()))
}

/// [`load`], or for a file it can't read, none at all (no network,
/// nothing more): a start reads it with [`load`] first, and fails there.
pub fn of(agent: &str) -> Sandbox {
    load(agent).unwrap_or_else(|_| Sandbox::none())
}

/// [`load`], or for a file it can't read, said on stderr and taken as
/// none at all, so `deny` and `allow` still work and write it anew, never
/// wider than it was.
pub fn load_or_warn(agent: &str) -> Sandbox {
    match load(agent) {
        Ok(sandbox) => sandbox,
        Err(err) => {
            link_format::aside(
                link_format::Report::warning(link_format::sentence!(
                    "{}'s grants can't be read.",
                    link_format::Name::new("agent", agent)
                ))
                .why(link_format::Sentence::adopt(&format!(
                    "{err:#}. They are taken as nothing beyond its harness's home, and allow \
                     and deny write them anew."
                ))),
            );
            Sandbox::none()
        }
    }
}

/// Where a write of agent `agent`'s file is locked: this machine's
/// records of it, never its settings' folder, which holds only its state.
fn lock(agent: &str) -> PathBuf {
    crate::settings::config_of(agent).join("sandbox.lock")
}

/// Writes `sandbox` as agent `agent`'s, only the user's.
pub fn save(agent: &str, sandbox: &Sandbox) -> anyhow::Result<()> {
    let _lock = link_files::lock_file(&lock(agent))?;
    save_unlocked(agent, sandbox)
}

/// A reviewed change commits only against the same current grants, under the writer's lock.
pub fn save_if(
    agent: &str,
    expected: &Sandbox,
    replacement: &Sandbox,
    before: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let _lock = link_files::lock_file(&lock(agent))?;
    anyhow::ensure!(
        load(agent)? == *expected,
        "the sandbox permissions changed while asking; review the new proposal"
    );
    before()?;
    save_unlocked(agent, replacement)
}

fn save_unlocked(agent: &str, sandbox: &Sandbox) -> anyhow::Result<()> {
    let path = path(agent);
    link_files::create_private_dir(&configuration_of(agent))
        .within(format!("creating {}", configuration_of(agent).display()))?;
    link_files::write_private(&path, sandbox.text(agent)?.as_bytes())
        .within(format!("writing {}", path.display()))
}

/// A new agent gets Link's defaults written out, an explicit file: only
/// a deleted one is taken, and recreated, closed.
pub fn initialize(agent: &str) -> anyhow::Result<bool> {
    write_missing(agent, &Sandbox::link())
}

/// Repair a deleted file with closed permissions, without replacing any file.
pub fn recreate(agent: &str) -> anyhow::Result<bool> {
    write_missing(agent, &Sandbox::none())
}

fn write_missing(agent: &str, sandbox: &Sandbox) -> anyhow::Result<bool> {
    write_missing_at(&path(agent), sandbox, agent)
}

fn write_missing_at(path: &Path, sandbox: &Sandbox, agent: &str) -> anyhow::Result<bool> {
    use std::io::Write;
    match std::fs::symlink_metadata(path) {
        Ok(_) => return Ok(false),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).within(format!("checking {}", path.display())),
    }
    link_files::create_private_dir(
        path.parent()
            .within("a sandbox file needs a parent folder")?,
    )?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(err) => {
            return Err(err).within(format!(
                "creating {} with closed permissions",
                path.display()
            ));
        }
    };
    file.write_all(sandbox.text(agent)?.as_bytes())
        .within(format!("writing {}", path.display()))?;
    Ok(true)
}

/// Keeps the file at `from` as agent `agent`'s, once it's read strictly.
pub fn keep(agent: &str, from: &Path) -> anyhow::Result<Sandbox> {
    let read = link_spec::read::<toml::Table>(from)?;
    let sandbox = Sandbox::parse(&read.text).within(format!("in {}", from.display()))?;
    save(agent, &sandbox)?;
    Ok(sandbox)
}

/// Each thing `sandbox` opens here beyond Link's file, as `lnk agent permission remove`
/// takes it back (`--internet.tcp github.com:22`). A narrower list of hosts, or a
/// limit of calls, narrows, and isn't here.
pub fn beyond_defaults(sandbox: &Sandbox) -> Vec<String> {
    let link = Sandbox::link().spec;
    sandbox
        .spec
        .on(Os::here())
        .unwrap_or_default()
        .items()
        .into_iter()
        .filter(|item| {
            item.internet
                .as_ref()
                .is_none_or(|i| i.per_minute.is_none())
        })
        .filter(|item| !link.covers(item, Os::here()).unwrap_or(false))
        .map(|item| link_sandbox_client::flags::shown(&item))
        .collect()
}

/// What it has, in a line: each thing it opens as its flag
/// (`--internet.hosts '*'`), then each team's folders it reaches.
pub fn summary(sandbox: &Sandbox) -> String {
    let mut parts: Vec<String> = sandbox
        .spec
        .on(Os::here())
        .unwrap_or_default()
        .items()
        .iter()
        .map(link_sandbox_client::flags::shown)
        .collect();
    if parts.is_empty() {
        parts.push("nothing beyond its harness's home".into());
    }
    for (team, folders) in &sandbox.spec.teams {
        let names = |names: &[String]| {
            names
                .iter()
                .map(|n| match n.as_str() {
                    link_harness_contract::harness::WHOLE => "all".to_string(),
                    n => format!("{n}/"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut reach = Vec::new();
        if !folders.written().is_empty() {
            reach.push(format!("writes {}", names(folders.written())));
        }
        if !folders.only_read().is_empty() {
            reach.push(format!("reads {}", names(folders.only_read())));
        }
        if reach.is_empty() {
            reach.push("reaches none".into());
        }
        parts.push(format!("team {team}'s folder: {}", reach.join(", ")));
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A write's lock is this machine's record, never a file beside the
    /// agent's state, which an archive carries.
    #[test]
    fn a_saved_files_lock_is_kept_out_of_its_settings_folder() {
        let agent = "lockcheck";
        save(agent, &Sandbox::link()).unwrap();
        let folder = path(agent).parent().unwrap().to_path_buf();
        let names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["sandbox.toml"], "{}", folder.display());
        assert!(!lock(agent).starts_with(&folder));
    }

    #[test]
    fn saved_grants_refuse_data_aliases_even_for_missing_files() {
        let home = link_test_kit::TempDir::new("grants-links");
        let safe = home.join("Safe");
        let data = home.join("data");
        link_files::create_private_dir(&safe).unwrap();
        link_files::create_private_dir(&data).unwrap();
        if !link_test_kit::gate::settings(&safe.join("sandbox.toml")) {
            return;
        }
        let target = data.join("sandbox.toml");
        link_files::write_private(&target, b"[files]\nwrite = [\"work\"]\n").unwrap();
        std::os::unix::fs::symlink(&target, safe.join("sandbox.toml")).unwrap();
        assert!(load_at(&safe.join("sandbox.toml")).is_err());
        std::os::unix::fs::symlink(&data, safe.join("agent")).unwrap();
        assert!(load_at(&safe.join("agent/sandbox.toml")).is_err());
        assert!(load_at(&safe.join("agent/missing.toml")).is_err());
    }

    #[test]
    fn a_missing_sandbox_file_is_closed_without_repair() {
        let home = link_test_kit::TempDir::new("sandbox-missing-closed");
        let path = home.join("missing/sandbox.toml");
        let sandbox = load_at(&path).unwrap();
        assert_eq!(sandbox, Sandbox::none());
        assert!(sandbox.folders().write.is_empty());
        assert!(sandbox.commands().is_none());
        assert!(sandbox.spec.internet.is_none());
        assert!(
            !path.exists(),
            "reading must not depend on repair succeeding"
        );
    }

    #[test]
    fn a_deleted_sandbox_file_is_recreated_closed_without_overwriting_an_owner_file() {
        let home = link_test_kit::TempDir::new("sandbox-recreate-closed");
        let path = home.join("Config/sandbox.toml");
        assert!(write_missing_at(&path, &Sandbox::none(), "main").unwrap());
        assert_eq!(load_at(&path).unwrap(), Sandbox::none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let own = "# Owner's restricted grant\n[files]\nwrite = [\"work\"]\n";
        std::fs::write(&path, own).unwrap();
        assert!(!write_missing_at(&path, &Sandbox::none(), "main").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), own);
        std::fs::remove_file(&path).unwrap();
        assert!(write_missing_at(&path, &Sandbox::none(), "main").unwrap());
        assert_eq!(load_at(&path).unwrap(), Sandbox::none());
    }

    fn parse(text: &str) -> Sandbox {
        Sandbox::parse(&format!("schema = 1\n{text}")).unwrap()
    }

    /// Link's sandbox file: the internet through its proxy, its work,
    /// instructions and skills written, so it changes how it works when
    /// asked, and its conversations only read.
    #[test]
    fn links_sandbox_file_is_the_default() {
        let links = Sandbox::link();
        assert_eq!(
            links.folders(),
            link_harness_contract::harness::Folders::default()
        );
        for folder in ["work", "instructions", "skills"] {
            assert!(links.folders().writes(folder), "{folder}");
        }
        assert!(!links.folders().writes("chats"));
        assert!(beyond_defaults(&links).is_empty());
        assert!(links.spec.behind_proxy());
        let commands = links.commands().unwrap();
        assert!(
            commands
                .toolchains
                .contains(&link_sandbox_contract::toolchains::Toolchain::Python)
        );
    }

    #[test]
    fn what_it_opens_beyond_links_is_said_as_flags() {
        let sandbox = parse(
            "[internet]\nhosts = [\"*\"]\ntcp = [\"github.com:22\"]\nper_minute = 600\n\
             [files]\nwrite = [\"work\"]\nread = [\"~\"]\n",
        );
        assert_eq!(
            beyond_defaults(&sandbox),
            ["--files.read '~'", "--internet.tcp github.com:22"]
        );
        let host = parse("[network]\nmode = \"host\"\n");
        assert!(!sandbox.spec.join(&host.spec).behind_proxy());
        assert!(summary(&Sandbox::none()).starts_with("nothing"));
    }

    /// `[commands]`: none without the section, a toolchain's programs by
    /// name, only those this machine has offered, and never `"*"`.
    #[test]
    fn its_commands_are_checked() {
        assert!(Sandbox::none().commands().is_none());
        let shell = parse("[commands]\ntoolchains = [\"shell\"]\n");
        assert!(shell.commands().unwrap().present("/nowhere").is_empty());
        let present = shell.commands().unwrap().present("/usr/bin:/bin");
        assert!(present.contains(&"sh".to_string()), "{present:?}");
        let some = parse("[commands]\nallow = [\"sh\", \"/bin/sh\", \"no-such-program-here\"]\n");
        assert_eq!(
            some.commands().unwrap().present("/usr/bin:/bin"),
            ["sh", "/bin/sh"]
        );
        // Any program, until a sandbox for toolchains replaces it.
        let any = parse("[commands]\nallow = [\"*\"]\ntoolchains = [\"rust\"]\n");
        assert_eq!(any.commands().unwrap().present("/nowhere"), ["*"]);
        for bad in [
            "[commands]\nallow = [\"../bin/sh\"]\n",
            "[commands]\nallow = [\"*\", \"sh\"]\n",
            "[commands]\ntoolchains = [\"cobol\"]\n",
            "[commands]\nrun = [\"sh\"]\n",
        ] {
            assert!(Sandbox::parse(bad).is_err(), "{bad:?}");
        }
    }
}

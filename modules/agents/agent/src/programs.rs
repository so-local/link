//! Where an agent's run and code tools find the programs it may start:
//! the folders of the `PATH` of whoever last ran `lnk agent start` here
//! (`here.toml`'s `path`), then the system's ([`sandbox_file::PATH`]), so
//! rustup's `cargo`, nvm's `node` or `~/.local/bin`'s `pyright` are found
//! as your shell finds them. A toolchain in your home folder is read in
//! the sandbox with what it needs beside it, its credentials hidden, and
//! its package caches moved to the agent's own folder, so nothing it
//! downloads reaches what you build (`toolchains.toml`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::sandbox_file;
use crate::settings::Settings;

/// `toolchains.toml`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    toolchain: Vec<Toolchain>,
    /// Each cache variable, by the folder of the agent's caches it names.
    caches: BTreeMap<String, String>,
}

/// What a program in one of your home's folders needs read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Toolchain {
    /// The folder, in your home, its programs are in, or one holding it.
    bin: String,
    /// The folders, in your home, read with it.
    read: Vec<String>,
    /// Of those, what's hidden: a credential.
    #[serde(default)]
    hide: Vec<String>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        toml::from_str(include_str!("toolchains.toml")).expect("toolchains.toml is checked")
    })
}

/// Of `path` (a `PATH`), the folders an agent's programs are found in:
/// each absolute and a folder here, none in Link's own (`~/.lnk`) or your
/// Link folder (`~/Link`, where agents' files are), none `system` (the
/// system's `PATH`) names; in order, once each.
pub fn recorded(path: &str, home: &Path, system: &str) -> Vec<String> {
    let closed = [home.join(".lnk"), home.join("Link")];
    let system: Vec<&str> = system.split(':').collect();
    let mut kept: Vec<String> = Vec::new();
    for folder in path.split(':') {
        let at = Path::new(folder);
        if !at.is_absolute()
            || folder.split('/').any(|part| part == "..")
            || closed.iter().any(|c| at.starts_with(c))
            || system.contains(&folder)
            || kept.iter().any(|k| k == folder)
            || !at.is_dir()
        {
            continue;
        }
        kept.push(folder.to_string());
    }
    kept
}

/// The folders of this process's `PATH`: whoever runs `lnk agent start`.
pub fn caller() -> Vec<String> {
    recorded(
        &std::env::var("PATH").unwrap_or_default(),
        &link_files::home_dir(),
        sandbox_file::PATH,
    )
}

/// The `PATH` agent `agent`'s programs are found on: the folders it was
/// started with, then the system's.
pub fn path(agent: &Settings) -> String {
    agent
        .programs
        .iter()
        .map(String::as_str)
        .chain([sandbox_file::PATH])
        .collect::<Vec<_>>()
        .join(":")
}

/// What a sandbox reads and hides of your home for `programs` (the
/// folders an agent was started with): each toolchain's folders, or a
/// folder no toolchain names alone; only those that are here.
fn reach(programs: &[String], home: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut read, mut hide) = (Vec::new(), Vec::new());
    let add = |list: &mut Vec<PathBuf>, path: PathBuf| {
        if path.exists() && !list.contains(&path) {
            list.push(path);
        }
    };
    for folder in programs {
        let Ok(inside) = Path::new(folder).strip_prefix(home) else {
            // Outside your home (`/opt/node22/bin`): the system's,
            // which every sandbox reads.
            continue;
        };
        match table()
            .toolchain
            .iter()
            .find(|t| inside.starts_with(&t.bin))
        {
            Some(toolchain) => {
                for path in &toolchain.read {
                    add(&mut read, home.join(path));
                }
                for path in &toolchain.hide {
                    add(&mut hide, home.join(path));
                }
            }
            None => add(&mut read, PathBuf::from(folder)),
        }
    }
    (read, hide)
}

/// `lnk sandbox run`'s flags for agent `agent`'s run and code tools: its
/// toolchains' folders read, their credentials hidden, and its own caches
/// written, made first.
pub fn flags(agent: &Settings) -> anyhow::Result<Vec<String>> {
    let (read, hide) = reach(&agent.programs, &link_files::home_dir());
    let caches = agent.data(link_files::layout::AgentData::Caches);
    link_files::create_private_dir(&caches)?;
    let spec = link_sandbox_client::Spec {
        files: Some(link_sandbox_contract::spec::Files {
            read: (!read.is_empty()).then(|| {
                read.into_iter()
                    .map(|path| path.display().to_string())
                    .collect()
            }),
            hide: (!hide.is_empty()).then(|| {
                hide.into_iter()
                    .map(|path| path.display().to_string())
                    .collect()
            }),
            write: Some(vec![caches.display().to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    link_spec::flags_of(&spec)
}

/// The variables agent `agent`'s run tool sets in each command's
/// sandbox (`--var NAME=VALUE`): each cache in its own caches folder.
pub fn vars(agent: &Settings) -> Vec<String> {
    let caches = agent.data(link_files::layout::AgentData::Caches);
    table()
        .caches
        .iter()
        .map(|(name, folder)| format!("{name}={}", caches.join(folder).display()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_reads_and_names_folders_in_a_home() {
        let table = table();
        assert!(!table.toolchain.is_empty() && !table.caches.is_empty());
        for toolchain in &table.toolchain {
            for path in std::iter::once(&toolchain.bin)
                .chain(&toolchain.read)
                .chain(&toolchain.hide)
            {
                assert!(
                    !path.starts_with('/') && !path.split('/').any(|p| p == ".."),
                    "{path}"
                );
            }
            for hidden in &toolchain.hide {
                assert!(
                    toolchain
                        .read
                        .iter()
                        .any(|r| hidden.starts_with(r.as_str())),
                    "{hidden} hides nothing it reads"
                );
            }
        }
    }

    /// Of a caller's `PATH`, the folders that are here, in its order,
    /// none of Link's, your Link folder's or the system's.
    #[test]
    fn a_callers_path_keeps_its_own_folders() {
        let home = link_test_kit::TempDir::new("programs-path");
        for folder in [
            ".cargo/bin",
            ".lnk/exec/bin",
            "Link/Agents/main/home",
            "opt/bin",
        ] {
            std::fs::create_dir_all(home.join(folder)).unwrap();
        }
        let at = |f: &str| home.join(f).display().to_string();
        let path = [
            at(".cargo/bin"),
            at(".lnk/exec/bin"),
            at("Link/Agents/main/home"),
            "relative/bin".into(),
            at("missing/bin"),
            "/usr/bin".into(),
            at("opt/bin"),
            at(".cargo/bin"),
        ]
        .join(":");
        assert_eq!(
            recorded(&path, &home, "/usr/bin:/bin"),
            [at(".cargo/bin"), at("opt/bin")]
        );
    }

    /// rustup's folder reads `~/.cargo` and `~/.rustup`, its credential
    /// hidden; a folder no toolchain names is read alone; one outside the
    /// home folder adds nothing.
    #[test]
    fn a_toolchain_in_the_home_folder_is_read_with_what_it_needs() {
        let home = link_test_kit::TempDir::new("programs-reach");
        for folder in [".cargo/bin", ".rustup", "tools/bin"] {
            std::fs::create_dir_all(home.join(folder)).unwrap();
        }
        std::fs::write(home.join(".cargo/credentials.toml"), "").unwrap();
        let at = |f: &str| home.join(f);
        let programs = [
            at(".cargo/bin").display().to_string(),
            at("tools/bin").display().to_string(),
            "/opt/node22/bin".into(),
        ];
        let (read, hide) = reach(&programs, &home);
        assert_eq!(read, [at(".cargo"), at(".rustup"), at("tools/bin")]);
        assert_eq!(hide, [at(".cargo/credentials.toml")]);
    }
}

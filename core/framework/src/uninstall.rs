//! The contract between the core (`lnk uninstall`, `lnk module remove`)
//! and a module that keeps something or runs a service:
//!
//! ```text
//! lnk-<module> uninstall --parts --json
//!         prints [`Kept`]: what it keeps, by part (`agents`, `buckets`, ...),
//!         each part's folders and files and its Keychain accounts, what
//!         uninstalling stops, and what stays the user's elsewhere
//! lnk-<module> uninstall [--delete <part>]...
//!         stops its background services, then deletes the parts named: run
//!         for each installed module before its program goes (`lnk module
//!         remove`: with no part)
//! ```
//!
//! The core knows no module's files: it asks each installed module, merges
//! parts of one name (the cloud adapters' `clouds`), and deletes what no
//! module claims (`rest`) itself. A module that doesn't answer keeps
//! nothing and runs nothing, as far as the core knows.

use std::path::PathBuf;

use anyhow::bail;
use serde::{Deserialize, Serialize};

mod credential;
pub use credential::{Credential, CredentialKind};

/// The uninstall protocol's flags, shared by every module.
#[derive(clap::Args, Serialize, Deserialize)]
#[command(about = None, long_about = None)]
#[serde(deny_unknown_fields)]
pub struct Flags {
    #[arg(long)]
    #[serde(default)]
    pub parts: bool,
    #[arg(long)]
    #[serde(default)]
    pub delete: Vec<String>,
    #[command(flatten)]
    #[serde(default, rename = "json")]
    pub output: link_spec::shared::Output,
}

/// What `uninstall --parts --json` prints.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    #[serde(default)]
    pub parts: Vec<Part>,
    /// What uninstalling stops, for the user (`Your agents stop: main.`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stops: String,
    /// What stays the user's outside this machine, and where to remove it
    /// (a bot, an app).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stays: String,
}

/// Something a module keeps, which `lnk uninstall` deletes only when the
/// user picks it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// Lowercase, as the user types it (`lnk uninstall --delete agents`).
    pub name: String,
    /// What deleting it loses, for the question.
    pub about: String,
    /// Its folders and files, absolute, whether or not they're there.
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    /// Its Keychain accounts: exact, or a prefix ending in `/`.
    #[serde(default)]
    pub keys: Vec<String>,
    /// How to keep a copy first, said when it's picked (a module-owned backup command).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub first: String,
    /// Of [`Part::paths`], those that hold keys, logins or a person's
    /// files: what an image of this machine must never carry
    /// (an image exporter must refuse a nonempty path). None,
    /// as from a module that doesn't say: all of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Vec<PathBuf>>,
    /// The credentials it holds, as a credential viewer shows them: what each
    /// is, where it's kept and what it may do, never its value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<Held>,
}

/// A credential a part holds, as a credential viewer shows it. Never its
/// value: that stays where it's kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Held {
    /// What it is, as Link's credentials page names it (`A model's key`).
    pub what: Credential,
    /// Whose or which (`main's, for anthropic`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub of: String,
    /// Where it's kept: the Keychain's item, or a file.
    pub kept: String,
    /// What it may do.
    pub may: String,
}

impl Held {
    /// Where a secret saved as Keychain item `account` ([`crate::keychain`])
    /// is kept, else in `file`.
    pub fn kept_in(account: &str, file: &std::path::Path) -> String {
        match link_keychain::available() {
            true => format!("the Keychain ({}:{account})", link_keychain::SERVICE),
            false => format!("{} (only yours)", file.display()),
        }
    }
}

impl Part {
    /// [`Part::secrets`]: those it names, else all its paths.
    pub fn secret_paths(&self) -> &[PathBuf] {
        self.secrets.as_deref().unwrap_or(&self.paths)
    }
}

impl Part {
    /// Whether Keychain account `account` is this part's.
    pub fn owns_key(&self, account: &str) -> bool {
        self.keys.iter().any(|k| match k.strip_suffix('/') {
            Some(prefix) => account.starts_with(&format!("{prefix}/")),
            None => account == k,
        })
    }
}

/// Runs `uninstall <args>` for a module keeping `kept`: `--parts --json`
/// prints it; else `stop` stops its services, `delete` does what deleting
/// the named parts needs beyond their paths and keys (a harness's traces
/// outside its home), and then their paths and keys go.
pub fn run(
    args: &Flags,
    kept: &Kept,
    stop: impl FnOnce() -> anyhow::Result<()>,
    delete: impl FnOnce(&[String]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    if args.parts {
        // Read by `lnk` and by callers of `uninstall --parts --json`,
        // whose output this parser chooses.
        link_format::select_json(args.output.json);
        let mut out = link_format::Out::with(link_settings::output::machine());
        match out.is_json() {
            true => out.reply(kept),
            false => out.data(format!("{}\n", serde_json::to_string(kept)?).as_bytes()),
        }
        out.finish(Ok(link_format::Done::shown()));
        return Ok(());
    }
    let picked = &args.delete;
    for name in picked {
        if !kept.parts.iter().any(|part| part.name == *name) {
            bail!("this module keeps no {name:?}");
        }
    }
    stop()?;
    if picked.is_empty() {
        return Ok(());
    }
    delete(picked)?;
    let mut failed = Vec::new();
    for part in kept.parts.iter().filter(|p| picked.contains(&p.name)) {
        for path in &part.paths {
            if let Err(err) = remove(path) {
                failed.push(format!("{}: {err}", path.display()));
            }
        }
        if link_keychain::available()
            && let Ok(accounts) = link_keychain::accounts()
        {
            for account in accounts.iter().filter(|a| part.owns_key(a)) {
                if let Err(err) = link_keychain::delete(account) {
                    failed.push(format!("the Keychain item link:{account}: {err:#}"));
                }
            }
        }
    }
    if !failed.is_empty() {
        bail!("couldn't remove {}", failed.join("; "));
    }
    Ok(())
}

/// `~/.lnk/<name>` for a part's paths.
pub fn settings(place: link_files::layout::Hidden) -> anyhow::Result<PathBuf> {
    place.try_path()
}

/// `<Link folder>/<name>` for a part's paths.
pub fn files(place: link_files::layout::Home) -> anyhow::Result<PathBuf> {
    place.try_path()
}

/// Removes `path`, a folder with all in it or a file, never following a
/// link; nothing if it isn't there.
pub fn remove(path: &std::path::Path) -> std::io::Result<()> {
    match path.symlink_metadata() {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_are_listed_stopped_and_deleted_by_name() {
        let dir = link_test_kit::TempDir::new("uninstall");
        std::fs::create_dir_all(dir.join("keep")).unwrap();
        std::fs::create_dir_all(dir.join("go/sub")).unwrap();
        std::fs::write(dir.join("go/sub/x"), "x").unwrap();
        let kept = Kept {
            parts: vec![
                Part {
                    name: "go".into(),
                    about: "what goes".into(),
                    paths: vec![dir.join("go"), dir.join("never-there")],
                    keys: vec!["go/".into()],
                    ..Part::default()
                },
                Part {
                    name: "keep".into(),
                    about: "what stays".into(),
                    paths: vec![dir.join("keep")],
                    ..Part::default()
                },
            ],
            ..Kept::default()
        };
        let (mut stopped, mut asked) = (false, Vec::new());
        run(
            &Flags {
                parts: false,
                delete: vec!["go".into()],
                output: Default::default(),
            },
            &kept,
            || {
                stopped = true;
                Ok(())
            },
            |parts| {
                asked = parts.to_vec();
                Ok(())
            },
        )
        .unwrap();
        assert!(stopped);
        assert_eq!(asked, ["go"]);
        assert!(!dir.join("go").exists() && dir.join("keep").exists());
        // No part: stopped, nothing deleted.
        run(
            &Flags {
                parts: false,
                delete: vec![],
                output: Default::default(),
            },
            &kept,
            || Ok(()),
            |_| panic!("nothing to delete"),
        )
        .unwrap();
        assert!(dir.join("keep").exists());
        assert!(
            run(
                &Flags {
                    parts: false,
                    delete: vec!["other".into()],
                    output: Default::default()
                },
                &kept,
                || Ok(()),
                |_| Ok(())
            )
            .is_err()
        );
        // Malformed syntax is refused by the parser before uninstall can run.
        #[derive(clap::Parser)]
        struct Invocation {
            #[command(flatten)]
            flags: Flags,
        }
        assert!(<Invocation as clap::Parser>::try_parse_from(["uninstall", "--delete"]).is_err());
        assert!(kept.parts[0].owns_key("go/a") && !kept.parts[0].owns_key("gone"));
    }
}

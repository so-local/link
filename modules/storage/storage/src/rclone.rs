//! rclone, the engine: installed by the module (a pinned version, checked
//! against the SHA-256 below), run with its own config file in
//! `~/.lnk/safe/storage`, and with every `RCLONE_*` variable removed so
//! a user's own rclone setup never mixes in.
//!
//! On a Mac the secret settings Link writes ([`SECRET_KEYS`]: cloud keys,
//! encryption passwords) are in the Keychain instead of the config file,
//! `rclone-keychain.toml` beside it listing which, read once per command;
//! each rclone run gets them as `RCLONE_CONFIG_<SECTION>_<KEY>` variables, which rclone reads as
//! it would the file's lines.
//!
//! An AWS or Google Cloud bucket signs in as its `lnk cloud` account, and
//! nothing of that sign-in is in the config file: each run on it gets the
//! account's (its adapter's `storage`, asked once per command)
//! the same way, and nothing from the shell's own `AWS_*` or
//! `GOOGLE_APPLICATION_CREDENTIALS`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use crate::said;
use anyhow::{bail, ensure};
use link_format::{Product, WithinExt, sentence};
use link_secretive::Duplicate;
use link_secretive::Secret;

use crate::clouds::{Cloud, Clouds, Kind};
use crate::engine::{
    Download, Engine, Found, Listed, Proof, Sending, Sent, Upload, Written, send_and_check_one,
};

mod buckets;
mod config;
mod running;
mod transfers;
use buckets::{FileList, class_flag, object, pending_in, uploader_hashes};
pub use config::{Kept, SECRET_KEYS, config_in, held, kept, remove_section, section};
use config::{file_section, keychain, keychain_list_path, keychain_settings, put_section};
use running::sweep_runs;

// The rclone release `lnk storage` runs. Changing it means updating
// `externals.json` from that release's `SHA256SUMS`, after checking its PGP
// signature (`gpg --verify SHA256SUMS`) against rclone's release key,
// fingerprint FBF737ECE9F8AB18604BD2AC93935E02FF3B54FA (`docs/content/KEYS`
// in rclone's repository). 1.75.1's was checked.
include!(concat!(env!("OUT_DIR"), "/external-pins.rs"));
pub const VERSION: &str = RCLONE_VERSION;
static PIN: std::sync::LazyLock<serde_json::Value> =
    std::sync::LazyLock::new(|| serde_json::from_str(RCLONE_JSON).expect("validated rclone pin"));

/// How many names one `rclone backend encode` is given.
const ENCODE_BATCH: usize = 1000;

/// Where the pinned rclone lives, installed or not: `LNK_RCLONE` when
/// set (a different rclone, for tests), else in `dir`.
pub fn program_in(dir: &Path) -> PathBuf {
    match std::env::var_os("LNK_RCLONE") {
        Some(path) => PathBuf::from(path),
        None => dir.join(format!("rclone-v{VERSION}")),
    }
}

/// The pinned rclone in `dir`, downloaded and checked first if it isn't
/// there. Older versions there are deleted.
pub async fn install(dir: &Path) -> anyhow::Result<PathBuf> {
    let program = program_in(dir);
    if program.is_file() {
        return Ok(program);
    }
    ensure!(
        std::env::var_os("LNK_RCLONE").is_none(),
        "LNK_RCLONE is {}, which isn't a file",
        program.display()
    );
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let artifact = PIN["platforms"]
        .get(&platform)
        .within("no rclone artifact for this platform")?;
    let sum = artifact["sha256"]
        .as_str()
        .within("rclone pin has no digest")?;
    let url = artifact["url"]
        .as_str()
        .within("rclone pin has no source")?;
    let member = artifact["program"]
        .as_str()
        .within("rclone pin has no archive member")?;
    said::note(sentence!(
        "Installing {}, the engine lnk storage uses (about 25 MB).",
        Product::new(format!("rclone {VERSION}"))
    ));
    let max_bytes = artifact["max_bytes"]
        .as_u64()
        .within("rclone pin has no size bound")?;
    let archive = link_artifact::Cache::shared()?
        .bytes(url, sum, max_bytes)
        .await?;
    let actual = link_digest::sha256(&archive);
    ensure!(
        actual == sum,
        "the rclone download's checksum is wrong; not installing it"
    );
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
        .within("the rclone download isn't a zip")?;
    let mut entry = zip
        .by_name(member)
        .within("the rclone download has no rclone in it")?;
    link_files::create_private_dir(dir)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes)?;
    // Unique, so two installs at once can't write into each other's file;
    // written as a program never open for writing here, so it runs at once.
    let tmp = dir.join(format!(
        "rclone-download-{}-{}",
        std::process::id(),
        crate::files::now_nanos()
    ));
    let written =
        link_files::write_program(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &program));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    for old in std::fs::read_dir(dir)?.flatten() {
        let old_name = old.file_name();
        let old_name = old_name.to_string_lossy();
        if old_name.starts_with("rclone-v") && old.path() != program {
            let _ = std::fs::remove_file(old.path());
        }
    }
    Ok(program)
}

#[derive(Duplicate)]
pub struct Rclone {
    program: PathBuf,
    /// The config file: every connected cloud's settings and keys.
    config: PathBuf,
    /// Clouds that sign in as a `lnk cloud` account, by their section,
    /// besides those `clouds.toml` says (one being connected).
    signing_in: RefCell<BTreeMap<String, String>>,
    /// Each account's sign-in, by account, once asked.
    #[duplicate]
    sign_ins: RefCell<BTreeMap<String, link_cloud_contract::Reply>>,
    /// The settings in the Keychain, once fetched, with the list of them
    /// they were fetched for: fetched again when it changes.
    #[duplicate]
    keychain: RefCell<Option<(String, Settings)>>,
    /// `rclone backend features` of each remote, once asked.
    features: RefCell<BTreeMap<String, serde_json::Value>>,
    /// Sending beside other clouds: this one's name, before each line
    /// rclone says.
    beside: Option<String>,
    /// The config files made for each run ([`Rclone::run_config`]), each
    /// read back once its run is done.
    runs: RefCell<Vec<Run>>,
}

/// A config file made for one run: the sections it was given, as the
/// config file and the Keychain hold them, and the keys a sign-in added.
#[derive(Clone, Debug)]
struct Run {
    path: PathBuf,
    written: Vec<(String, Vec<(String, String)>)>,
    added: Vec<(String, String)>,
}

/// Settings, as (section, key, value).
type Settings = Vec<(String, String, Secret)>;

impl Drop for Rclone {
    fn drop(&mut self) {
        self.settle_runs();
    }
}

impl Rclone {
    /// The engine, installed into `dir` first if needed, with its config
    /// file there.
    pub fn in_dir(dir: &Path) -> anyhow::Result<Self> {
        let program = link_runtime::block_on(install(&dir.join("engine")))?;
        Self::with(program, dir)
    }

    /// `program` as the engine, with its config file in `dir`.
    pub fn with(program: PathBuf, dir: &Path) -> anyhow::Result<Self> {
        let config = config_in(dir);
        if !config.exists() {
            link_files::write_private(&config, b"")?;
        }
        sweep_runs(&config.with_file_name("runs"));
        Ok(Self {
            program,
            config,
            signing_in: RefCell::default(),
            sign_ins: RefCell::default(),
            keychain: RefCell::default(),
            features: RefCell::default(),
            beside: None,
            runs: RefCell::default(),
        })
    }

    /// The engine for `~/.lnk/safe/storage`.
    pub fn new() -> anyhow::Result<Self> {
        Self::in_dir(&crate::files::config_dir()?)
    }

    /// The engine for `~/.lnk/safe/storage` as it's installed: an error
    /// when it isn't, never installed by a store's call.
    pub fn installed() -> anyhow::Result<Self> {
        let dir = crate::files::config_dir()?;
        let program = program_in(&dir.join("engine"));
        ensure!(
            program.exists(),
            "lnk storage's engine isn't installed here: lnk storage create installs it"
        );
        Self::with(program, &dir)
    }

    /// Object `path` (`aws-personal-crypt:.link/x`): its bytes, at most
    /// `max`, none when there's none.
    pub fn object_get(&self, path: &str, max: u64) -> anyhow::Result<Option<Vec<u8>>> {
        // One byte past the most: an object that large is refused.
        let output = self.output(&["cat", "--count", &max.saturating_add(1).to_string(), path])?;
        match output.status.code() {
            Some(0) => {
                ensure!(
                    output.stdout.len() as u64 <= max,
                    "{} is larger than {max} bytes",
                    link_format::clean(path)
                );
                Ok(Some(output.stdout))
            }
            // Its folder, or it, isn't there.
            Some(3 | 4) => Ok(None),
            _ if said(&output).contains("not found") => Ok(None),
            _ => bail!("{}", said(&output)),
        }
    }

    /// Writes object `path` with `data`, whole: in place of any there.
    pub fn object_put(&self, path: &str, data: &[u8]) -> anyhow::Result<()> {
        use std::io::Write as _;
        let mut child = self
            .command(&["rcat", "--size", &data.len().to_string(), path])?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .within("running rclone")?;
        if let Some(mut stdin) = child.stdin.take() {
            // rclone says why on stderr if it stops before reading it all.
            let _ = stdin.write_all(data);
        }
        let output = child.wait_with_output().within("running rclone")?;
        ensure!(output.status.success(), "{}", said(&output));
        Ok(())
    }

    /// Removes object `path`; nothing when there's none.
    pub fn object_delete(&self, path: &str) -> anyhow::Result<()> {
        let output = self.output(&["deletefile", path])?;
        match output.status.code() {
            Some(0 | 3 | 4) => Ok(()),
            _ if said(&output).contains("not found") => Ok(()),
            _ => bail!("{}", said(&output)),
        }
    }

    /// Every object under folder `path` (ending `/`), by its path from
    /// there, and its size: none when there's no such folder.
    pub fn object_list(&self, path: &str) -> anyhow::Result<Vec<(String, u64)>> {
        let output = self.output(&["lsjson", "-R", "--files-only", "--no-mimetype", path])?;
        if !output.status.success() {
            return match output.status.code() {
                Some(3 | 4) => Ok(Vec::new()),
                _ if said(&output).contains("not found") => Ok(Vec::new()),
                _ => bail!("{}", said(&output)),
            };
        }
        let items: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).within("reading rclone's listing")?;
        Ok(items
            .into_iter()
            .filter_map(|item| {
                Some((
                    item["Path"].as_str()?.to_string(),
                    item["Size"].as_u64().unwrap_or(0),
                ))
            })
            .collect())
    }

    /// Section `section` signs in as `lnk cloud` account `account`, from
    /// now on (a cloud being connected, not in `clouds.toml` yet).
    pub fn sign_in_as(&self, section: &str, account: &str) {
        self.signing_in
            .borrow_mut()
            .insert(section.to_string(), account.to_string());
    }
}

/// The last few lines rclone wrote to stderr, for an error message.
fn said(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(3)..].join("\n");
    if tail.is_empty() {
        format!("rclone failed ({})", output.status)
    } else {
        link_format::clean(&tail.replace('\n', " / "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_has_a_checksum() {
        for platform in [
            "linux-x86_64",
            "linux-aarch64",
            "macos-x86_64",
            "macos-aarch64",
        ] {
            let sum = PIN["platforms"][platform]["sha256"].as_str().unwrap();
            assert_eq!(sum.len(), 64);
        }
    }
}

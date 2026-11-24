//! Running the cloud's own CLI as an account, and the files it reads.

use link_format::WithinExt;
use link_secretive::Secret;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::bail;
use link_cloud_contract::Account;

/// A cloud CLI, acting as one account.
pub struct Cli {
    pub program: PathBuf,
    pub env: BTreeMap<String, Secret>,
}

impl Cli {
    /// `program` (`aws`, `gcloud`) as found by [`find`], acting with `env`.
    pub fn new(program: &str, env: BTreeMap<String, Secret>) -> anyhow::Result<Self> {
        let Some(path) = find(program) else {
            bail!(
                "Link on {} needs its CLI, {program}, which isn't installed: \
                 lnk cloud connect {} installs it, or {}",
                if program == "aws" {
                    "AWS"
                } else {
                    "Google Cloud"
                },
                if program == "aws" { "aws" } else { "gcp" },
                match program {
                    "aws" => "https://aws.amazon.com/cli (or brew install awscli)",
                    _ =>
                        "https://cloud.google.com/sdk/docs/install (or brew install --cask google-cloud-sdk)",
                }
            );
        };
        Ok(Self { program: path, env })
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = link_secretive::process::command(&self.program);
        // Only the account's settings: the shell's own can't redirect it.
        for (key, _) in std::env::vars_os() {
            let key = key.to_string_lossy();
            if key.starts_with("AWS_") || key.starts_with("CLOUDSDK_") {
                command.env_remove(&*key);
            }
        }
        // The account's: aws reads an account's keys from its environment or its own files, nowhere else.
        for (name, value) in &self.env {
            link_secretive::process::give(&mut command, name, value);
        }
        command.args(args).stdin(Stdio::null());
        command
    }

    /// Runs it with `args`; its output, or an error with what it said.
    pub fn run(&self, args: &[&str]) -> anyhow::Result<Vec<u8>> {
        let output = self
            .command(args)
            .output()
            .within(format!("running {}", self.program.display()))?;
        if !output.status.success() {
            bail!(
                "{}",
                link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
            );
        }
        Ok(output.stdout)
    }

    /// Runs it with `args` on this program's own stdin, stdout and
    /// stderr, carrying what flows through them (a tunnel's bytes), until
    /// it ends; an error when it fails.
    pub fn carry(&self, args: &[&str]) -> anyhow::Result<()> {
        let status = self
            .command(args)
            .stdin(Stdio::inherit())
            .status()
            .within(format!("running {}", self.program.display()))?;
        if !status.success() {
            bail!(
                "{} {} ended: {status}",
                self.program.display(),
                args.join(" ")
            );
        }
        Ok(())
    }

    /// [`Cli::run`], its output read as JSON.
    pub fn json(&self, args: &[&str]) -> anyhow::Result<serde_json::Value> {
        let output = self.run(args)?;
        if output.iter().all(u8::is_ascii_whitespace) {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_slice(&output).within(format!(
            "reading {} {}",
            self.program.display(),
            args.join(" ")
        ))
    }
}

/// `a` and `b`, side by side: each asks the cloud on its own (a CLI call
/// is mostly its start), so two that don't need each other overlap.
pub fn both<A: Send, B: Send>(
    a: impl FnOnce() -> A + Send,
    b: impl FnOnce() -> B + Send,
) -> (A, B) {
    std::thread::scope(|scope| {
        let a = scope.spawn(a);
        let b = b();
        let a = a
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        (a, b)
    })
}

/// A program: `LNK_AWS` or `LNK_GCLOUD` (tests), else on the PATH
/// (absolute folders only), else where Link installed it.
pub fn find(program: &str) -> Option<PathBuf> {
    let var = format!("LNK_{}", program.to_uppercase());
    if let Some(path) = std::env::var_os(var) {
        return Some(PathBuf::from(path));
    }
    let on_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(program))
            .find(|p| p.is_file())
    });
    on_path.or_else(|| Some(installed(program)?).filter(|p| p.is_file()))
}

/// Where `lnk cloud connect` installs `program` when you let it: in
/// Link's settings folder, for you only.
pub fn installed(program: &str) -> Option<PathBuf> {
    match program {
        "aws" => Some(
            link_files::layout::Hidden::Aws
                .try_path()
                .ok()?
                .join("bin")
                .join("aws"),
        ),
        "gcloud" => Some(
            link_files::layout::Hidden::Gcp
                .try_path()
                .ok()?
                .join("google-cloud-sdk/bin/gcloud"),
        ),
        _ => None,
    }
}

/// A short, stable hash of `text` (FNV-1a), for names.
pub fn short_hash(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:08x}", hash >> 32)
}

/// A file only the user can read, deleted when dropped: what the CLI
/// reads with `file://`.
pub struct PrivateFile {
    dir: std::path::PathBuf,
    path: std::path::PathBuf,
}

impl PrivateFile {
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for PrivateFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn private_file(name: &str, contents: &str) -> anyhow::Result<PrivateFile> {
    let dir = link_files::private_temp_dir("cloud")?;
    let path = dir.join(name);
    link_files::write_private(&path, contents.as_bytes())?;
    Ok(PrivateFile { dir, path })
}

/// The AWS CLI acting as one account, in one region: what boxes and
/// buckets run it as.
pub struct Aws {
    pub cli: Cli,
    pub region: String,
}

impl Aws {
    pub fn new(account: &Account, region: Option<&str>) -> anyhow::Result<Self> {
        let region = region
            .map(str::to_string)
            .or_else(|| account.region.clone())
            .or_else(|| Some(account.place.clone()).filter(|p| !p.is_empty()))
            .unwrap_or_else(|| "us-east-1".into());
        let mut env = account.child_environment();
        env.insert("AWS_REGION".into(), region.clone().into());
        // Output as JSON, without a pager, whatever the user's aws config says.
        env.insert("AWS_PAGER".into(), Secret::default());
        env.insert("AWS_DEFAULT_OUTPUT".into(), "json".to_string().into());
        Ok(Self {
            cli: Cli::new("aws", env)?,
            region,
        })
    }
}

//! Running the cloud's own CLI as an account, and the files it reads.

use link_format::WithinExt;
use link_secretive::{Duplicate, Secret};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::bail;
use link_cloud_contract::Account;

/// A cloud CLI, acting as one account.
pub struct Cli {
    pub program: PathBuf,
    pub env: BTreeMap<String, Secret>,
    config: Option<PathBuf>,
}

impl Cli {
    /// `program` (`aws`, `gcloud`) as found by [`find`], acting with `env`.
    pub fn new(program: &str, mut env: BTreeMap<String, Secret>) -> anyhow::Result<Self> {
        // Existing saved accounts may predate the explicit SDK override.
        // GOOGLE_APPLICATION_CREDENTIALS alone does not select gcloud's
        // identity. Bind every SDK call to the account's same saved file.
        if program == "gcloud"
            && let Some(file) = env
                .get(crate::connect::CREDENTIALS)
                .map(Duplicate::duplicate)
        {
            env.insert(crate::connect::OVERRIDE.into(), file);
        }
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
        // A saved SDK token-file or impersonation setting takes precedence
        // over credential_file_override. File-backed account calls get a
        // fresh private SDK configuration for this client's lifetime.
        let config = if program == "gcloud" && env.contains_key(crate::connect::CREDENTIALS) {
            let config = link_files::private_temp_dir("gcp-sdk")?;
            env.insert(
                "CLOUDSDK_CONFIG".into(),
                config.display().to_string().into(),
            );
            Some(config)
        } else {
            None
        };
        Ok(Self {
            program: path,
            env,
            config,
        })
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
        // The account's: gcloud reads an account's settings (where its credentials are) from its environment.
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

    /// [`Cli::run`] for a change to the project's IAM policy: tried again
    /// while it `waits` (a new account IAM doesn't know yet, another
    /// change made at the same time).
    #[expect(
        clippy::disallowed_methods,
        reason = "backs off while Google Cloud makes a new account known"
    )]
    pub fn grant(&self, args: &[&str]) -> anyhow::Result<Vec<u8>> {
        for wait in [1, 2, 4, 8, 16, 30] {
            match self.run(args) {
                Err(err) if waits(&err.to_string()) => {
                    std::thread::sleep(std::time::Duration::from_secs(wait));
                }
                done => return done,
            }
        }
        self.run(args)
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

impl Drop for Cli {
    fn drop(&mut self) {
        if let Some(config) = &self.config {
            let _ = std::fs::remove_dir_all(config);
        }
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

/// gcloud acting as one account, in one project and zone: what boxes and
/// buckets run it as.
pub struct Gcp {
    pub cli: Cli,
    pub account: String,
    /// The project's ID, which names it and holds no secret.
    pub project: String,
    pub zone: String,
}

impl Gcp {
    pub fn new(account: &Account, zone: Option<&str>) -> anyhow::Result<Self> {
        let project = account
            .project
            .clone()
            .within("the gcp account has no project: lnk cloud connect gcp --project <id>")?;
        let zone = zone
            .map(str::to_string)
            .or_else(|| Some(account.place.clone()).filter(|p| !p.is_empty()))
            .unwrap_or_else(|| "us-central1-a".into());
        let mut env = account.child_environment();
        env.insert("CLOUDSDK_CORE_PROJECT".into(), project.clone().into());
        // Never a question: gcloud would wait on one.
        env.insert(
            "CLOUDSDK_CORE_DISABLE_PROMPTS".into(),
            "1".to_string().into(),
        );
        let mut cli = Cli::new("gcloud", env)?;
        // A region (`europe-west2`), as a box spec may say: its first zone.
        let zone = match is_region(&zone) {
            true => first_zone(&cli, &zone)?,
            false => zone,
        };
        cli.env
            .insert("CLOUDSDK_COMPUTE_ZONE".into(), zone.clone().into());
        Ok(Self {
            cli,
            account: account.name.clone(),
            project,
            zone,
        })
    }
}

/// Whether `place` is a region (`europe-west2`) rather than a zone in it
/// (`europe-west2-b`): a zone ends in a dash and one letter.
pub fn is_region(place: &str) -> bool {
    let bytes = place.as_bytes();
    !(bytes.len() > 2
        && bytes[bytes.len() - 2] == b'-'
        && bytes[bytes.len() - 1].is_ascii_lowercase())
}

/// The first zone, by name, of `region` that's up.
fn first_zone(cli: &Cli, region: &str) -> anyhow::Result<String> {
    let zones = cli.json(&["compute", "zones", "list", "--format", "json"])?;
    let mut found: Vec<String> = zones
        .as_array()
        .into_iter()
        .flatten()
        .filter(|z| z["status"] == "UP")
        .filter(|z| {
            z["region"]
                .as_str()
                .and_then(|r| r.rsplit('/').next())
                .is_some_and(|r| r == region)
        })
        .filter_map(|z| z["name"].as_str().map(str::to_string))
        .collect();
    found.sort();
    found
        .into_iter()
        .next()
        .within(format!("no zone of {} is up", link_format::clean(region)))
}

/// Whether an IAM policy change that failed with `text` succeeds when
/// tried again: IAM can take a minute to know a new account, and the
/// policy is read, changed and written whole, so two boxes made at once
/// conflict, and Google asks for the change to be retried.
fn waits(text: &str) -> bool {
    (text.contains("Service account") && text.contains("does not exist"))
        || text.contains("concurrent policy changes")
}

#[cfg(test)]
mod waiting {
    /// A new account and a concurrent change are waited out; a refusal isn't.
    #[test]
    fn an_iam_change_waits_out_a_new_account_and_a_concurrent_one() {
        assert!(super::waits(
            "Service account lnk-job-x@p.iam does not exist."
        ));
        assert!(super::waits(
            "Resource in projects [p:setIamPolicy] is the subject of a conflict: There were \
             concurrent policy changes. Please retry the whole read-modify-write"
        ));
        assert!(!super::waits("PERMISSION_DENIED"));
    }
}

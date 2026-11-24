//! TPM-bound keeper files. The TPM seals a small HPKE recipient; only
//! its opaque TPM blobs and the encrypted payload touch persistent disk.
//! Context files live on tmpfs. A TPM error never falls back to plaintext.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::{
    Secret,
    secret::{About, Recipient, Sealed},
};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

const MARKER: &[u8] = b"LNK-TPM1\n";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept {
    public: Vec<u8>,
    private: Vec<u8>,
    payload: Sealed<Secret<Vec<u8>>>,
}

link_files::specification!(Kept, include_str!("at_rest.schema.toml"));

fn about() -> About {
    About {
        owner: "keeper".into(),
        kind: "disk-store".into(),
        version: 1,
    }
}

pub fn has_tpm() -> bool {
    link_secretive::test_var("LNK_TEST_TPM").is_some() || Path::new("/dev/tpmrm0").exists()
}

struct Operation(PathBuf);
impl Operation {
    fn new() -> anyhow::Result<Self> {
        let root = link_secretive::test_var("LNK_TEST_TPM_RUN")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if super::keeper::running_as_keeper() {
                    "/run/lnk-secrets".into()
                } else {
                    "/run/lnk-bootstrap".into()
                }
            });
        if !root.exists() {
            link_files::create_private_dir(&root)?;
        }
        let dir = root.join(link_files::random_token()?);
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let operation = Self(dir);
        operation.run(
            "createprimary",
            &["-C", "o", "-g", "sha256", "-G", "ecc", "-c", "primary.ctx"],
            None,
        )?;
        Ok(operation)
    }

    fn run(
        &self,
        action: &str,
        args: &[&str],
        input: Option<&Secret>,
    ) -> anyhow::Result<Secret<Vec<u8>>> {
        let program = link_secretive::test_var("LNK_TEST_TPM").unwrap_or_else(|| "tpm2".into());
        let mut command = link_secretive::process::command(program);
        command
            .arg(action)
            .args(["-T", "device:/dev/tpmrm0"])
            .args(args)
            .current_dir(&self.0);
        let mut child = command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("starting the TPM storage adapter")?;
        let writer = input.map(|input| {
            let mut stdin = child.stdin.take().expect("piped TPM input");
            let input = input.duplicate();
            std::thread::spawn(move || stdin.write_all(input.expose().as_bytes()))
        });
        let output = child.wait_with_output()?;
        if let Some(writer) = writer {
            writer
                .join()
                .map_err(|_| anyhow::anyhow!("writing the TPM handoff"))??;
        }
        ensure!(
            output.status.success(),
            "TPM {action} refused: no plaintext fallback"
        );
        Ok(Secret::new(output.stdout))
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        // The kernel resource manager closes this client's transient
        // objects at process exit. Never flush another client's handles.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Open a private file. A keeper upgrade migrates plaintext while its
/// store lock is held; bootstrap's root launcher also requests migration.
pub fn read(path: &Path, migrate: bool) -> anyhow::Result<Option<Secret<Vec<u8>>>> {
    let _lock = link_files::lock_file(&path.with_extension("tpm-lock"))?;
    let bytes = match std::fs::read(path) {
        Ok(bytes) => Secret::new(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("reading the keeper's credential"),
    };
    let Some(sealed) = bytes.expose().strip_prefix(MARKER) else {
        if migrate && has_tpm() {
            write_locked(path, bytes.expose())?;
        }
        return Ok(Some(bytes));
    };
    let kept: Kept =
        link_files::saved::parse_json(sealed).context("invalid TPM storage envelope")?;
    let operation = Operation::new()?;
    link_files::write_private(&operation.0.join("secret.pub"), &kept.public)?;
    link_files::write_private(&operation.0.join("secret.priv"), &kept.private)?;
    operation.run(
        "load",
        &[
            "-C",
            "primary.ctx",
            "-u",
            "secret.pub",
            "-r",
            "secret.priv",
            "-c",
            "object.ctx",
        ],
        None,
    )?;
    let opened = operation.run("unseal", &["-c", "object.ctx"], None)?;
    let receiver = opened.expose_map(|bytes| String::from_utf8_lossy(bytes).into_owned());
    let recipient = Recipient::from_kept(&receiver).context("invalid TPM receiver")?;
    Ok(Some(recipient.open_sealed(&kept.payload, &about())?))
}

/// Seal first, then atomically replace and sync the file and directories
/// before acknowledging a first-boot handoff. Without a TPM the file is
/// the keeper's private fallback, never the working user's.
pub fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let _lock = link_files::lock_file(&path.with_extension("tpm-lock"))?;
    write_locked(path, bytes)
}

fn write_locked(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if has_tpm() {
        let operation = Operation::new()?;
        let recipient = Recipient::new();
        operation.run(
            "create",
            &[
                "-C",
                "primary.ctx",
                "-a",
                "fixedtpm|fixedparent|userwithauth",
                "-i",
                "-",
                "-u",
                "secret.pub",
                "-r",
                "secret.priv",
            ],
            Some(&recipient.keep()),
        )?;
        let kept = Kept {
            public: std::fs::read(operation.0.join("secret.pub"))?,
            private: std::fs::read(operation.0.join("secret.priv"))?,
            payload: Sealed::seal(&Secret::new(bytes.to_vec()), recipient.seal_to(), about())?,
        };
        let mut output = MARKER.to_vec();
        output.extend(link_files::saved::print_json(&kept)?);
        link_files::write_private(path, &output)?;
    } else {
        link_files::write_private(path, bytes)?;
    }
    std::fs::File::open(path)?.sync_all()?;
    let mut parent = path.parent();
    while let Some(dir) = parent {
        if !dir.as_os_str().is_empty() {
            std::fs::File::open(dir)?.sync_all()?;
        }
        parent = dir.parent();
    }
    Ok(())
}

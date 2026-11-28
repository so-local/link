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

/// The envelope's format, as `at_rest.schema.toml` declares it: the
/// payload is sealed at its `current` schema and opens from its `oldest`,
/// so a change to what's sealed is a step of that schema, and a payload
/// sealed before it is sealed again at the current one when it's read.
fn format() -> &'static link_files::schema::Format {
    <Kept as link_files::schema::Specification>::format()
}

fn about() -> About {
    About {
        owner: "keeper".into(),
        kind: "disk-store".into(),
        version: format().current,
    }
}

pub fn has_tpm() -> bool {
    link_secretive::test_var("LNK_TEST_TPM").is_some() || Path::new("/dev/tpmrm0").exists()
}

/// Whether this keeper's stores are sealed to a TPM, as its unit says
/// (`lnk secret keep --sealed`, written by `lnk secret keeper create` on
/// a machine with one): once set, nothing is written in plaintext.
static SEALED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Notes, for the rest of this process, that its stores are sealed to a
/// TPM: a write finding none is refused, never written in plaintext.
pub fn require_sealing() {
    SEALED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// How a write keeps its bytes: whether there's a TPM, and whether the
/// keeper's configuration says its stores are sealed to one.
#[derive(Clone, Copy)]
struct Sealing {
    tpm: bool,
    required: bool,
}

impl Sealing {
    fn here() -> Sealing {
        Sealing {
            tpm: has_tpm(),
            required: SEALED.load(std::sync::atomic::Ordering::SeqCst),
        }
    }
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
            write_locked(path, bytes.expose(), Sealing::here())?;
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
    let opened = recipient.open_sealed_within(&kept.payload, &about(), format().oldest)?;
    // Sealed at an older version of the envelope: sealed again at the
    // current one, under the same lock.
    if migrate && kept.payload.about().version != format().current {
        drop(operation);
        write_locked(path, opened.expose(), Sealing::here())?;
    }
    Ok(Some(opened))
}

/// Seal first, then atomically replace and sync the file and directories
/// before acknowledging a first-boot handoff. Without a TPM the file is
/// the keeper's private fallback, never the working user's.
pub fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let _lock = link_files::lock_file(&path.with_extension("tpm-lock"))?;
    write_locked(path, bytes, Sealing::here())
}

fn write_locked(path: &Path, bytes: &[u8], sealing: Sealing) -> anyhow::Result<()> {
    ensure!(
        sealing.tpm || !sealing.required,
        "this keeper seals its stores to a TPM, and none answers at /dev/tpmrm0: {} isn't \
         written in plaintext",
        path.display()
    );
    if sealing.tpm {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A keeper whose configuration says its stores are sealed to a TPM
    /// writes nothing when it finds none, never plaintext.
    #[test]
    fn a_sealed_keeper_never_writes_plaintext() {
        let dir = link_test_kit::TempDir::short("at-rest-sealed");
        let file = dir.join("secrets.toml");
        let refused = write_locked(
            &file,
            b"value = \"hunter2\"",
            Sealing {
                tpm: false,
                required: true,
            },
        )
        .unwrap_err();
        let refused = format!("{refused:#}");
        assert!(refused.contains("isn't written in plaintext"), "{refused}");
        assert!(!refused.contains("hunter2"), "{refused}");
        assert!(!file.exists());
        // A keeper that never had a TPM keeps its private file.
        write_locked(
            &file,
            b"value = \"hunter2\"",
            Sealing {
                tpm: false,
                required: false,
            },
        )
        .unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"value = \"hunter2\"");
    }

    /// What the envelope seals is at its schema's version, the one a step
    /// changes: so a sealed format changes by the schema's own channel.
    #[test]
    fn the_sealed_payload_is_at_the_envelopes_schema() {
        assert_eq!(about().version, format().current);
        assert!(format().oldest <= format().current);
    }
}

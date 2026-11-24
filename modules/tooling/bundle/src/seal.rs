//! Sealing a bundle: its `install.sh`, `VERSION`, `COMMIT` and
//! `ADVISORIES.toml`, the sums of every file, and their signature, checked
//! against the signer it was made for before anything leaves the machine.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};

use crate::NAMESPACE;
use crate::cli::Build;
use crate::constants::{LINK, STAGING};

/// A signer: its allowed-signers lines, where they came from, and
/// where its secret is kept when the Safe keeps it.
pub struct Signer {
    pub name: String,
    pub lines: String,
    /// `~/.lnk/safe/bundles/signers/<name>/secret`, for a signer named
    /// rather than given as a file.
    pub secret: Option<PathBuf>,
    /// Whether this run made it.
    pub made: bool,
}

/// The folder a named signer is kept in: `~/.lnk/safe/bundles/signers/<name>`.
pub fn signer_dir(name: &str) -> anyhow::Result<PathBuf> {
    Ok(link_files::layout::Hidden::BundleSigners
        .try_path()?
        .join(name))
}

/// A name a signer is kept by: lowercase letters, digits and dashes.
fn a_name(named: &str) -> bool {
    !named.is_empty()
        && named
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Signer {
    /// `link` and `staging` are Link's own signers, their public lines
    /// from the public source's `scripts/release/`. Any other name is a
    /// signer kept in the Safe, made there on first use. Anything else is
    /// a signers file. A named signer's secret is kept beside it, so
    /// `--signing-secret` is needed only for a file.
    pub fn read(named: &str, public: &Path) -> anyhow::Result<Self> {
        let (path, secret, made) = match named {
            LINK | STAGING => (
                public.join(format!("scripts/release/{named}-signers")),
                Some(signer_dir(named)?.join("secret")),
                false,
            ),
            name if a_name(name) => {
                let dir = signer_dir(name)?;
                let made = !dir.join("signers").exists();
                if made {
                    make(name, &dir)?;
                }
                (dir.join("signers"), Some(dir.join("secret")), made)
            }
            other => (PathBuf::from(other), None, false),
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading the signers {}", path.display()))?;
        let lines: String = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| format!("{l}\n"))
            .collect();
        link_core::release::signer_keys(&lines)
            .with_context(|| format!("{} holds no Ed25519 signer", path.display()))?;
        ensure!(
            !lines.contains('\''),
            "{} holds a ', which install.sh can't hold",
            path.display()
        );
        Ok(Signer {
            name: named.to_string(),
            lines,
            secret,
            made,
        })
    }

    /// Whether it's one of Link's own signers, whose public line is
    /// committed and whose secret only Link's CI holds.
    pub fn links(&self) -> bool {
        [LINK, STAGING].contains(&self.name.as_str())
    }

    /// Its one line, as a dev or prod build's `lnk` is compiled to trust it.
    pub fn own_line(&self) -> anyhow::Result<&str> {
        let mut lines = self.lines.lines();
        let (Some(line), None) = (lines.next(), lines.next()) else {
            bail!("a dev or prod build's signer is one line, its own");
        };
        ensure!(
            line.starts_with("link-release namespaces=\"link-release\" ssh-ed25519 "),
            "a dev or prod build's signer reads link-release namespaces=\"link-release\" ssh-ed25519 <key> <name>"
        );
        Ok(line)
    }
}

/// A signer of your own kept in the Safe, as `lnk bundle signer show`
/// shows it: its folder, its allowed-signers file and the one line in it.
pub struct Kept {
    pub name: String,
    pub folder: PathBuf,
    pub signers: PathBuf,
    pub line: String,
}

/// Refuses a name no signer of your own is kept by: one not a signer's
/// name, or one of Link's own, whose lines are the public source's.
fn own_name(name: &str) -> anyhow::Result<()> {
    ensure!(
        a_name(name),
        "{name:?} isn't a signer's name: lowercase letters, digits and dashes"
    );
    ensure!(
        ![LINK, STAGING].contains(&name),
        "{name} is Link's own signer, its line in the public source's scripts/release/{name}-signers"
    );
    Ok(())
}

/// The signer of your own kept as `name`: an error when none is.
pub fn kept(name: &str) -> anyhow::Result<Kept> {
    own_name(name)?;
    let folder = signer_dir(name)?;
    let signers = folder.join("signers");
    ensure!(
        signers.is_file(),
        "no signer {name} is kept in {}: lnk bundle signer create {name} makes it",
        folder.display()
    );
    let signer = Signer::read(name, Path::new("."))?;
    let line = signer.own_line()?.to_string();
    Ok(Kept {
        name: name.to_string(),
        folder,
        signers,
        line,
    })
}

/// Makes the signer of your own `name`, refused when one is kept already.
pub fn create(name: &str) -> anyhow::Result<Kept> {
    own_name(name)?;
    let folder = signer_dir(name)?;
    ensure!(
        !folder.join("signers").exists(),
        "a signer {name} is kept already, in {}: lnk bundle signer show {name}",
        folder.display()
    );
    make(name, &folder)?;
    kept(name)
}

/// A new Ed25519 signer kept in `dir`: its secret, which only its owner
/// reads, and its one allowed-signers line, written last, so a signer is
/// kept once its line is. What an interrupted run left before it, a
/// secret nothing trusts yet, is removed and the signer made again.
fn make(name: &str, dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let secret = dir.join("secret");
    for left in [secret.clone(), dir.join("secret.pub")] {
        match std::fs::remove_file(&left) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                return Err(err).with_context(|| format!("removing {}", left.display()));
            }
            _ => {}
        }
    }
    crate::run(
        "ssh-keygen",
        &[
            "-q",
            "-t",
            "ed25519",
            "-N",
            "",
            "-C",
            &format!("lnk-bundle-{name}"),
            "-f",
            &secret.to_string_lossy(),
        ],
        dir,
    )
    .context("making the signer")?;
    let public = std::fs::read_to_string(dir.join("secret.pub"))?;
    let key: Vec<&str> = public.split_whitespace().take(2).collect();
    ensure!(key.len() == 2, "ssh-keygen wrote no public half");
    std::fs::remove_file(dir.join("secret.pub"))?;
    let partial = dir.join("signers.partial");
    std::fs::write(
        &partial,
        format!(
            "{NAMESPACE} namespaces=\"{NAMESPACE}\" {} {} {name}\n",
            key[0], key[1]
        ),
    )?;
    std::fs::rename(&partial, dir.join("signers"))?;
    Ok(())
}

/// Which signer each build may be signed by: staging by the staging
/// signer its own source names in `scripts/release/staging-signers`,
/// which its `lnk` is compiled to trust, by whatever name it's kept here
/// (a source of your own may name a signer of yours there); a dev build by a signer of your own, neither Link's nor
/// the staging signer, so no released or staging `lnk` installs it; a prod
/// build by any but the staging signer, Link's for Link's public release.
pub fn check_signer(build: Build, signer: &Signer, public: &Path) -> anyhow::Result<()> {
    // Link's own keys, none where a file holds none yet (the staging
    // signer before it's made).
    let keys = |named: &str| -> anyhow::Result<Vec<[u8; 32]>> {
        let file = public.join(format!("scripts/release/{named}-signers"));
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        if !text
            .lines()
            .any(|l| l.contains("ssh-ed25519 ") && !l.trim_start().starts_with('#'))
        {
            return Ok(Vec::new());
        }
        link_core::release::signer_keys(&text)
    };
    let own = link_core::release::signer_keys(&signer.lines)?;
    let none_of = |link: Vec<[u8; 32]>| own.iter().all(|k| !link.contains(k));
    match build {
        Build::Staging => {
            let staging = keys(STAGING)?;
            ensure!(
                signer.name == STAGING || own.iter().all(|k| staging.contains(k)),
                "a staging build is signed by the staging signer its source's scripts/release/staging-signers names: --signer {STAGING}"
            );
        }
        Build::Dev => ensure!(
            none_of(keys(LINK)?.into_iter().chain(keys(STAGING)?).collect()),
            "a dev build is signed by a signer of your own, never Link's or the staging signer"
        ),
        Build::Prod => ensure!(
            none_of(keys(STAGING)?),
            "a prod build is never signed by the staging signer: --signer {LINK} for Link's, or one of your own"
        ),
    }
    Ok(())
}

/// What a sealed bundle says beside its programs.
pub struct Seal<'a> {
    pub version: &'a str,
    pub commit: &'a str,
    pub public: &'a Path,
    pub signer: &'a Signer,
    pub secret: &'a Path,
    pub files: &'a [PathBuf],
}

/// Seals the programs in `dist`: writes the release's own files, sums
/// every file and signs the sums with `seal.secret`, checked against
/// `seal.signer`.
pub fn seal(dist: &Path, seal: &Seal) -> anyhow::Result<()> {
    let install = std::fs::read_to_string(seal.public.join("core/cli/install.sh"))
        .context("reading core/cli/install.sh")?;
    let mut replaced = 0;
    let install: String = install
        .lines()
        .map(|line| {
            if line.starts_with("RELEASE_SIGNERS=") {
                replaced += 1;
                format!("RELEASE_SIGNERS='{}'", seal.signer.lines.trim_end())
            } else if line == "MIN_VERSION=''" {
                format!("MIN_VERSION='{}'", seal.version)
            } else {
                line.to_string()
            }
        })
        .map(|l| l + "\n")
        .collect();
    ensure!(
        replaced == 1,
        "install.sh has no one RELEASE_SIGNERS line to replace"
    );
    let write = |name: &str, text: &[u8]| -> anyhow::Result<()> {
        let path = dist.join(name);
        ensure!(
            !path.exists(),
            "{name} is made by sealing, and a bundle already holds one"
        );
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    };
    write("install.sh", install.as_bytes())?;
    write("VERSION", format!("{}\n", seal.version).as_bytes())?;
    write("COMMIT", format!("{}\n", seal.commit).as_bytes())?;
    write(
        "ADVISORIES.toml",
        &std::fs::read(seal.public.join("scripts/release/advisories.toml"))
            .context("reading scripts/release/advisories.toml")?,
    )?;
    for file in seal.files {
        let name = file
            .file_name()
            .with_context(|| format!("{} names no file", file.display()))?
            .to_string_lossy();
        write(
            &name,
            &std::fs::read(file).with_context(|| format!("reading {}", file.display()))?,
        )?;
    }
    let mut sums = String::new();
    for name in crate::files(dist)? {
        let sum = link_digest::sha256_of(std::fs::File::open(dist.join(&name))?)?;
        sums.push_str(&format!("{sum}  {name}\n"));
    }
    std::fs::write(dist.join("SHA256SUMS"), &sums)?;
    sign(dist, seal)
}

fn sign(dist: &Path, seal: &Seal) -> anyhow::Result<()> {
    private(seal.secret)?;
    crate::run(
        "ssh-keygen",
        &[
            "-q",
            "-Y",
            "sign",
            "-n",
            NAMESPACE,
            "-f",
            &seal.secret.to_string_lossy(),
            "SHA256SUMS",
        ],
        dist,
    )
    .context("signing SHA256SUMS")?;
    let sums = std::fs::read(dist.join("SHA256SUMS"))?;
    let signature = std::fs::read_to_string(dist.join("SHA256SUMS.sig"))?;
    let keys = link_core::release::signer_keys(&seal.signer.lines)?;
    link_core::release::verify(&sums, &signature, &keys).with_context(|| {
        format!(
            "{} didn't sign it as the signer {} expects",
            seal.secret.display(),
            seal.signer.name
        )
    })
}

/// A signing secret's file only its owner reads.
fn private(key: &Path) -> anyhow::Result<()> {
    let meta = std::fs::symlink_metadata(key)
        .with_context(|| format!("reading the signing secret {}", key.display()))?;
    ensure!(
        meta.is_file(),
        "the signing secret {} isn't a plain file",
        key.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            meta.permissions().mode() & 0o077 == 0,
            "the signing secret {} can be read by others: chmod 600 it",
            key.display()
        );
    }
    Ok(())
}

/// Checks every file `dir`'s `SHA256SUMS` lists against its sum, and that
/// it lists every file there but itself and its signature.
pub fn check_sums(dir: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let text = std::fs::read_to_string(dir.join("SHA256SUMS"))
        .with_context(|| format!("{} has no SHA256SUMS", dir.display()))?;
    let mut listed = Vec::new();
    for line in text.lines() {
        let (sum, name) = line
            .split_once("  ")
            .with_context(|| format!("SHA256SUMS has a line it can't read: {line:?}"))?;
        let actual = link_digest::sha256_of(
            std::fs::File::open(dir.join(name))
                .with_context(|| format!("{name} is listed and missing"))?,
        )?;
        ensure!(
            actual.eq_ignore_ascii_case(sum),
            "{name} doesn't match its sum"
        );
        listed.push((name.to_string(), sum.to_string()));
    }
    for name in crate::files(dir)? {
        ensure!(
            name == "SHA256SUMS"
                || name == "SHA256SUMS.sig"
                || listed.iter().any(|(n, _)| *n == name),
            "{name} is in the bundle and not in its SHA256SUMS"
        );
    }
    Ok(listed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK_LINE: &str = "link-release namespaces=\"link-release\" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIH9+OiZ/SeNGnTeS94BYhtoALcCIHMNqITeJujSzZeHW link\n";

    /// A release signer's line for the testing kit's public key `name`,
    /// so no key but a test's is written in this file.
    fn line(name: &str) -> String {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../packages/test-kit/fixtures");
        let public = std::fs::read_to_string(fixtures.join(format!("{name}.pub"))).unwrap();
        format!(
            "link-release namespaces=\"link-release\" {}\n",
            public.trim()
        )
    }

    fn source(staging: &str) -> link_test_kit::TempDir {
        let public = link_test_kit::TempDir::new("bundle-signers");
        let release = public.join("scripts/release");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::write(release.join("link-signers"), LINK_LINE).unwrap();
        std::fs::write(release.join("staging-signers"), staging).unwrap();
        public
    }

    fn named(name: &str, lines: &str) -> Signer {
        Signer {
            name: name.into(),
            lines: lines.into(),
            secret: None,
            made: false,
        }
    }

    /// A signer of your own is made once, then shown by its name: its
    /// folder, its file and its one line; made again, or one of Link's
    /// own, it's refused, and one never made isn't shown.
    #[test]
    fn a_signer_of_your_own_is_made_once_and_shown_by_its_name() {
        // ssh-keygen has no flag that only answers, so it's found by name.
        if !link_test_kit::gate::program("sh", &["-c", "command -v ssh-keygen"]) {
            return;
        }
        let missing = kept("seal-test-never").err().expect("none was made");
        assert!(missing.to_string().contains("signer create"), "{missing}");
        let made = create("seal-test").unwrap();
        assert_eq!(made.folder, signer_dir("seal-test").unwrap());
        assert_eq!(made.signers, made.folder.join("signers"));
        assert!(made.folder.join("secret").is_file());
        assert!(
            made.line
                .starts_with("link-release namespaces=\"link-release\" ssh-ed25519 ")
                && made.line.ends_with(" seal-test"),
            "{}",
            made.line
        );
        let shown = kept("seal-test").unwrap();
        assert_eq!(shown.line, made.line);
        let again = create("seal-test").err().expect("made twice");
        assert!(again.to_string().contains("kept already"), "{again}");
        for refused in [LINK, STAGING, "Not-A-Name"] {
            assert!(create(refused).is_err(), "{refused}");
            assert!(kept(refused).is_err(), "{refused}");
        }
        // A run interrupted before its line was written is made again.
        let left = signer_dir("seal-test-left").unwrap();
        std::fs::create_dir_all(&left).unwrap();
        for file in ["secret", "secret.pub", "signers.partial"] {
            std::fs::write(left.join(file), "left by an interrupted run\n").unwrap();
        }
        let redone = create("seal-test-left").unwrap();
        assert!(redone.line.ends_with(" seal-test-left"), "{}", redone.line);
        assert!(!left.join("secret.pub").exists());
        assert!(!left.join("signers.partial").exists());
    }

    /// A source of your own may name a signer of yours its staging
    /// signer, and a staging build of it is signed by that signer, kept
    /// under any name; a signer the source doesn't name is refused.
    #[test]
    fn a_staging_build_is_signed_by_the_staging_signer_its_source_names() {
        let (ours, other) = (line("test-staging-key"), line("test-release-key"));
        let public = source(&format!("# the staging signer\n{ours}"));
        check_signer(Build::Staging, &named("qa", &ours), &public).unwrap();
        let refused = check_signer(Build::Staging, &named("qa", &other), &public).unwrap_err();
        assert!(refused.to_string().contains("staging-signers"), "{refused}");
        let both = format!("{ours}{other}");
        assert!(check_signer(Build::Staging, &named("qa", &both), &public).is_err());
        // A dev build is never signed by the source's staging signer.
        assert!(check_signer(Build::Dev, &named("qa", &ours), &public).is_err());
        // A source naming no staging signer yet takes only Link's by name.
        let unnamed = source("# none yet\n");
        assert!(check_signer(Build::Staging, &named("qa", &ours), &unnamed).is_err());
    }
}

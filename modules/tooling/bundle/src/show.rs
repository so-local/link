//! `lnk bundle show`: what a bundle holds, its sums checked and its
//! signature checked against the signer asked for, or Link's signer.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use link_actions::value::Words;

use crate::cli::Target;
use crate::seal::Signer;

/// What a bundle holds.
pub struct Shown {
    /// The bundle's folder, which `LNK_RELEASES_URL=file://` names.
    pub root: PathBuf,
    pub dir: PathBuf,
    pub version: Option<String>,
    /// Each program archive: its program and target.
    pub programs: Vec<(String, Target)>,
    /// Every other file, by name.
    pub files: Vec<String>,
    pub checksums: Checksums,
    pub signature: Signature,
}

/// Whether the inventory was checked against supplied file digests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checksums {
    Absent,
    Verified,
}

pub enum Signature {
    None,
    /// Signed by the signer named.
    By(String),
    /// Signed, but not by Link's signer, and no other signer was named.
    NotLink,
}

impl Shown {
    /// The verification status a person reads after the file inventory.
    pub fn verification(&self) -> link_format::Sentence {
        if self.checksums == Checksums::Absent {
            return link_format::sentence!("No checksums or signature were supplied.");
        }
        match &self.signature {
            Signature::By(by) => {
                link_format::sentence!("Its sums match, signed by {}.", by.clone())
            }
            Signature::NotLink => link_format::sentence!(
                "Its sums match, not by Link's signer: name its signer with --signer to check it."
            ),
            Signature::None => link_format::sentence!("Its sums match, and it isn't signed."),
        }
    }
}

/// The one version folder of a bundle's root, or `folder` itself when it's
/// a version folder already.
pub fn version_dir(folder: &Path) -> anyhow::Result<PathBuf> {
    let download = folder.join("download");
    if !download.is_dir() {
        // A part as CI hands it on: its one `v<version>` folder.
        let versions: Vec<PathBuf> = std::fs::read_dir(folder)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.join("VERSION").is_file())
            .collect();
        return Ok(match versions.as_slice() {
            [one] => one.clone(),
            _ => folder.to_path_buf(),
        });
    }
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&download)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    match versions.len() {
        1 => Ok(versions.remove(0)),
        0 => bail!("{} holds no version", download.display()),
        _ => bail!(
            "{} holds more than one version: name its download/v<version> folder",
            download.display()
        ),
    }
}

/// Whether `value` names a kept bundle rather than a folder: a word with
/// no path separator, so `devbox` is always the kept one and never a
/// `./devbox` folder that happens to be here, whose `bin/lnk` nothing
/// has checked.
fn is_a_name(value: &Path) -> bool {
    let text = value.to_string_lossy();
    !text.is_empty()
        && !text.contains(std::path::MAIN_SEPARATOR)
        && !text.contains('/')
        && text != "."
        && text != ".."
}

/// `folder`'s bundle, checked: a folder, given by a path, or a version
/// kept in `~/.lnk/data/bundles`, given by its name ([`is_a_name`]).
pub fn show(folder: &Path, signer: Option<&str>, here: &Path) -> anyhow::Result<Shown> {
    let kept = match is_a_name(folder) {
        false => None,
        true => {
            let named = folder.to_string_lossy();
            let kept = crate::make::kept(&named)?;
            anyhow::ensure!(
                kept.is_some(),
                "no bundle {named} is kept here: lnk bundle create {named} makes it \
                 (lnk bundle list), and a folder of that name is ./{named}"
            );
            kept
        }
    };
    let folder = kept.as_deref().unwrap_or(folder);
    // Absolute, so what reads its files from another folder (tar), and
    // whoever is given it (a box made from it), finds it.
    let root =
        std::fs::canonicalize(folder).with_context(|| format!("finding {}", folder.display()))?;
    let folder = root.as_path();
    let dir = version_dir(folder)?;
    let version = std::fs::read_to_string(dir.join("VERSION"))
        .ok()
        .map(|v| v.trim().to_string());
    anyhow::ensure!(
        signer.is_none() || dir.join("SHA256SUMS.sig").is_file(),
        "the bundle has no signature to verify against the requested signer"
    );
    let (listed, checksums) = if dir.join("SHA256SUMS").is_file() {
        let listed = crate::seal::check_sums(&dir)?
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>();
        (listed, Checksums::Verified)
    } else {
        (crate::files(&dir)?, Checksums::Absent)
    };
    let mut programs = Vec::new();
    let mut files = Vec::new();
    for name in listed {
        match target_of(&name) {
            Some(target) => programs.push((inside(&dir.join(&name))?, target)),
            None => files.push(name),
        }
    }
    let signature = if dir.join("SHA256SUMS.sig").is_file() {
        let sums = std::fs::read(dir.join("SHA256SUMS"))?;
        let sig = std::fs::read_to_string(dir.join("SHA256SUMS.sig"))?;
        match signer {
            Some(named) => {
                let lines = if named == crate::constants::LINK {
                    link_core::release::LINK_SIGNER.to_string()
                } else {
                    Signer::read(named, here)?.lines
                };
                let keys = link_core::release::signer_keys(&lines)?;
                link_core::release::verify(&sums, &sig, &keys)
                    .with_context(|| format!("its signature isn't {named}'s"))?;
                Signature::By(named.to_string())
            }
            None => {
                let keys = link_core::release::signer_keys(link_core::release::LINK_SIGNER)?;
                match link_core::release::verify(&sums, &sig, &keys) {
                    Ok(()) => Signature::By(crate::constants::LINK.into()),
                    Err(_) => Signature::NotLink,
                }
            }
        }
    } else {
        Signature::None
    };
    Ok(Shown {
        root,
        dir,
        version,
        programs,
        files,
        checksums,
        signature,
    })
}

/// An asset's target: `lnk-measure-x86_64-unknown-linux-musl.tar.gz` is for
/// that target. Its program is the one file inside it ([`inside`]).
pub(crate) fn target_of(name: &str) -> Option<Target> {
    let rest = name.strip_prefix("lnk-")?.strip_suffix(".tar.gz")?;
    Target::released().find(|t| rest.ends_with(&format!("-{}", t.word())))
}

/// The one program an archive holds, by its name inside.
fn inside(archive: &Path) -> anyhow::Result<String> {
    let dir = archive.parent().unwrap_or(Path::new("."));
    let listed = crate::run("tar", &["-tzf", &archive.to_string_lossy()], dir)?;
    let names: Vec<&str> = listed.lines().filter(|l| !l.is_empty()).collect();
    match names.as_slice() {
        [one] => Ok(one.trim_start_matches("./").to_string()),
        _ => bail!("{} holds more than one file", archive.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsigned_part() -> link_test_kit::temp::TempDir {
        let part = link_test_kit::temp::TempDir::new("bundle-inspection");
        std::fs::write(part.join("VERSION"), "0.0.4\n").unwrap();
        part
    }

    #[test]
    fn inspection_reports_absent_checksums_without_claiming_a_match() {
        let part = unsigned_part();
        let shown = show(&part, None, &part).unwrap();
        let status = shown.verification().to_string();
        assert_eq!(status, "No checksums or signature were supplied.");
    }

    #[test]
    fn inspection_requires_a_signature_when_a_signer_is_explicit() {
        let part = unsigned_part();
        let result = show(&part, Some("link"), &part);
        let Err(error) = result else {
            panic!("an explicit signer accepted an unsigned part");
        };
        assert!(error.to_string().contains("signature"), "{error:#}");
    }

    #[test]
    fn inspection_checks_supplied_sums_before_reporting_a_match() {
        let part = unsigned_part();
        let version = std::fs::read(part.join("VERSION")).unwrap();
        std::fs::write(
            part.join("SHA256SUMS"),
            format!("{}  VERSION\n", link_digest::sha256(&version)),
        )
        .unwrap();
        let shown = show(&part, None, &part).unwrap();
        assert_eq!(shown.checksums, Checksums::Verified);
        assert_eq!(
            shown.verification().to_string(),
            "Its sums match, and it isn't signed."
        );
        assert!(show(&part, Some("link"), &part).is_err());
        std::fs::write(part.join("VERSION"), "changed\n").unwrap();
        let Err(error) = show(&part, None, &part) else {
            panic!("a changed file was reported as matching its checksum");
        };
        assert!(error.to_string().contains("VERSION"), "{error:#}");
    }

    #[test]
    fn an_asset_names_its_target() {
        assert_eq!(
            target_of("lnk-core-x86_64-unknown-linux-musl.tar.gz"),
            Some(Target::X8664UnknownLinuxMusl)
        );
        assert_eq!(
            target_of("lnk-my-server-aarch64-apple-darwin.tar.gz"),
            Some(Target::Aarch64AppleDarwin)
        );
        assert_eq!(target_of("SHA256SUMS"), None);
        assert_eq!(
            target_of("lnk-core-host.tar.gz"),
            None,
            "host is never an archive's"
        );
    }

    #[test]
    fn each_release_target_says_its_architecture() {
        use crate::cli::Architecture;
        let arm: Vec<Target> = Target::released()
            .filter(|t| t.architecture() == Some(Architecture::Arm64))
            .collect();
        assert_eq!(
            arm,
            [Target::Aarch64AppleDarwin, Target::Aarch64UnknownLinuxMusl]
        );
        assert_eq!(Target::Host.architecture(), None);
        for architecture in Architecture::all() {
            let linux = architecture.linux();
            assert_eq!(linux.architecture(), Some(architecture));
            assert!(linux.word().ends_with("-unknown-linux-musl"), "{linux:?}");
        }
        let here = Target::here().unwrap();
        assert_eq!(Target::Host.resolved().unwrap(), here);
        assert!(here.architecture().is_some());
    }

    #[test]
    fn a_bundle_by_its_path_is_shown_from_anywhere() {
        let dir = link_test_kit::temp::TempDir::new("bundle-path");
        let dist = dir.path().join("mine/download/v0.0.4-dev.1");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("VERSION"), "0.0.4-dev.1\n").unwrap();
        // A program's archive, which tar reads from a folder of its own.
        let inside = dir.path().join("lnk-core");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::write(inside.join("lnk"), "a program").unwrap();
        crate::run(
            "tar",
            &[
                "-czf",
                &dist
                    .join("lnk-core-x86_64-unknown-linux-musl.tar.gz")
                    .to_string_lossy(),
                "lnk",
            ],
            &inside,
        )
        .unwrap();
        let relative = pathdiff(&dir.path().join("mine"));
        let shown = show(&relative, None, dir.path()).unwrap();
        assert!(shown.root.is_absolute(), "{}", shown.root.display());
        assert_eq!(
            shown.root,
            std::path::absolute(dir.path().join("mine")).unwrap()
        );
    }

    /// `to`, relative to the folder the test runs in.
    fn pathdiff(to: &Path) -> PathBuf {
        let here = std::env::current_dir().unwrap();
        let ups = here.components().count() - 1;
        let mut path = PathBuf::new();
        for _ in 0..ups {
            path.push("..");
        }
        path.join(to.strip_prefix("/").unwrap())
    }

    #[test]
    fn a_word_is_a_kept_name_and_a_path_a_folder() {
        for name in ["devbox", "prodbox", "0.0.4-dev.1"] {
            assert!(is_a_name(Path::new(name)), "{name}");
        }
        for folder in ["./devbox", "bundles/devbox", "/srv/qa/devbox", ".", ".."] {
            assert!(!is_a_name(Path::new(folder)), "{folder}");
        }
    }

    #[test]
    fn a_name_never_made_says_how_to_make_it() {
        let here = link_test_kit::temp::TempDir::new("bundle-show");
        let Err(said) = show(Path::new("never-made-box"), None, here.path()) else {
            panic!("a bundle never made was shown");
        };
        let said = said.to_string();
        assert!(said.contains("lnk bundle create never-made-box"), "{said}");
    }
}

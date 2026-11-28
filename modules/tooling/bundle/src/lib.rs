//! `lnk bundle`: any set of Link's programs, built from source for its
//! targets into one signed folder in the published release layout, which
//! `install.sh`, `lnk upgrade` and `lnk box create` already read.

pub mod cli;
pub mod compile;
pub mod constants;
pub mod discover;
pub mod make;
pub mod outside;
pub mod seal;
pub mod show;
pub mod spec;

#[cfg(test)]
link_test_kit::program!(unit);

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, bail, ensure};

/// The signature namespace every release is signed in.
pub const NAMESPACE: &str = link_core::release::NAMESPACE;

/// Runs `program` with `args` in `dir`, failing with what it said.
pub fn run(program: &str, args: &[&str], dir: &Path) -> anyhow::Result<String> {
    let output = link_secretive::process::command(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {program}"))?;
    if !output.status.success() {
        bail!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `path` made absolute against `dir`.
pub fn absolute(path: &Path, dir: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    }
}

/// The archive one program is released in: `lnk` as `core`, a module by its
/// name without `lnk-`, any other program by its own name.
pub fn asset(program: &str, target: &str) -> String {
    let id = match program {
        "lnk" => "core",
        other => other.strip_prefix("lnk-").unwrap_or(other),
    };
    format!("lnk-{id}-{target}.tar.gz")
}

/// The workspace version a source's `Cargo.toml` names.
pub fn workspace_version(source: &Path) -> anyhow::Result<String> {
    let text = std::fs::read_to_string(source.join("Cargo.toml"))
        .with_context(|| format!("reading {}/Cargo.toml", source.display()))?;
    let table: toml::Table = toml::from_str(&text)?;
    table
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("{}/Cargo.toml names no workspace version", source.display()))
}

/// The commit a staging or dev build is made from, which its version
/// names, so every machine building one commit gives it one version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    /// When it was committed, in seconds since 1970.
    pub time: u64,
    /// Its full hash.
    pub hash: String,
}

impl Commit {
    /// `HEAD` of the repository `dir` is in.
    pub fn head(dir: &Path) -> anyhow::Result<Self> {
        let said = run(
            "git",
            &["show", "--no-patch", "--format=%ct %H", "HEAD"],
            dir,
        )?;
        let (time, hash) = said
            .trim()
            .split_once(' ')
            .with_context(|| format!("git gave {said:?} for HEAD"))?;
        ensure!(
            hash.len() > constants::HASH_CHARS && hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "git gave {hash:?} for HEAD's hash"
        );
        Ok(Commit {
            time: time
                .parse()
                .with_context(|| format!("git gave {time:?} for HEAD's time"))?,
            hash: hash.to_ascii_lowercase(),
        })
    }

    /// Its build ID: its time in UTC to the minute, which orders builds,
    /// then `g` and the start of its hash, which names it. The `g` keeps
    /// the hash a word, since a version's part of digits alone can't
    /// start with zero.
    pub fn id(&self) -> String {
        format!(
            "{}.g{}",
            stamp(self.time),
            &self.hash[..constants::HASH_CHARS]
        )
    }
}

/// Whether `id` is a build ID ([`Commit::id`]).
pub fn is_build_id(id: &str) -> bool {
    id.split_once(".g").is_some_and(|(time, hash)| {
        time.len() == 12
            && time.bytes().all(|b| b.is_ascii_digit())
            && hash.len() == constants::HASH_CHARS
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// `secs` since 1970, in UTC, as `yyyymmddhhmm`.
pub fn stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (hour, minute) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}{hour:02}{minute:02}")
}

/// The version a bundle is built at: the workspace's for a prod build, one
/// carrying its commit's build ID for staging and dev, or the one asked
/// for when it fits.
pub fn version(
    build: cli::Build,
    asked: Option<&str>,
    workspace: &str,
    commit: &Commit,
) -> anyhow::Result<String> {
    match (build, asked) {
        (cli::Build::Prod, None) => Ok(workspace.to_string()),
        (cli::Build::Prod, Some(v)) => {
            ensure!(
                v == workspace,
                "a prod build is built at its workspace's version, {workspace}, not {v}: set the workspace's version first"
            );
            Ok(v.to_string())
        }
        (kind, None) => Ok(format!("{workspace}-{}.{}", kind.word(), commit.id())),
        (kind, Some(v)) => {
            let suffix = format!("-{}.", kind.word());
            let stamped = v.split_once(&suffix).is_some_and(|(_, id)| is_build_id(id));
            ensure!(
                stamped,
                "a {} build's version is <version>-{}.<yyyymmddhhmm>.g<commit>, not {v}",
                kind.word(),
                kind.word()
            );
            Ok(v.to_string())
        }
    }
}

/// Every regular file directly in `dir`, by name, sorted.
pub fn files(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "{} holds {}, which isn't a file",
            dir.display(),
            entry.file_name().to_string_lossy()
        );
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_program_has_its_asset() {
        let t = "x86_64-unknown-linux-musl";
        assert_eq!(asset("lnk", t), "lnk-core-x86_64-unknown-linux-musl.tar.gz");
        assert_eq!(
            asset("lnk-measure", t),
            "lnk-measure-x86_64-unknown-linux-musl.tar.gz"
        );
        assert_eq!(
            asset("my-server", t),
            "lnk-my-server-x86_64-unknown-linux-musl.tar.gz"
        );
    }

    #[test]
    fn versions_follow_their_build() {
        use cli::Build::*;
        // Committed 2026-11-09 07:20 UTC.
        let at = Commit {
            time: 1_794_182_400 + 7 * 3600 + 20 * 60,
            hash: "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b".into(),
        };
        assert_eq!(version(Prod, None, "0.0.4", &at).unwrap(), "0.0.4");
        assert!(version(Prod, Some("0.0.5"), "0.0.4", &at).is_err());
        assert_eq!(
            version(Staging, None, "0.0.4", &at).unwrap(),
            "0.0.4-staging.202611090720.g1a2b3c4d5e6f"
        );
        let given = "0.0.4-dev.202611090000.g0123456789ab";
        assert!(version(Dev, Some(given), "0.0.4", &at).is_ok());
        assert!(version(Dev, Some("0.0.4-dev.202611090000"), "0.0.4", &at).is_err());
        let staged = "0.0.4-staging.202611090000.g0123456789ab";
        assert!(version(Dev, Some(staged), "0.0.4", &at).is_err());
        assert!(version(Prod, Some(staged), "0.0.4", &at).is_err());
    }

    #[test]
    fn a_build_id_is_a_time_and_a_hash() {
        assert!(is_build_id("202611090720.g1a2b3c4d5e6f"));
        assert!(!is_build_id("202611090720"));
        assert!(!is_build_id("202611090720.g1A2B3C4D5E6F"));
        assert!(!is_build_id("202611090720.g1a2b3c4d5e6"));
        assert!(!is_build_id("20261109072.g1a2b3c4d5e6f"));
    }

    #[test]
    fn a_stamp_is_its_time_in_utc() {
        assert_eq!(stamp(0), "197001010000");
        assert_eq!(stamp(951_782_400), "200002290000");
        assert_eq!(stamp(4_102_444_799), "209912312359");
    }
}

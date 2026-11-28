//! Installing gcloud for you, when boxes need it and it's missing:
//! Google's own archive, from dl.google.com, unpacked into Link's settings
//! folder (`~/.lnk/safe/gcp`), for you only. No administrator rights,
//! and nothing else on the machine changes: its installer runs without
//! touching your shell's settings.

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure};
use link_format::{Out, Path as Shown, Progress, Question, WithinExt, phrase, sentence};

const DOWNLOADS: &str = "https://dl.google.com/dl/cloudsdk/channels/rapid/downloads";

/// The version Link installs and each archive's SHA-256 (`install.toml`).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Pinned {
    version: String,
    sha256: std::collections::BTreeMap<String, String>,
}

/// What `install.toml` pins, read once.
fn pinned() -> &'static Pinned {
    static READ: std::sync::LazyLock<Pinned> = std::sync::LazyLock::new(|| {
        link_files::fs::from_toml(include_str!("install.toml"), Path::new("install.toml"))
            .expect("checked install.toml")
    });
    &READ
}

/// A script's lines refusing `file` unless its SHA-256 is `sum`, with
/// whichever of `sha256sum` (Linux) and `shasum` (macOS) is there.
fn verify(file: &str, sum: &str, what: &str) -> String {
    format!(
        "printf '%s  %s\\n' {sum} \"{file}\" | {{ if command -v sha256sum >/dev/null 2>&1; \
         then sha256sum -c -; else shasum -a 256 -c -; fi; }} >/dev/null \
         || {{ echo {why} >&2; exit 1; }}\n",
        sum = quote(sum),
        why = quote(&format!(
            "{what} isn't the one Link pins: its SHA-256 differs, so it's refused"
        )),
    )
}

/// Asks (on stderr) whether to install gcloud, and does: the program,
/// or None when you said no or there's no terminal to ask on.
pub fn offer(out: &mut Out) -> anyhow::Result<Option<PathBuf>> {
    if !out.can_ask() {
        return Ok(None);
    }
    let install_it = out
        .ask(Question::confirm(
            sentence!(
                "Boxes need gcloud, Google Cloud's CLI, which isn't installed. Install it for \
                 you (Google's archive, from dl.google.com)?"
            ),
            true,
        ))
        .map_err(|report| anyhow::anyhow!("{}", report.headline))?;
    if !install_it {
        return Ok(None);
    }
    install(out).map(Some)
}

pub fn install(out: &mut Out) -> anyhow::Result<PathBuf> {
    let program = crate::cli::installed("gcloud").within("no settings folder")?;
    let dir = program.ancestors().nth(3).within("no folder for gcloud")?;
    link_files::create_private_dir(dir)?;
    let script = script(std::env::consts::OS, std::env::consts::ARCH, dir)?;
    out.block(Progress::new(phrase!(
        "Installing gcloud into {}",
        Shown::new(dir)
    )));
    // The installer writes to the terminal itself.
    let _ = out.hand_over();
    // Its output on stderr: stdout is the account.
    let status = link_secretive::process::command("/bin/sh")
        .args(["-c", &script])
        .stdout(link_format::streams::child_stderr()?)
        .status()
        .within("running gcloud's installer")?;
    ensure!(status.success(), "gcloud's installer failed (above)");
    ensure!(
        program.is_file(),
        "gcloud's installer finished, but {} isn't there",
        program.display()
    );
    Ok(program)
}

/// The install, as a shell script, for `os` and `arch` (Rust's names),
/// into `dir`: `dir/google-cloud-sdk/bin/gcloud` after.
fn script(os: &str, arch: &str, dir: &Path) -> anyhow::Result<String> {
    let archive = match (os, arch) {
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-arm",
        ("macos", "x86_64") => "darwin-x86_64",
        ("macos", "aarch64") => "darwin-arm",
        _ => bail!(
            "Link can't install gcloud on {os} {arch}: \
             https://cloud.google.com/sdk/docs/install has it"
        ),
    };
    let pinned = pinned();
    let sum = pinned
        .sha256
        .get(archive)
        .within(format!("install.toml pins no gcloud for {archive}"))?;
    let dir = quote(&dir.display().to_string());
    Ok(format!(
        "set -e\n\
         tmp=$(mktemp -d)\n\
         trap 'rm -rf \"$tmp\"' EXIT\n\
         curl -fsSL --proto '=https' -o \"$tmp/gcloud.tar.gz\" {url}\n\
         {verify}\
         tar -xzf \"$tmp/gcloud.tar.gz\" -C \"$tmp\"\n\
         rm -rf {dir}/google-cloud-sdk\n\
         mv \"$tmp/google-cloud-sdk\" {dir}/google-cloud-sdk\n\
         {dir}/google-cloud-sdk/install.sh --quiet --usage-reporting false \
         --path-update false --command-completion false\n",
        url = quote(&format!(
            "{DOWNLOADS}/google-cloud-cli-{}-{archive}.tar.gz",
            pinned.version
        )),
        verify = verify("$tmp/gcloud.tar.gz", sum, "gcloud's archive"),
    ))
}

/// `text` quoted for a POSIX shell.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_installs_googles_archive_into_links_folder() {
        let dir = Path::new("/home/me/.lnk/safe/gcp");
        let linux = script("linux", "x86_64", dir).unwrap();
        let version = &pinned().version;
        assert!(linux.contains(&format!(
            "'https://dl.google.com/dl/cloudsdk/channels/rapid/downloads/google-cloud-cli-{version}-linux-x86_64.tar.gz'"
        )));
        // Checked before it's unpacked.
        let sum = &pinned().sha256["linux-x86_64"];
        let (checked, unpacked) = (
            linux.find(sum.as_str()).unwrap(),
            linux.find("tar -xzf").unwrap(),
        );
        assert!(checked < unpacked, "{linux}");
        assert!(
            linux
                .contains("mv \"$tmp/google-cloud-sdk\" '/home/me/.lnk/safe/gcp'/google-cloud-sdk")
        );
        assert!(linux.contains("--path-update false"));
        assert!(
            script("linux", "aarch64", dir)
                .unwrap()
                .contains("-linux-arm.tar.gz")
        );
        assert!(
            script("macos", "aarch64", dir)
                .unwrap()
                .contains("-darwin-arm.tar.gz")
        );
        assert!(
            script("macos", "x86_64", dir)
                .unwrap()
                .contains("-darwin-x86_64.tar.gz")
        );
        assert!(script("windows", "x86_64", dir).is_err());
    }

    /// One version, and a SHA-256 for each archive the script fetches.
    #[test]
    fn every_archive_is_pinned() {
        let pinned = pinned();
        assert!(!pinned.version.is_empty());
        assert_eq!(pinned.sha256.len(), 4);
        for sum in pinned.sha256.values() {
            assert!(
                sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit()),
                "{sum}"
            );
        }
    }

    /// An archive whose SHA-256 differs is refused before it's unpacked;
    /// the pinned one passes.
    #[test]
    fn an_archive_that_isnt_pinned_is_refused() {
        let dir = link_test_kit::TempDir::new("gcloud-verify");
        let file = dir.path().join("archive");
        std::fs::write(&file, b"not gcloud").unwrap();
        // SHA-256 of "not gcloud".
        let right = "bb0d0b75c6da59279b0fef70c1b3f2974db68cd57c9f69d99982bc1a2850e316";
        let path = file.display().to_string();
        let run = |sum: &str| {
            link_test_kit::env::command("/bin/sh", &dir)
                .args(["-c", &verify(&path, sum, "gcloud's archive")])
                .output()
                .unwrap()
        };
        assert!(run(right).status.success());
        let wrong = run(&"0".repeat(64));
        assert!(!wrong.status.success());
        assert!(String::from_utf8_lossy(&wrong.stderr).contains("isn't the one Link pins"));
    }

    #[test]
    fn the_script_runs_in_a_shell() {
        // Syntax only: `sh -n` reads it without running anything.
        let home = link_test_kit::TempDir::new("gcloud-script");
        let text = script("linux", "x86_64", &home.join("it's here")).unwrap();
        let status = link_test_kit::env::command("/bin/sh", &home)
            .args(["-n", "-c", &text])
            .status()
            .unwrap();
        assert!(status.success(), "{text}");
    }
}

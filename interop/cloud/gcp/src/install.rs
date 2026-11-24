//! Installing gcloud for you, when boxes need it and it's missing:
//! Google's own archive, from dl.google.com, unpacked into Link's settings
//! folder (`~/.lnk/safe/gcp`), for you only. No administrator rights,
//! and nothing else on the machine changes: its installer runs without
//! touching your shell's settings.

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure};
use link_format::{Out, Path as Shown, Progress, Question, WithinExt, phrase, sentence};

const DOWNLOADS: &str = "https://dl.google.com/dl/cloudsdk/channels/rapid/downloads";

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
    let dir = quote(&dir.display().to_string());
    Ok(format!(
        "set -e\n\
         tmp=$(mktemp -d)\n\
         trap 'rm -rf \"$tmp\"' EXIT\n\
         curl -fsSL --proto '=https' -o \"$tmp/gcloud.tar.gz\" {url}\n\
         tar -xzf \"$tmp/gcloud.tar.gz\" -C \"$tmp\"\n\
         rm -rf {dir}/google-cloud-sdk\n\
         mv \"$tmp/google-cloud-sdk\" {dir}/google-cloud-sdk\n\
         {dir}/google-cloud-sdk/install.sh --quiet --usage-reporting false \
         --path-update false --command-completion false\n",
        url = quote(&format!("{DOWNLOADS}/google-cloud-cli-{archive}.tar.gz")),
    ))
}

/// `text` quoted for a POSIX shell.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn it_installs_googles_archive_into_links_folder() {
        let dir = Path::new("/home/me/.lnk/safe/gcp");
        let linux = script("linux", "x86_64", dir).unwrap();
        assert!(linux.contains(
            "'https://dl.google.com/dl/cloudsdk/channels/rapid/downloads/google-cloud-cli-linux-x86_64.tar.gz'"
        ));
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

    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn the_script_runs_in_a_shell() {
        // Syntax only: `sh -n` reads it without running anything.
        let text = script("linux", "x86_64", Path::new("/tmp/it's here")).unwrap();
        let status = Command::new("/bin/sh")
            .args(["-n", "-c", &text])
            .status()
            .unwrap();
        assert!(status.success(), "{text}");
    }
}

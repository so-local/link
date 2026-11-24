//! Installing the AWS CLI for you, when boxes need it and it's missing:
//! AWS's own installer, from awscli.amazonaws.com, into Link's settings
//! folder (`~/.lnk/safe/aws`), for you only. No administrator rights,
//! and nothing else on the machine changes.

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure};
use link_format::{Out, Path as Shown, Progress, Question, Value, WithinExt, phrase, sentence};

const LINUX: &str = "https://awscli.amazonaws.com/awscli-exe-linux";
const MAC: &str = "https://awscli.amazonaws.com/AWSCLIV2.pkg";

/// Asks (on stderr) whether to install the AWS CLI, and does: the
/// program, or None when you said no or there's no terminal to ask on.
pub fn offer(out: &mut Out) -> anyhow::Result<Option<PathBuf>> {
    if !out.can_ask() {
        return Ok(None);
    }
    let from = if cfg!(target_os = "macos") {
        MAC
    } else {
        "awscli.amazonaws.com"
    };
    let install_it = out
        .ask(Question::confirm(
            sentence!(
                "Boxes need the AWS CLI, which isn't installed. Install it for you (AWS's \
                 installer, from {})?",
                Value::from(from)
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
    let program = crate::cli::installed("aws").within("no settings folder")?;
    let dir = program
        .parent()
        .and_then(Path::parent)
        .within("no folder for the AWS CLI")?;
    link_files::create_private_dir(dir)?;
    let script = script(std::env::consts::OS, std::env::consts::ARCH, dir)?;
    out.block(Progress::new(phrase!(
        "Installing the AWS CLI into {}",
        Shown::new(dir)
    )));
    // The installer writes to the terminal itself.
    let _ = out.hand_over();
    // Its output on stderr: stdout is the account.
    let status = link_secretive::process::command("/bin/sh")
        .args(["-c", &script])
        .stdout(link_format::streams::child_stderr()?)
        .status()
        .within("running the AWS CLI's installer")?;
    ensure!(status.success(), "the AWS CLI's installer failed (above)");
    ensure!(
        program.is_file(),
        "the AWS CLI's installer finished, but {} isn't there",
        program.display()
    );
    Ok(program)
}

/// The install, as a shell script, for `os` and `arch` (Rust's names),
/// into `dir`: `dir/bin/aws` after.
fn script(os: &str, arch: &str, dir: &Path) -> anyhow::Result<String> {
    let raw = dir.display().to_string();
    let dir = quote(&raw);
    let head = "set -e\n\
                tmp=$(mktemp -d)\n\
                trap 'rm -rf \"$tmp\"' EXIT\n";
    match (os, arch) {
        ("linux", "x86_64" | "aarch64") => Ok(format!(
            "{head}\
             command -v unzip >/dev/null || {{ echo 'The AWS CLI comes as a zip: install unzip first (sudo apt install unzip).' >&2; exit 1; }}\n\
             curl -fsSL --proto '=https' -o \"$tmp/awscli.zip\" {url}\n\
             unzip -q \"$tmp/awscli.zip\" -d \"$tmp\"\n\
             \"$tmp/aws/install\" --update --install-dir {dir}/aws-cli --bin-dir {dir}/bin\n",
            url = quote(&format!("{LINUX}-{arch}.zip")),
        )),
        ("macos", _) => Ok(format!(
            "{head}\
             curl -fsSL --proto '=https' -o \"$tmp/AWSCLIV2.pkg\" {url}\n\
             cat > \"$tmp/choices.xml\" <<'EOF'\n\
             <?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><array><dict>\n\
             <key>choiceAttribute</key><string>customLocation</string>\n\
             <key>attributeSetting</key><string>{xml}</string>\n\
             <key>choiceIdentifier</key><string>default</string>\n\
             </dict></array></plist>\n\
             EOF\n\
             /usr/sbin/installer -pkg \"$tmp/AWSCLIV2.pkg\" -target CurrentUserHomeDirectory -applyChoiceChangesXML \"$tmp/choices.xml\"\n\
             mkdir -p {dir}/bin\n\
             ln -sf {dir}/aws-cli/aws {dir}/bin/aws\n",
            url = quote(MAC),
            xml = xml_text(&raw)?,
        )),
        _ => bail!(
            "Link can't install the AWS CLI on {os} {arch}: https://aws.amazon.com/cli has it"
        ),
    }
}

/// `text` quoted for a POSIX shell.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// A folder for the installer's settings file: no characters XML or the
/// shell's quoting would change.
fn xml_text(text: &str) -> anyhow::Result<String> {
    ensure!(
        !text.contains(['<', '>', '&', '\'', '"']),
        "Link can't install the AWS CLI into {text:?}: install it from https://aws.amazon.com/cli"
    );
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn it_installs_into_links_folder_from_aws() {
        let dir = Path::new("/home/me/.lnk/safe/aws");
        let linux = script("linux", "x86_64", dir).unwrap();
        assert!(linux.contains("'https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip'"));
        assert!(linux.contains(
            "--install-dir '/home/me/.lnk/safe/aws'/aws-cli --bin-dir '/home/me/.lnk/safe/aws'/bin"
        ));
        let arm = script("linux", "aarch64", dir).unwrap();
        assert!(arm.contains("awscli-exe-linux-aarch64.zip"));
        let mac = script("macos", "aarch64", dir).unwrap();
        assert!(mac.contains("-target CurrentUserHomeDirectory"));
        assert!(mac.contains("<string>/home/me/.lnk/safe/aws</string>"));
        assert!(mac.contains("ln -sf '/home/me/.lnk/safe/aws'/aws-cli/aws"));
        assert!(script("windows", "x86_64", dir).is_err());
        assert!(script("macos", "aarch64", Path::new("/a<b")).is_err());
    }

    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn the_script_runs_in_a_shell() {
        // Syntax only: `sh -n` reads it without running anything.
        for (os, arch, dir) in [
            ("linux", "x86_64", "/tmp/it's here"),
            ("macos", "aarch64", "/tmp/a b"),
        ] {
            let text = script(os, arch, Path::new(dir)).unwrap();
            let status = Command::new("/bin/sh")
                .args(["-n", "-c", &text])
                .status()
                .unwrap();
            assert!(status.success(), "{os}: {text}");
        }
    }
}

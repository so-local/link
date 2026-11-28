//! Installing the AWS CLI for you, when boxes need it and it's missing:
//! AWS's own installer, from awscli.amazonaws.com, into Link's settings
//! folder (`~/.lnk/safe/aws`), for you only. No administrator rights,
//! and nothing else on the machine changes.

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure};
use link_format::{Out, Path as Shown, Progress, Question, Value, WithinExt, phrase, sentence};

const LINUX: &str = "https://awscli.amazonaws.com/awscli-exe-linux";
const MAC: &str = "https://awscli.amazonaws.com/AWSCLIV2";

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

/// Asks (on stderr) whether to install the AWS CLI, and does: the
/// program, or None when you said no or there's no terminal to ask on.
pub fn offer(out: &mut Out) -> anyhow::Result<Option<PathBuf>> {
    if !out.can_ask() {
        return Ok(None);
    }
    let from = "awscli.amazonaws.com";
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
    let pinned = pinned();
    let sum = |name: &str| {
        pinned
            .sha256
            .get(name)
            .within(format!("install.toml pins no AWS CLI for {name}"))
    };
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
             {verify}\
             unzip -q \"$tmp/awscli.zip\" -d \"$tmp\"\n\
             \"$tmp/aws/install\" --update --install-dir {dir}/aws-cli --bin-dir {dir}/bin\n",
            url = quote(&format!("{LINUX}-{arch}-{}.zip", pinned.version)),
            verify = verify(
                "$tmp/awscli.zip",
                sum(&format!("linux-{arch}"))?,
                "The AWS CLI's installer"
            ),
        )),
        ("macos", _) => Ok(format!(
            "{head}\
             curl -fsSL --proto '=https' -o \"$tmp/AWSCLIV2.pkg\" {url}\n\
             {verify}\
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
            url = quote(&format!("{MAC}-{}.pkg", pinned.version)),
            verify = verify(
                "$tmp/AWSCLIV2.pkg",
                sum("macos")?,
                "The AWS CLI's installer"
            ),
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
        let version = &pinned().version;
        assert!(linux.contains(&format!(
            "'https://awscli.amazonaws.com/awscli-exe-linux-x86_64-{version}.zip'"
        )));
        // Checked before it's unpacked, and the Mac's before it's installed.
        let sum = &pinned().sha256["linux-x86_64"];
        assert!(linux.find(sum.as_str()).unwrap() < linux.find("unzip -q").unwrap());
        assert!(linux.contains(
            "--install-dir '/home/me/.lnk/safe/aws'/aws-cli --bin-dir '/home/me/.lnk/safe/aws'/bin"
        ));
        let arm = script("linux", "aarch64", dir).unwrap();
        assert!(arm.contains(&format!("awscli-exe-linux-aarch64-{version}.zip")));
        assert!(arm.contains(pinned().sha256["linux-aarch64"].as_str()));
        let mac = script("macos", "aarch64", dir).unwrap();
        assert!(mac.contains("-target CurrentUserHomeDirectory"));
        assert!(mac.contains(&format!(
            "'https://awscli.amazonaws.com/AWSCLIV2-{version}.pkg'"
        )));
        let sum = &pinned().sha256["macos"];
        assert!(mac.find(sum.as_str()).unwrap() < mac.find("/usr/sbin/installer").unwrap());
        assert!(mac.contains("<string>/home/me/.lnk/safe/aws</string>"));
        assert!(mac.contains("ln -sf '/home/me/.lnk/safe/aws'/aws-cli/aws"));
        assert!(script("windows", "x86_64", dir).is_err());
        assert!(script("macos", "aarch64", Path::new("/a<b")).is_err());
    }

    /// One version, and a SHA-256 for each installer the script fetches.
    #[test]
    fn every_installer_is_pinned() {
        let pinned = pinned();
        assert!(!pinned.version.is_empty());
        assert_eq!(pinned.sha256.len(), 3);
        for sum in pinned.sha256.values() {
            assert!(
                sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit()),
                "{sum}"
            );
        }
    }

    /// An installer whose SHA-256 differs is refused before it runs; the
    /// pinned one passes.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn an_installer_that_isnt_pinned_is_refused() {
        let dir = link_test_kit::TempDir::new("awscli-verify");
        let file = dir.path().join("installer");
        std::fs::write(&file, b"not the aws cli").unwrap();
        // SHA-256 of "not the aws cli".
        let right = "40b3152fd6c3f3e7fa85d2e7af7e7a36040cd278c30042eab15cea0e3391e8cc";
        let path = file.display().to_string();
        let run = |sum: &str| {
            Command::new("/bin/sh")
                .args(["-c", &verify(&path, sum, "The AWS CLI's installer")])
                .output()
                .unwrap()
        };
        assert!(run(right).status.success());
        let wrong = run(&"0".repeat(64));
        assert!(!wrong.status.success());
        assert!(String::from_utf8_lossy(&wrong.stderr).contains("isn't the one Link pins"));
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

//! `install.sh` against a release on this machine: stand-ins for `curl`,
//! `ssh-keygen` and `uname` answer as theirs do, so each refusal and hint
//! is the script's own.

use std::path::{Path, PathBuf};
use std::process::Output;

use link_test_kit::TempDir;

/// The version the stand-in release says it is.
const VERSION: &str = "0.0.1";

/// A home, a release of `lnk` [`VERSION`] for Linux x86_64, and stand-ins
/// on the PATH: `curl` copies from the release (exit 22 for a file it
/// lacks, 6 for the one `CURL_UNREACHABLE` names, and a SIGINT to the
/// script for the one `CURL_INTERRUPT` names), logging each file asked.
struct Installing {
    home: TempDir,
    release: PathBuf,
    bin: PathBuf,
}

impl Installing {
    fn new(name: &str) -> Installing {
        let home = TempDir::new(name);
        let release = home.join("release");
        let bin = home.join("stand-ins");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let program = home.join("program");
        std::fs::create_dir_all(&program).unwrap();
        link_test_kit::write_program(
            &program.join("lnk"),
            format!("#!/bin/sh\necho 'lnk {VERSION}'\n"),
        )
        .unwrap();
        // Linux's, and a Mac's for the line a Mac's bash is given.
        let assets = [
            "lnk-core-x86_64-unknown-linux-musl.tar.gz",
            "lnk-core-x86_64-apple-darwin.tar.gz",
        ];
        for asset in assets {
            let packed = link_test_kit::env::command("tar", &home)
                .arg("-czf")
                .arg(release.join(asset))
                .arg("-C")
                .arg(&program)
                .arg("lnk")
                .status()
                .unwrap();
            assert!(packed.success());
        }
        std::fs::write(release.join("VERSION"), format!("{VERSION}\n")).unwrap();
        let sums: String = ["VERSION", assets[0], assets[1]]
            .iter()
            .map(|file| {
                let bytes = std::fs::read(release.join(file)).unwrap();
                format!("{}  {file}\n", link_digest::sha256(&bytes))
            })
            .collect();
        std::fs::write(release.join("SHA256SUMS"), sums).unwrap();
        std::fs::write(release.join("SHA256SUMS.sig"), "signed\n").unwrap();
        link_test_kit::write_program(
            &bin.join("curl"),
            r#"#!/bin/sh
for a; do case "$a" in http*) url=$a ;; esac; done
prev=''
for a; do [ "$prev" = -o ] && out=$a; prev=$a; done
name=${url##*/}
echo "$name" >> "$CURL_LOG"
if [ "$name" = "${CURL_UNREACHABLE:-}" ]; then echo "curl: (6) Could not resolve host: releases" >&2; exit 6; fi
if [ "$name" = "${CURL_INTERRUPT:-}" ]; then kill -INT "$PPID"; fi
[ -f "$RELEASE/$name" ] || { echo "curl: (22) The requested URL returned error: 404" >&2; exit 22; }
cp "$RELEASE/$name" "$out"
"#,
        )
        .unwrap();
        link_test_kit::write_program(&bin.join("ssh-keygen"), "#!/bin/sh\nexit 0\n").unwrap();
        link_test_kit::write_program(
            &bin.join("uname"),
            "#!/bin/sh\ncase \"$1\" in -s) echo \"${UNAME_S:-Linux}\" ;; -m) echo x86_64 ;; esac\n",
        )
        .unwrap();
        Installing { home, release, bin }
    }

    /// The default folder `lnk` goes in.
    fn dir(&self) -> PathBuf {
        self.home.join(".lnk/exec/bin")
    }

    /// The files `curl` was asked for, in order.
    fn asked(&self) -> String {
        std::fs::read_to_string(self.home.join("curl.log")).unwrap_or_default()
    }

    /// `install.sh` run with `envs`, its stand-ins first on the PATH.
    fn run(&self, envs: &[(&str, &str)]) -> Output {
        let path =
            std::env::join_paths([self.bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")])
                .unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
        let mut command = link_test_kit::env::command("sh", &self.home);
        command
            .arg(script)
            .env("PATH", path)
            .env("SHELL", "/bin/zsh")
            .env("LNK_RELEASES_URL", "https://releases.invalid")
            .env("LNK_VERSION", VERSION)
            .env("RELEASE", &self.release)
            .env("CURL_LOG", self.home.join("curl.log"))
            .env_remove("LNK_INSTALL_DIR");
        command.envs(envs.iter().copied());
        command.output().unwrap()
    }
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// It installs `lnk` beside where it ran it, leaves nothing of its own
/// there, and gives each shell its own PATH line.
#[test]
fn it_installs_and_gives_each_shell_its_own_path_line() {
    let installing = Installing::new("install-script-shells");
    let fish = installing.run(&[("SHELL", "/usr/bin/fish")]);
    assert!(fish.status.success(), "{}", said(&fish));
    let dir = installing.dir();
    assert!(dir.join("lnk").is_file());
    let left: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(left, ["lnk"], "only lnk is left in its folder");
    assert!(said(&fish).contains("fish_add_path"), "{}", said(&fish));
    assert!(said(&fish).contains("Next: lnk up"), "{}", said(&fish));
    let mac_bash = installing.run(&[("SHELL", "/bin/bash"), ("UNAME_S", "Darwin")]);
    assert!(
        said(&mac_bash).contains("~/.bash_profile"),
        "{}",
        said(&mac_bash)
    );
    let linux_bash = installing.run(&[("SHELL", "/bin/bash")]);
    assert!(
        said(&linux_bash).contains("~/.bashrc"),
        "{}",
        said(&linux_bash)
    );
    let other = installing.run(&[("SHELL", "/bin/dash")]);
    assert!(said(&other).contains("~/.profile"), "{}", said(&other));
}

/// Installed over an older install, it says its modules are another
/// release's, and that `lnk upgrade` brings them up.
#[test]
fn over_an_older_install_it_names_the_upgrade() {
    let installing = Installing::new("install-script-older");
    let dir = installing.dir();
    std::fs::create_dir_all(&dir).unwrap();
    link_test_kit::write_program(
        &dir.join("lnk-example"),
        "#!/bin/sh\necho 'lnk-example 0.0.0'\n",
    )
    .unwrap();
    let output = installing.run(&[]);
    assert!(output.status.success(), "{}", said(&output));
    let text = said(&output);
    assert!(text.contains("another release's: lnk-example"), "{text}");
    assert!(text.contains("Next: lnk upgrade"), "{text}");
}

/// A folder it can't write is refused before anything is downloaded.
#[test]
fn a_folder_it_cant_write_is_refused_before_downloading() {
    let installing = Installing::new("install-script-unwritable");
    let file = installing.home.join("a-file");
    std::fs::write(&file, "").unwrap();
    let inside = file.join("bin");
    let output = installing.run(&[("LNK_INSTALL_DIR", inside.to_str().unwrap())]);
    assert!(!output.status.success());
    assert!(
        said(&output).contains("can't write to"),
        "{}",
        said(&output)
    );
    assert_eq!(installing.asked(), "", "nothing was downloaded");
}

/// A download that fails says which, and whether the release lacks it
/// or the network didn't answer: a signature the release lacks is "not
/// signed", one the network lost isn't.
#[test]
fn a_failed_download_says_which_and_why() {
    let installing = Installing::new("install-script-downloads");
    let unreachable = installing.run(&[("CURL_UNREACHABLE", "SHA256SUMS.sig")]);
    assert!(!unreachable.status.success());
    let text = said(&unreachable);
    assert!(text.contains("couldn't download SHA256SUMS.sig"), "{text}");
    assert!(!text.contains("not signed"), "{text}");
    std::fs::remove_file(installing.release.join("SHA256SUMS.sig")).unwrap();
    let unsigned = installing.run(&[]);
    assert!(
        said(&unsigned).contains("not signed"),
        "{}",
        said(&unsigned)
    );
    std::fs::remove_file(installing.release.join("SHA256SUMS")).unwrap();
    let missing = installing.run(&[]);
    assert!(
        said(&missing).contains("has no SHA256SUMS"),
        "{}",
        said(&missing)
    );
}

/// Interrupted, it stops, removing what it downloaded and installing
/// nothing.
#[test]
fn interrupting_it_stops_it() {
    let installing = Installing::new("install-script-interrupt");
    let output = installing.run(&[("CURL_INTERRUPT", "VERSION")]);
    assert_eq!(output.status.code(), Some(130), "{}", said(&output));
    assert!(
        !installing.asked().contains("lnk-core"),
        "{}",
        installing.asked()
    );
    let dir = installing.dir();
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
}

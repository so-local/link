//! `signal-cli`: harnesses that speak Signal do it through it (its
//! daemon, on a unix socket or a port of the agent's, or `jsonRpc` over
//! its stdin and stdout, as each adapter runs it), linked to the agent's
//! number as a device, as Signal Desktop is, and so does `lnk-signal bridge` when
//! Link carries Signal for a harness that speaks none.
//!
//! `lnk-signal cli install --home <dir> --json` installs it into a
//! harness's home ([`install_in`]), which `lnk-agent` runs in that
//! harness's sandbox before `configure`: a pinned release, checked by
//! SHA-256 as the storage module pins rclone; the native build on Linux
//! x86-64, else the Java build with a pinned Java runtime. It runs through
//! a wrapper ([`program_in`]) that keeps the account in the home
//! (`~/.signal-cli/data`, the harness's state, which a move carries) and
//! passes Link's proxy (`HTTPS_PROXY`) on as Java's proxy settings, which
//! is all `signal-cli` reads. `lnk-signal cli link --home <dir>` links it
//! from a QR code on the terminal ([`link_in`]).

use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, bail};
use link_format::{
    Done, Exit, Foreign, Name, Out, Report, Text, WithinExt, cmd, phrase, prose, sentence,
};

struct Pinned {
    url: &'static str,
    sha256: &'static str,
}
include!(concat!(env!("OUT_DIR"), "/installer-pins.rs"));

/// `.signal-cli` in `home`.
fn dir_in(home: &Path) -> PathBuf {
    home.join(".signal-cli")
}

/// The wrapper in `home`, which runs with `HOME` set to `home`.
pub fn program_in(home: &Path) -> PathBuf {
    dir_in(home).join("signal-cli")
}

/// Its account: the agent's number, its keys. The harness's state, as
/// `~/`-relative for [`link_harness_contract::harness::State`].
pub const DATA: &str = "~/.signal-cli/data";

/// The agent's number, which [`install`] writes and [`link`] links, as
/// `~/`-relative for [`link_harness_contract::harness::State`].
pub const ACCOUNT: &str = "~/.signal-cli/account";

/// The account's folder in `home`, which `--config` names.
pub fn data_in(home: &Path) -> PathBuf {
    dir_in(home).join("data")
}

/// The wrapper: `signal-cli` with its account in the home, through the
/// proxy when there is one. Paths start from `$HOME`, which every step of
/// a harness has as its home, so it works as well after a move.
fn wrapper(native: bool, java_home: &str) -> String {
    let run = if native {
        format!(r#"exec "$HOME/.signal-cli/{VERSION}/signal-cli" $proxy"#)
    } else {
        format!(
            r#"export JAVA_HOME="$HOME/.signal-cli/java/{java_home}"
export JAVA_OPTS="$proxy"
exec "$HOME/.signal-cli/{VERSION}/signal-cli-{VERSION}/bin/signal-cli""#
        )
    };
    format!(
        r#"#!/bin/sh
# signal-cli {VERSION}, written by Link: its account in ~/.signal-cli/data,
# through the proxy in HTTPS_PROXY when there is one.
proxy=""
if [ -n "$HTTPS_PROXY" ]; then
  address=${{HTTPS_PROXY#*://}}
  address=${{address%%/*}}
  address=${{address##*@}}
  proxy="-Dhttps.proxyHost=${{address%:*}} -Dhttps.proxyPort=${{address##*:}}"
fi
{run} --config "$HOME/.signal-cli/data" "$@"
"#
    )
}

/// Installs `signal-cli` into `home`, if it isn't, for the agent's number
/// `account` (which [`link`] checks), and returns the wrapper to run it
/// with.
pub fn install_in(home: &Path, account: &str) -> anyhow::Result<PathBuf> {
    let dir = || dir_in(home);
    let native = cfg!(all(target_os = "linux", target_arch = "x86_64"));
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let java_home = if os == "macos" {
        JAVA_HOME_MAC
    } else {
        JAVA_HOME_LINUX
    };
    let release = dir().join(VERSION);
    if !release.is_dir() {
        let java = match native {
            true => None,
            false => Some(
                JAVA.iter()
                    .find(|(o, a, _)| *o == os && *a == arch)
                    .map(|(_, _, pinned)| pinned)
                    .with_context(|| format!("Link has no signal-cli for {os} on {arch}"))?,
            ),
        };
        link_connection_contract::note(&format!(
            "Installing signal-cli {VERSION} (first time only)"
        ));
        let staging = dir().join(format!("{VERSION}.{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let unpacked = unpack(if native { &NATIVE } else { &JAVA_BUILD }, &staging);
        let unpacked = unpacked.and_then(|()| match java {
            Some(java) => {
                let _ = std::fs::remove_dir_all(dir().join("java"));
                std::fs::create_dir_all(dir().join("java"))?;
                unpack(java, &dir().join("java"))
            }
            None => Ok(()),
        });
        if let Err(err) = unpacked {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(err);
        }
        std::fs::rename(&staging, &release)?;
    }
    link_files::write_program(&program_in(home), wrapper(native, java_home))?;
    std::fs::create_dir_all(data_in(home))?;
    std::fs::write(dir_in(home).join("account"), account)?;
    Ok(program_in(home))
}

/// Downloads `pinned` and unpacks it into `into`, if its SHA-256 is the
/// pinned one.
fn unpack(pinned: &Pinned, into: &std::path::Path) -> anyhow::Result<()> {
    let archive = into.join("download.tar.gz");
    let bytes = link_runtime::block_on(link_artifact::Cache::shared()?.bytes(
        pinned.url,
        pinned.sha256,
        200_000_000,
    ))?;
    std::fs::write(&archive, bytes)?;
    let unpacked = link_harness_contract::harness::run(
        link_secretive::process::command("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(into),
    );
    let _ = std::fs::remove_file(&archive);
    unpacked
}

/// The number `signal-cli` in `home` is linked to, if it is.
pub fn linked_in(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(data_in(home).join("data/accounts.json")).ok()?;
    let accounts: serde_json::Value = serde_json::from_str(&text).ok()?;
    accounts["accounts"]
        .as_array()?
        .iter()
        .find_map(|a| a["number"].as_str().map(String::from))
}

/// Links `signal-cli` in `home` to the agent's number (as [`install_in`]
/// was given it), as a device: its link as a QR code on the terminal, for
/// Signal on the phone (Settings > Linked Devices). Anew with `again`.
/// Whether it's linked: false when it isn't and there's no terminal to
/// show the code on.
pub fn link(home: &Path, again: bool, out: &mut Out) -> anyhow::Result<bool> {
    let expected = std::fs::read_to_string(dir_in(home).join("account"))
        .context("signal-cli isn't set up here: run lnk agent connect signal")?;
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdin());
    link_in(home, expected.trim(), again, terminal, out, || {
        Ok(link_secretive::process::command(program_in(home)))
    })
}

/// [`link`] to number `expected`, showing the code when
/// `terminal`; `signal_cli` is the command to run it with (its program
/// and the options before `link`), asked for only when it links.
pub fn link_in(
    home: &Path,
    expected: &str,
    again: bool,
    terminal: bool,
    out: &mut Out,
    signal_cli: impl FnOnce() -> anyhow::Result<Command>,
) -> anyhow::Result<bool> {
    let data = data_in(home);
    match linked_in(home) {
        Some(number) if !again && number == expected => return Ok(true),
        Some(_) => {
            std::fs::remove_dir_all(&data)?;
            std::fs::create_dir_all(&data)?;
        }
        None => {}
    }
    if !terminal {
        return Ok(false);
    }
    let mut child = signal_cli()?
        .args(["link", "-n", "Link"])
        .stdout(Stdio::piped())
        .spawn()
        .context("running signal-cli link")?;
    let stdout = child.stdout.take().context("no output from signal-cli")?;
    let mut number = None;
    for line in BufReader::new(stdout).lines() {
        let line = line?;
        if line.starts_with("sgnl://") {
            out.now(Text(prose!(
                "On the phone with the agent's Signal ({}), open Settings > Linked Devices > \
                 Link New Device, and scan this code.",
                Name::new("number", expected)
            )));
            // The code itself, as it is: a program draws it, not Link.
            out.data(format!("{}\n", link_connection_contract::qr(&line)?).as_bytes());
        } else if let Some(n) = line.strip_prefix("Associated with: ") {
            number = Some(n.trim().to_string());
        }
    }
    let status = child.wait()?;
    let Some(number) = number.filter(|_| status.success()) else {
        bail!("Signal didn't link ({status}); lnk agent connect signal shows a new code");
    };
    if number != expected {
        let _ = std::fs::remove_dir_all(&data);
        bail!(
            "that phone's Signal is {number}, not {expected}: scan the code from the agent's \
             phone, or connect Signal again with its number"
        );
    }
    Ok(true)
}

/// `lnk-signal cli <args>`: `install --home <dir> --json`, the connection's
/// settings (`{"account", ...}`) on stdin, answers `{"program", "state"}`;
/// `state --json`, `{"state"}` alone ([`STATE`]); `link
/// --home <dir> [--again]`, exits 0 once linked and
/// [`link_harness_contract::harness::NOT_LINKED`] when it isn't and there's no
/// terminal.
pub fn main(given: Native) -> std::process::ExitCode {
    link_settings::output::run(|out| run(out, given))
}

/// Which of the connection protocol's `cli` commands `lnk-signal` was run
/// with (`actions.toml`), with what each was given: its home, absolute
/// as its declaration checks.
pub enum Native {
    /// `cli install --home <dir>`.
    Install { home: PathBuf },
    /// `cli link --home <dir> [--again]`.
    Link { home: PathBuf, again: bool },
    /// `cli state`.
    State,
}

/// What of a harness's home is the agent's Signal account, `~/`-relative,
/// which a move carries with the harness's own state: its account's
/// folder. The rest (the wrapper, Java, `signal-cli`) `install` makes
/// again.
pub const STATE: &[&str] = &[DATA];

/// A line of `cli` that can't be used.
fn usage(headline: link_format::Sentence, why: link_format::Prose) -> Report {
    Report::error(headline).why(why).exit(Exit::Usage).next(
        cmd!("lnk agent connect signal"),
        phrase!("connect Signal, which sets its program up"),
    )
}

/// `answer` for a program as its declared output, the contract's
/// `cli`: `--json`'s reply, and otherwise the same JSON as it is.
fn document(out: &mut Out, answer: &serde_json::Value) -> Result<(), Report> {
    link_connection_contract::document::<link_connection_contract::declared::Cli>(out, answer)
}

fn run(out: &mut Out, given: Native) -> Result<Done, Report> {
    let failed = |headline| move |err| link_connection_contract::failed("signal", headline, err);
    match given {
        Native::State => {
            document(out, &serde_json::json!({"state": STATE}))?;
            Ok(Done::shown())
        }
        Native::Install { home } => {
            let mut input = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
                .within("reading Signal's settings on stdin")?;
            let settings: link_harness_contract::harness::Signal =
                serde_json::from_str(&input).within("reading Signal's settings on stdin")?;
            if !link_connection_contract::is_number(&settings.account) {
                return Err(usage(
                    sentence!(
                        "The account {} isn't a phone number.",
                        Foreign::new(&settings.account)
                    ),
                    prose!("It is a number with its country code."),
                ));
            }
            let program = install_in(&home, &settings.account)
                .map_err(failed(sentence!("Signal's program wasn't installed.")))?;
            document(
                out,
                &serde_json::json!({"program": program, "state": STATE}),
            )?;
            Ok(Done::shown())
        }
        Native::Link { home, again } => {
            match link(&home, again, out).map_err(failed(sentence!("Signal wasn't linked.")))? {
                true => Ok(Done::shown()),
                // The harness's own code, which it reads.
                false => Err(Report::error(sentence!(
                    "Signal isn't linked, and there's no terminal to link it on."
                ))
                .exit(Exit::Refused)
                .next(
                    cmd!("lnk agent connect signal"),
                    phrase!("connect it on a terminal"),
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_fixed_releases() {
        for pinned in [&NATIVE, &JAVA_BUILD]
            .into_iter()
            .chain(JAVA.iter().map(|(_, _, p)| p))
        {
            assert!(
                pinned.url.starts_with("https://github.com/"),
                "{}",
                pinned.url
            );
            assert!(!pinned.url.contains("latest"), "{}", pinned.url);
            assert_eq!(pinned.sha256.len(), 64);
        }
        assert!(NATIVE.url.contains(&format!("/v{VERSION}/")));
        assert!(JAVA_BUILD.url.contains(&format!("/v{VERSION}/")));
    }

    #[cfg(unix)]
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn the_wrapper_passes_the_proxy_to_java() {
        let dir = link_test_kit::TempDir::new("signal");
        let bin = dir.join(format!(".signal-cli/{VERSION}"));
        std::fs::create_dir_all(&bin).unwrap();
        // A signal-cli that says what it got.
        link_test_kit::write_program(&bin.join("signal-cli"), "#!/bin/sh\necho \"$@\"\n").unwrap();
        let wrapper = dir.join("wrapper");
        link_test_kit::write_program(&wrapper, super::wrapper(true, "")).unwrap();
        let run = |proxy: Option<&str>| {
            let mut command = Command::new(&wrapper);
            command.env("HOME", &dir).env_remove("HTTPS_PROXY");
            if let Some(proxy) = proxy {
                command.env("HTTPS_PROXY", proxy);
            }
            String::from_utf8(command.arg("--version").output().unwrap().stdout).unwrap()
        };
        let data = dir.join(".signal-cli/data").display().to_string();
        assert_eq!(
            run(Some("http://127.0.0.1:40781")),
            format!(
                "-Dhttps.proxyHost=127.0.0.1 -Dhttps.proxyPort=40781 --config {data} --version\n"
            )
        );
        // A proxy on this machine's loopback names its run's secret, which
        // Java can't send: the host and port alone, the secret never on
        // its command line.
        assert_eq!(
            run(Some("http://lnk:0123456789abcdef@127.0.0.1:50123")),
            format!(
                "-Dhttps.proxyHost=127.0.0.1 -Dhttps.proxyPort=50123 --config {data} --version\n"
            )
        );
        assert_eq!(run(None), format!("--config {data} --version\n"));
    }

    #[test]
    fn linking_requires_an_installed_account() {
        let (mut out, _) = Out::capture(false, &[]);
        let dir = link_test_kit::TempDir::new("signal-not-installed");
        let err = run(
            &mut out,
            Native::Link {
                home: dir.to_path_buf(),
                again: false,
            },
        )
        .unwrap_err();
        assert!(
            err.why
                .as_ref()
                .unwrap()
                .to_string()
                .contains("isn't set up")
        );
    }

    #[test]
    fn a_link_is_a_qr_code() {
        let code = link_connection_contract::qr("sgnl://linkdevice?uuid=abc&pub_key=def").unwrap();
        assert!(code.lines().count() > 10);
        assert!(code.contains('█') || code.contains('▀') || code.contains('▄'));
    }
}

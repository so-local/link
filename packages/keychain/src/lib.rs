//! Secrets in the macOS Keychain, through Apple's own `security` tool,
//! rather than in Link's settings files.
//!
//! Items are generic passwords of the service [`SERVICE`], one account
//! per secret (`relay-token`, `agent/main/model-key`, ...). They are made
//! with `security`, so its access list trusts `security` (Apple's, the same
//! across `lnk` upgrades) and reading one never asks: Link's own programs
//! aren't signed by Apple, so an item made by them would ask again after
//! every upgrade. Secrets reach `security` on its stdin (`security -i`,
//! the value hex-encoded), never on a command line.
//!
//! The store is chosen once per platform, never per secret: the Keychain
//! on a Mac, and elsewhere (Linux, boxes), where there is none
//! ([`available`] is false), the settings files (mode 600). A Keychain
//! that refuses a secret, or can't be read, is an error: Link never falls
//! back to a file on a Mac, and never reads a secret a file holds there.
//! Callers keep the secret in memory as before and only change where it is
//! saved: [`stash`] when saving, [`fetch`] when loading.

use std::{
    io::{Read, Write},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use anyhow::{bail, ensure};
use link_format::WithinExt;

use link_secretive::Secret;

/// The Keychain service every Link item belongs to.
pub const SERVICE: &str = "link";

/// `security`'s exit status when no item matches.
const NOT_FOUND: i32 = 44;

mod constants;
use constants::ANSWERS_WITHIN;

/// The `security` Link runs: Apple's, or `LNK_SECURITY`'s stand-in in a
/// test build alone, so no environment a release starts in can send your
/// secrets to another program.
fn program() -> std::ffi::OsString {
    link_secretive::test_var("LNK_SECURITY").unwrap_or_else(|| "/usr/bin/security".into())
}

/// Whether secrets go to the Keychain: on macOS, always. `LNK_SECURITY`
/// (another `security`, read by a test build alone) turns it on anywhere.
pub fn available() -> bool {
    cfg!(target_os = "macos") || link_secretive::test_var("LNK_SECURITY").is_some()
}

/// An account name `security` takes as one word: letters, digits and
/// `._:/@-`.
fn check_account(account: &str) -> anyhow::Result<()> {
    ensure!(
        !account.is_empty()
            && account.len() <= 200
            && account
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:/@-".contains(&b)),
        "not a Keychain account name: {account:?}"
    );
    Ok(())
}

/// The secret saved as `account`, if there is one.
///
/// `security -w` prints a secret holding any unprintable byte, such as
/// the newlines of an SSH private key, as bare hex, and one without as
/// itself: the two can't be told apart. `-g` marks which it is, so the
/// secret is read from the line it writes, `password: ` and then either
/// `0x` and the bytes in hex, or the text in quotes.
pub fn get(account: &str) -> anyhow::Result<Option<Secret>> {
    check_account(account)?;
    let mut command = link_secretive::process::command(program());
    command.args(["find-generic-password", "-s", SERVICE, "-a", account, "-g"]);
    let output = answered(command, None, ANSWERS_WITHIN)?;
    if output.status.code() == Some(NOT_FOUND) {
        return Ok(None);
    }
    if !output.status.success() {
        bail!(
            "the Keychain didn't give {account} ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stderr = Secret::new(
        String::from_utf8(output.stderr)
            .map_err(|_| anyhow::anyhow!("security's answer isn't text"))?,
    );
    password(stderr.expose())
        .map(Some)
        .within(format!("reading {account} from security's answer"))
}

/// The secret on `security -g`'s `password: ` line: `0x` and its bytes
/// in hex (then, ignored, its readable characters), or the text in
/// quotes, or nothing for an empty secret.
fn password(stderr: &str) -> anyhow::Result<Secret> {
    let line = stderr
        .lines()
        .find_map(|line| {
            line.strip_prefix("password: ")
                .or((line == "password:").then_some(""))
        })
        .within("security gave no password line")?;
    if line.is_empty() {
        return Ok(Secret::new(String::new()));
    }
    if let Some(quoted) = line.strip_prefix('"') {
        let text = quoted
            .strip_suffix('"')
            .within("security's password isn't quoted")?;
        return Ok(Secret::new(text.to_string()));
    }
    let hex = line
        .strip_prefix("0x")
        .and_then(|rest| rest.split(' ').next())
        .within("security's password is neither quoted nor hex")?;
    ensure!(
        hex.len() % 2 == 0 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "security's password isn't hex"
    );
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()?;
    let text =
        String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("a Keychain item isn't text"))?;
    Ok(Secret::new(text))
}

/// Saves `secret` as `account`, replacing what was there, and reads it
/// back: an error means it isn't in the Keychain.
pub fn set(account: &str, secret: &Secret) -> anyhow::Result<()> {
    check_account(account)?;
    has_default_keychain(account)?;
    let hex = secret.expose_map(|s| s.bytes().map(|b| format!("{b:02x}")).collect::<String>());
    let line = hex.expose_map(|hex| {
        format!(
            "add-generic-password -U -s {SERVICE} -a {account} -l {SERVICE}:{account} -X {hex}\n"
        )
    });
    let mut command = link_secretive::process::command(program());
    command.arg("-i");
    let output = answered(command, Some(line.expose().as_bytes()), ANSWERS_WITHIN)?;
    ensure!(
        output.status.success(),
        "the Keychain didn't take {account}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    // `security -i` doesn't always fail when a command does: check.
    ensure!(
        get(account)?.as_ref() == Some(secret),
        "the Keychain didn't keep {account}"
    );
    Ok(())
}

/// Refuses a save the Keychain would answer with a dialog: with no
/// default keychain (a `HOME` that isn't the user's, a user who never
/// logged in), `security` asks a person where to keep the item and waits.
fn has_default_keychain(account: &str) -> anyhow::Result<()> {
    let mut command = link_secretive::process::command(program());
    command.arg("default-keychain");
    let output = answered(command, None, ANSWERS_WITHIN)?;
    ensure!(
        output.status.success(),
        "the Keychain can't keep {account}: this user has no default keychain ({}), \
         so saving would wait on a dialog; run it as a user who logged in to this Mac, \
         with their own HOME",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

/// `security`'s answer to `command`, `input` on its stdin, within `limit`.
/// `security` answers at once unless it is showing a person a dialog (a
/// locked keychain, a keychain to choose): past the limit it is ended and
/// the call fails, as a Keychain that asks for a person fails the command.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a child Link started until it exits or its limit passes; no test waits here"
)]
fn answered(mut command: Command, input: Option<&[u8]>, limit: Duration) -> anyhow::Result<Output> {
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .within("running security")?;
    if let Some(input) = input {
        let mut stdin = child.stdin.take().within("security's stdin")?;
        let written = stdin.write_all(input);
        drop(stdin);
        if let Err(error) = written {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error).within("writing to security");
        }
    }
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut out);
            }
            out
        })
    };
    let stdout = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "the Keychain asked for a person (security gave no answer in {}s): \
                 unlock it, or run as a user whose Keychain answers, and run again",
                limit.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Ok(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Removes `account`, if it's there.
pub fn delete(account: &str) -> anyhow::Result<()> {
    check_account(account)?;
    let mut command = link_secretive::process::command(program());
    command.args(["delete-generic-password", "-s", SERVICE, "-a", account]);
    let status = answered(command, None, ANSWERS_WITHIN)?.status;
    ensure!(
        status.success() || status.code() == Some(NOT_FOUND),
        "the Keychain didn't remove {account} ({status})"
    );
    Ok(())
}

/// Every item of Link's, by account (`lnk uninstall`, to remove some and
/// keep the rest); none where there is no Keychain. `security
/// dump-keychain` without `-d` lists the items' names, never their secrets.
pub fn accounts() -> anyhow::Result<Vec<String>> {
    if !available() {
        return Ok(Vec::new());
    }
    let mut command = link_secretive::process::command(program());
    command.arg("dump-keychain");
    let output = answered(command, None, ANSWERS_WITHIN)?;
    ensure!(
        output.status.success(),
        "security couldn't list the Keychain ({})",
        output.status
    );
    Ok(link_accounts(&String::from_utf8_lossy(&output.stdout)))
}

/// The accounts of the service [`SERVICE`] in `security dump-keychain`'s
/// output: one `keychain:` line per item, then its attributes, among them
/// `"acct"<blob>="..."` and `"svce"<blob>="..."`.
fn link_accounts(dump: &str) -> Vec<String> {
    let value = |line: &str, key: &str| {
        line.trim()
            .strip_prefix(&format!("\"{key}\"<blob>=\""))
            .and_then(|rest| rest.strip_suffix('"'))
            .map(String::from)
    };
    let mut found = Vec::new();
    let mut item: (Option<String>, Option<String>) = (None, None);
    for line in dump.lines().chain(["keychain: end"]) {
        if line.starts_with("keychain:") {
            if let (Some(account), Some(service)) = item.clone()
                && service == SERVICE
                && check_account(&account).is_ok()
                && !found.contains(&account)
            {
                found.push(account);
            }
            item = (None, None);
        } else if let Some(account) = value(line, "acct") {
            item.0 = Some(account);
        } else if let Some(service) = value(line, "svce") {
            item.1 = Some(service);
        }
    }
    found
}

/// Removes every item of Link's (`lnk uninstall`), where there is a
/// Keychain.
pub fn delete_all() -> anyhow::Result<()> {
    if !available() {
        return Ok(());
    }
    // One item per call; bounded, in case one won't go.
    for _ in 0..1000 {
        let mut command = link_secretive::process::command(program());
        command.args(["delete-generic-password", "-s", SERVICE]);
        let status = answered(command, None, ANSWERS_WITHIN)?.status;
        if status.code() == Some(NOT_FOUND) {
            return Ok(());
        }
        ensure!(status.success(), "security failed ({status})");
    }
    bail!("more than 1000 items")
}

/// Where a secret goes when saving: into the Keychain as `account` when
/// there is one, and then `None` for the file; else the secret itself, for
/// the file. `None` removes `account`. A Keychain that refuses is an
/// error, never a secret for the file.
pub fn stash(account: &str, secret: Option<&Secret>) -> anyhow::Result<Option<Secret>> {
    if !available() {
        return Ok(secret.map(Secret::duplicate));
    }
    match secret {
        Some(secret) => set(account, secret)?,
        None => delete(account)?,
    }
    Ok(None)
}

/// A secret when loading: the Keychain's `account` where there is one,
/// else `in_file`, the one the settings file holds. On a Mac a secret in
/// the file is refused, never read: Link keeps none there.
pub fn fetch(account: &str, in_file: Option<Secret>) -> anyhow::Result<Option<Secret>> {
    if !available() {
        return Ok(in_file);
    }
    ensure!(
        in_file.is_none(),
        "a settings file holds {account}, which Link keeps only in the Keychain: \
         remove it from the file and set it again"
    );
    get(account).within(format!(
        "reading {SERVICE}:{account} from the Keychain: unlock it, and run again"
    ))
}

/// A secret kept on its own: in the Keychain as `account` where there is
/// one, else in the file `path`, readable by this user alone (mode 600).
pub struct Kept {
    pub account: String,
    pub path: std::path::PathBuf,
}

impl Kept {
    /// What's kept; none when nothing is. Where there is a Keychain only
    /// it is read, and a file at `path` is refused. A Keychain that can't
    /// be read (locked, or asking for a person) is an error, never none,
    /// so nothing is made over what's there.
    pub fn load(&self) -> anyhow::Result<Option<Secret>> {
        if !available() {
            return match self.path.exists() {
                true => link_secretive::read_secret(&self.path).map(Some),
                false => Ok(None),
            };
        }
        ensure!(
            !self.path.exists(),
            "{} holds {}, which Link keeps only in the Keychain: remove the file and run again",
            self.path.display(),
            self.account
        );
        fetch(&self.account, None)
    }

    /// `secret` kept: in the Keychain where there is one, else in the
    /// file. A Keychain that refuses it is an error.
    pub fn keep(&self, secret: &Secret) -> anyhow::Result<()> {
        if let Some(secret) = stash(&self.account, Some(secret))? {
            link_files::write_private(&self.path, secret.expose().as_bytes())
                .within(format!("keeping it in {}", self.path.display()))?;
        }
        Ok(())
    }

    /// Where it's kept, as a list of what Link keeps says it.
    pub fn shown(&self) -> String {
        shown(&self.account, &self.path)
    }
}

/// The display location of an account's credential, never its value.
pub fn shown(account: &str, file: &std::path::Path) -> String {
    if available() {
        format!("the Keychain ({}:{account})", SERVICE)
    } else {
        format!("{} (only yours)", file.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A release build runs Apple's `security` whatever its environment
    /// names: `LNK_SECURITY` is a test build's alone, so a variable set
    /// where `lnk` starts can't send your secrets to another program.
    /// Proved by `cargo test --release -p link-keychain`.
    #[cfg(not(debug_assertions))]
    #[test]
    fn a_release_ignores_a_stand_in() {
        // SAFETY: this test alone reads or writes the variable.
        unsafe { std::env::set_var("LNK_SECURITY", "/nonexistent/security") };
        assert_eq!(program(), "/usr/bin/security");
        assert_eq!(available(), cfg!(target_os = "macos"));
        unsafe { std::env::remove_var("LNK_SECURITY") };
    }

    #[test]
    fn account_names() {
        assert!(check_account("agent/main/model-key").is_ok());
        assert!(check_account("relay-token").is_ok());
        assert!(check_account("a b").is_err());
        assert!(check_account("a;rm").is_err());
        assert!(check_account("").is_err());
    }

    /// `security -g`'s three forms: quoted text, hex (its readable
    /// characters after, ignored), and nothing.
    #[test]
    fn passwords_from_security() {
        let read = |stderr: &str| password(stderr).unwrap().expose().clone();
        assert_eq!(read("password: \"0a0b 'x\" \"y\"\n"), "0a0b 'x\" \"y");
        assert_eq!(read("password: 0x610A5C62  \"a\\012\\134b\"\n"), "a\n\\b");
        assert_eq!(read("password: 0x0A \n"), "\n");
        assert_eq!(read("password: \n"), "");
        assert!(password("nothing\n").is_err());
        assert!(password("password: 0xZZ\n").is_err());
    }

    #[test]
    fn accounts_from_a_dump() {
        let dump = r#"keychain: "/Users/me/Library/Keychains/login.keychain-db"
version: 512
class: "genp"
attributes:
    0x00000007 <blob>="link:agent/main/model-key"
    "acct"<blob>="agent/main/model-key"
    "svce"<blob>="link"
keychain: "/Users/me/Library/Keychains/login.keychain-db"
class: "genp"
attributes:
    "acct"<blob>="me@example.com"
    "svce"<blob>="someone-else"
keychain: "/Users/me/Library/Keychains/login.keychain-db"
class: "genp"
attributes:
    "svce"<blob>="link"
    "acct"<blob>="relay-token"
"#;
        assert_eq!(link_accounts(dump), ["agent/main/model-key", "relay-token"]);
        assert!(link_accounts("").is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn round_trip_through_a_fake_security() {
        let dir = link_test_kit::TempDir::new("keychain");
        let script = link_test_kit::keychain::stand_in(&dir);
        // SAFETY: this test alone, in this crate's test program, sets it or
        // reads a store through it. A crate whose other tests read the
        // store never sets it in its own process: it gives it to a child.
        unsafe { std::env::set_var("LNK_SECURITY", &script) };
        assert!(available());
        let plain = |s: Option<Secret>| s.map(|s| s.expose().clone());
        assert_eq!(plain(get("relay-token").unwrap()), None);
        set(
            "relay-token",
            &"s3cret with spaces & 'quotes'".to_string().into(),
        )
        .unwrap();
        assert_eq!(
            plain(get("relay-token").unwrap()).as_deref(),
            Some("s3cret with spaces & 'quotes'")
        );
        // An SSH private key's newlines, which `security -w` would give as hex.
        let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3Bl\n-----END OPENSSH PRIVATE KEY-----";
        set("box/ssh-ca", &pem.to_string().into()).unwrap();
        assert_eq!(plain(get("box/ssh-ca").unwrap()).as_deref(), Some(pem));
        delete("box/ssh-ca").unwrap();
        set("agent/main/model-key", &"k".to_string().into()).unwrap();
        let mut accounts = accounts().unwrap();
        accounts.sort();
        assert_eq!(accounts, ["agent/main/model-key", "relay-token"]);
        delete("agent/main/model-key").unwrap();
        assert_eq!(
            plain(stash("relay-token", Some(&"t2".to_string().into())).unwrap()),
            None
        );
        assert_eq!(
            plain(fetch("relay-token", None).unwrap()).as_deref(),
            Some("t2")
        );
        // A secret in a file, where the Keychain keeps it, is refused.
        assert!(fetch("relay-token", Some("old".to_string().into())).is_err());
        let kept = Kept {
            account: "relay-token".into(),
            path: dir.join("relay-token"),
        };
        assert_eq!(plain(kept.load().unwrap()).as_deref(), Some("t2"));
        std::fs::write(&kept.path, "old").unwrap();
        assert!(kept.load().is_err());
        std::fs::remove_file(&kept.path).unwrap();
        kept.keep(&"t3".to_string().into()).unwrap();
        assert!(!kept.path.exists());
        assert_eq!(plain(kept.load().unwrap()).as_deref(), Some("t3"));
        assert_eq!(plain(stash("relay-token", None).unwrap()), None);
        assert_eq!(plain(get("relay-token").unwrap()), None);
        assert_eq!(plain(kept.load().unwrap()), None);
        // A Keychain that refuses is an error, never a file.
        let refusing = dir.join("refusing");
        link_test_kit::programs::write_program(&refusing, "#!/bin/sh\nexit 1\n").unwrap();
        unsafe { std::env::set_var("LNK_SECURITY", &refusing) };
        assert!(stash("relay-token", Some(&"t4".to_string().into())).is_err());
        assert!(fetch("relay-token", None).is_err());
        assert!(kept.keep(&"t4".to_string().into()).is_err());
        assert!(!kept.path.exists());
        // One that can't be read is an error, never none.
        assert!(kept.load().is_err());
        // A user with no default keychain, whose save would wait on a
        // dialog, fails before anything is saved.
        let homeless = dir.join("homeless");
        link_test_kit::programs::write_program(
            &homeless,
            "#!/bin/sh\n[ \"$1\" = default-keychain ] && { echo 'A default keychain could not be found.' >&2; exit 1; }\ntouch \"$0.saved\"\n",
        )
        .unwrap();
        unsafe { std::env::set_var("LNK_SECURITY", &homeless) };
        let refused = set("relay-token", &"t5".to_string().into()).unwrap_err();
        assert!(
            format!("{refused:#}").contains("no default keychain"),
            "{refused:#}"
        );
        assert!(!dir.join("homeless.saved").exists());
        unsafe { std::env::remove_var("LNK_SECURITY") };
    }

    /// A `security` showing a dialog never answers; the call fails at its
    /// limit, saying it asked for a person, rather than waiting on it.
    #[test]
    #[cfg(unix)]
    fn a_keychain_asking_for_a_person_fails() {
        let dir = link_test_kit::TempDir::new("keychain-asks");
        let asking = dir.join("asking");
        link_test_kit::programs::write_program(&asking, "#!/bin/sh\nexec sleep 600\n").unwrap();
        let command = link_test_kit::env::command(&asking, &dir);
        let error = answered(
            command,
            Some(b"add-generic-password\n"),
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("asked for a person"),
            "{error:#}"
        );
    }
}

#[cfg(test)]
link_test_kit::program!(system);

//! A box's users (`lnk box user`): people with a Linux user of their own
//! on a box, each let in by SSH into a sandbox of their own. What the box
//! module writes and every other side reads, from this one module: where
//! root keeps them, the line that lets a key in, and the one way into a
//! user's sandbox, for an SSH session and a run alike.
//!
//! Root keeps each user in `/etc/lnk/users/<name>/`, where neither they
//! nor anything they run writes: `user.toml` (their folder and key),
//! `<name>.toml` (their sandbox's spec, its paths absolute) and
//! `authorized_keys`, which sshd reads for them (`AuthorizedKeysFile`,
//! for the group [`GROUP`]) in place of their own, which their sandbox
//! could change. Its one line leaves sshd nothing to do but enter the
//! sandbox:
//!
//! ```text
//! restrict,pty,command="/usr/local/lib/lnk/lnk-sandbox enter --spec /etc/lnk/users/alice/alice.toml --folder /home/alice" ssh-ed25519 AAAA... lnk-user-alice
//! ```
//!
//! `restrict` turns off port, agent and X11 forwarding and `~/.ssh/rc`,
//! each a way around the sandbox; `pty` gives back a terminal. Their login
//! shell is `/bin/sh`, which reads no file of theirs before the command.
//! The programs are root's copies in `/usr/local/lib/lnk`, since the box's
//! own user's home is closed to other users.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};

use serde::{Deserialize, Serialize};

/// Who a run runs as: `root`, outside any sandbox, or a user of the box
/// by name, in their own sandbox (a box's own user, by name, in the
/// sandbox the run names): core's shared shape (`link-types`).
pub use link_types::User;

/// The group every box user is in, which sshd's drop-in names.
pub const GROUP: &str = "lnk-users";

/// A folder [`Places::here`] puts in place of `/`, owned by whoever runs
/// the test rather than root, for tests of a stand-in box. Read by a
/// debug build only ([`link_secretive::test_var`]).
pub const TEST_ROOT: &str = "LNK_TEST_BOX_ROOT";

/// Where a box keeps its users, and the programs that let them in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    /// The machine's `/`: a test's folder in a test.
    pub root: PathBuf,
    /// Who must own what's kept: root, and in a test the test's user.
    pub owners: Vec<u32>,
}

impl Places {
    /// This machine's: `/`, or [`TEST_ROOT`]'s folder in a test.
    pub fn here() -> Places {
        match link_secretive::test_var(TEST_ROOT) {
            Some(root) => Places {
                root: root.into(),
                // SAFETY: geteuid has no failure and touches no memory.
                owners: vec![0, unsafe { libc::geteuid() }],
            },
            None => Places {
                root: "/".into(),
                owners: vec![0],
            },
        }
    }

    fn at(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    fn system(&self, path: &Path) -> PathBuf {
        self.root
            .join(path.strip_prefix("/").expect("a system path is absolute"))
    }

    /// Every user's folder: `/etc/lnk/users`.
    pub fn users(&self) -> PathBuf {
        self.system(&link_files::layout::System::BoxUsers.path())
    }

    /// User `name`'s folder of root's.
    pub fn user(&self, name: &str) -> PathBuf {
        self.users().join(name)
    }

    /// User `name`'s sandbox spec.
    pub fn spec(&self, name: &str) -> PathBuf {
        self.user(name).join(format!("{name}.toml"))
    }

    /// User `name`'s keys, which sshd reads for them.
    pub fn keys(&self, name: &str) -> PathBuf {
        self.user(name).join("authorized_keys")
    }

    /// Root's copies of `lnk` and `lnk-sandbox`.
    pub fn programs(&self) -> PathBuf {
        self.system(&link_files::layout::SystemRoot::Exec.path())
    }

    /// The `lnk-sandbox` every user's session runs.
    pub fn sandbox(&self) -> PathBuf {
        self.system(&link_files::layout::System::SandboxProgram.path())
    }

    /// The home folder a new user `name` gets.
    pub fn home(&self, name: &str) -> PathBuf {
        self.at("home").join(name)
    }

    /// sshd's drop-in for [`GROUP`].
    pub fn sshd(&self) -> PathBuf {
        self.at("etc/ssh/sshd_config.d/60-lnk-users.conf")
    }

    /// The drop-in's text: a box user's keys are root's file, never their
    /// own, and nothing of sshd's leads around their sandbox.
    pub fn sshd_text(&self) -> String {
        format!(
            "# Written by lnk box user: a box user's keys are root's, each a way into their \
             sandbox.\nMatch Group {GROUP}\n    AuthorizedKeysFile {}/%u/authorized_keys\n    \
             DisableForwarding yes\n    PermitUserRC no\n    PermitUserEnvironment no\n",
            self.users().display()
        )
    }
}

/// One user of a box, as root keeps them (`user.toml`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxUser {
    /// Their folder: their home, the one place they write.
    pub folder: PathBuf,
    /// Their key's type and data (`ssh-ed25519 AAAA...`), without its
    /// comment.
    pub key: String,
}

link_files::specification!(BoxUser, include_str!("box_user.schema.toml"));

/// An error unless `name` can name a box's user: a Linux user's name
/// ([`User`]), not root's.
pub fn check_name(name: &str) -> anyhow::Result<()> {
    match User::try_from(name.to_string()) {
        Ok(user) if user.is_root() => bail!("root is the box's own, not a user lnk box user adds"),
        Ok(_) => Ok(()),
        Err(why) => bail!("{why}"),
    }
}

/// User `name`, as root keeps them; None if the box has no such user.
pub fn load(places: &Places, name: &str) -> anyhow::Result<Option<BoxUser>> {
    check_name(name)?;
    let trust = link_files::fs::Trust {
        owners: &places.owners,
        deny_mode: 0o022,
        ancestors: true,
        no_link: true,
    };
    let path = places.user(name).join("user.toml");
    match link_files::fs::read_trusted(&path, trust, 64 * 1024, "a box user's") {
        Ok(read) => Ok(Some(
            link_files::saved::parse(&read.text)
                .with_context(|| format!("reading {}", path.display()))?,
        )),
        Err(err) if link_files::fs::missing(&err) => Ok(None),
        Err(err) => Err(err.context(format!(
            "{} lets a key into this box: it's root's alone",
            path.display()
        ))),
    }
}

/// Every user's name, sorted.
pub fn names(places: &Places) -> anyhow::Result<Vec<String>> {
    let mut names = Vec::new();
    let entries = match std::fs::read_dir(places.users()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(names),
        Err(err) => return Err(err).context("reading the box's users"),
    };
    for entry in entries {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if check_name(&name).is_ok() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// The command a session of user `name` (whose folder is `folder`) runs,
/// as its words: their sandbox, entered. A run as them adds `--` and its
/// command.
pub fn entry(places: &Places, name: &str, folder: &Path) -> Vec<String> {
    [
        places.sandbox().display().to_string(),
        "enter".into(),
        "--spec".into(),
        places.spec(name).display().to_string(),
        "--folder".into(),
        folder.display().to_string(),
    ]
    .to_vec()
}

/// What marks user `name`'s line as Link's.
fn marker(name: &str) -> String {
    format!("lnk-user-{name}")
}

/// The `authorized_keys` line for user `name`: `key`, let in only to
/// their sandbox.
pub fn line(places: &Places, name: &str, user: &BoxUser) -> anyhow::Result<String> {
    let words = entry(places, name, &user.folder);
    for word in &words[..] {
        ensure!(
            Path::new(&words[0]).is_absolute() && !word.contains(['"', '\\', '\n', ' ']),
            "{word} can't go in authorized_keys"
        );
    }
    Ok(format!(
        "restrict,pty,command=\"{}\" {} {}",
        words.join(" "),
        user.key,
        marker(name)
    ))
}

/// A public key's type and data from `text` (a key line, or the path of a
/// `.pub` file), its comment and any options dropped.
pub fn public_key(text: &str) -> anyhow::Result<String> {
    let text = match Path::new(text.trim()).is_file() {
        true => std::fs::read_to_string(text.trim()).context("reading the key file")?,
        false => text.to_string(),
    };
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let (Some(line), None) = (lines.next(), lines.next()) else {
        bail!("give one public key (ssh-ed25519 AAAA..., or a .pub file)");
    };
    let mut words = line.split_whitespace();
    let (Some(kind), Some(data)) = (words.next(), words.next()) else {
        bail!("that isn't a public key (ssh-ed25519 AAAA...)");
    };
    let known = kind.starts_with("ssh-")
        || kind.starts_with("ecdsa-sha2-")
        || kind.starts_with("sk-ssh-")
        || kind.starts_with("sk-ecdsa-");
    let base64 = data
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b));
    ensure!(
        known && base64 && data.len() >= 16 && kind.bytes().all(|b| b.is_ascii_graphic()),
        "that isn't a public key (ssh-ed25519 AAAA...); a private key is never needed"
    );
    Ok(format!("{kind} {data}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBgm7VJ2Pqg8qXU0j2c5y3fF7g2nq";

    #[test]
    fn names_and_keys_are_plain() {
        check_name("alice").unwrap();
        check_name("bob-2").unwrap();
        for bad in [
            "",
            "root",
            "Alice",
            "a b",
            "-a",
            "a/b",
            "a\"b",
            &"x".repeat(33),
        ] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
        assert_eq!(public_key(&format!("{KEY} alice@laptop\n")).unwrap(), KEY);
        for bad in [
            "",
            "hello",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "ssh-ed25519 AAAA\"x",
            "ssh-ed25519 short",
            &format!("{KEY}\n{KEY}"),
        ] {
            assert!(public_key(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_users_line_can_only_enter_their_sandbox() {
        let places = Places {
            root: "/".into(),
            owners: vec![0],
        };
        let user = BoxUser {
            folder: "/home/alice".into(),
            key: KEY.into(),
        };
        assert_eq!(
            line(&places, "alice", &user).unwrap(),
            format!(
                "restrict,pty,command=\"/usr/local/lib/lnk/lnk-sandbox enter --spec \
                 /etc/lnk/users/alice/alice.toml --folder /home/alice\" {KEY} lnk-user-alice"
            )
        );
        let spaced = Places {
            root: "/a b".into(),
            owners: vec![0],
        };
        assert!(line(&spaced, "alice", &user).is_err());
        assert!(
            places
                .sshd_text()
                .contains("AuthorizedKeysFile /etc/lnk/users/%u/authorized_keys")
        );
    }
}

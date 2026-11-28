//! A box's users on a stand-in box (`lnk box user`): a folder standing
//! for the box's `/` (`LNK_TEST_BOX_ROOT`), the box's own commands
//! (`useradd`, `systemctl`) stood in for, and an sshd of the test's own
//! reading each user's keys from where root keeps them. Each user SSHes in
//! with their key and lands in their own sandbox. Skipped where sshd,
//! ssh or bubblewrap can't run, or as another user than root, whose
//! files a box keeps them in.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use link_sandbox_contract::box_users::{self, Places};
use link_test_kit::TempDir;

use link_test_kit::programs;

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn keygen(path: &Path) {
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

struct StandIn {
    dir: TempDir,
    /// The box's `/`.
    root: PathBuf,
    bin: PathBuf,
    port: u16,
    sshd: Option<Child>,
    _held: link_test_kit::HeldNumber,
}

impl Drop for StandIn {
    fn drop(&mut self) {
        if let Some(mut sshd) = self.sshd.take() {
            let _ = sshd.kill();
            let _ = sshd.wait();
        }
    }
}

impl StandIn {
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn new(users: &[&str]) -> Option<Self> {
        use link_test_kit::gate;
        let sshd = gate::sshd()?;
        let target = Path::new(env!("CARGO_TARGET_TMPDIR"));
        // Outside /tmp, which no spec reads.
        if !link_test_kit::out_of_tmp(target) {
            return None;
        }
        let dir = TempDir::under(target, "box-users");
        let (root, bin) = (dir.join("box"), dir.join("bin"));
        for folder in [&root, &bin, &dir.join("home")] {
            std::fs::create_dir_all(folder).unwrap();
        }
        std::fs::write(
            dir.join("kind.toml"),
            "kind = \"box\"\nos = \"ubuntu-24.04\"\narch = \"x86_64\"\n",
        )
        .unwrap();
        // The box's own commands: what they were asked, and a home made.
        let log = dir.join("host.log");
        for program in ["groupadd", "userdel", "systemctl", "chown", "pkill"] {
            link_test_kit::write_program(
                &bin.join(program),
                format!("#!/bin/sh\necho \"{program} $*\" >> '{}'\n", log.display()),
            )
            .unwrap();
        }
        link_test_kit::write_program(
            &bin.join("useradd"),
            format!(
                "#!/bin/sh\necho \"useradd $*\" >> '{}'\n\
                 while [ $# -gt 0 ]; do [ \"$1\" = --home-dir ] && mkdir -p \"$2\"; shift; done\n",
                log.display()
            ),
        )
        .unwrap();
        keygen(&dir.join("hostkey"));
        for user in users {
            keygen(&dir.join(user));
        }
        // Its port, which sshd binds itself: held while the stand-in is.
        let held = link_test_kit::HeldNumber::port();
        let port = held.port_number();
        let places = Places {
            root: root.clone(),
            owners: vec![0],
        };
        // sshd reads each user's keys from root's file for them, as the
        // drop-in has it for the group; here every user is this one, whose
        // session gets the fixture's home, never the account's own.
        let keys: Vec<String> = users
            .iter()
            .map(|u| places.keys(u).display().to_string())
            .collect();
        let config = dir.join("sshd_config");
        std::fs::write(
            &config,
            format!(
                "ListenAddress 127.0.0.1\nPort {port}\nHostKey {}\nAuthorizedKeysFile {}\n\
                 StrictModes no\nUsePAM no\nPasswordAuthentication no\nPidFile none\n\
                 SetEnv HOME={}\n",
                dir.join("hostkey").display(),
                keys.join(" "),
                dir.join("home").display(),
            ),
        )
        .unwrap();
        let _ = std::fs::create_dir_all("/run/sshd");
        let child = Command::new(sshd)
            .args(["-D", "-e", "-f"])
            .arg(&config)
            .stderr(Stdio::null())
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(err) => {
                gate::lacks("an sshd that starts", err, Some(gate::SANDBOX));
                return None;
            }
        };
        let mut stand_in = Self {
            dir,
            root,
            bin,
            port,
            sshd: Some(child),
            _held: held,
        };
        let start = Instant::now();
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            if start.elapsed() > Duration::from_secs(5) {
                gate::lacks(
                    "an sshd that starts",
                    "it listened on no port in 5s",
                    Some(gate::SANDBOX),
                );
                return None;
            }
            if let Some(Ok(Some(status))) = stand_in.sshd.as_mut().map(|c| c.try_wait()) {
                gate::lacks("an sshd that starts", status, Some(gate::SANDBOX));
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(stand_in)
    }

    fn places(&self) -> Places {
        Places {
            root: self.root.clone(),
            owners: vec![0],
        }
    }

    /// `lnk box user <args>`, run on the box as root.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn user(&self, args: &[&str]) -> Output {
        Command::new(programs().join("lnk-box"))
            .arg("user")
            .args(args)
            .env_clear()
            .env("HOME", self.dir.join("home"))
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.dir.join("home")),
            )
            .env(
                "PATH",
                format!("{}:/usr/local/bin:/usr/bin:/bin", self.bin.display()),
            )
            .env("LNK_EXE", programs().join("lnk"))
            .env(box_users::TEST_ROOT, &self.root)
            .env("LNK_TEST_MACHINE_KIND", self.dir.join("kind.toml"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `command` over SSH with `user`'s key.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn ssh(&self, user: &str, command: &str) -> Output {
        Command::new("ssh")
            .arg("-i")
            .arg(self.dir.join(user))
            .args([
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "BatchMode=yes",
                "-o",
                "LogLevel=ERROR",
                "-p",
                &self.port.to_string(),
            ])
            .arg(format!("{}@127.0.0.1", link_test_kit::users::me()))
            .arg(command)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

/// EN12's: two users with different sandboxes, each SSH session confined
/// to its own; a run as one in theirs; one removed, their key opening
/// nothing.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn each_user_of_a_box_lands_in_their_own_sandbox() {
    // A box's users are root's files.
    if !link_test_kit::gate::root() {
        return;
    }
    if !crate::landlock() {
        return;
    }
    let (ann, ben) = ("lnk-ann", "lnk-ben");
    let Some(b) = StandIn::new(&[ann, ben]) else {
        return;
    };
    let shared = b.root.join("srv");
    for user in [ann, ben] {
        std::fs::create_dir_all(shared.join(user)).unwrap();
        std::fs::write(
            shared.join(user).join("notes.txt"),
            format!("NOTES-OF-{user}"),
        )
        .unwrap();
        let spec = b.dir.join(format!("{user}.toml"));
        std::fs::write(
            &spec,
            format!(
                "schema = 1\n[files]\nread = [\"{}\"]\n[commands]\ntoolchains = [\"shell\"]\n",
                shared.join(user).display()
            ),
        )
        .unwrap();
        let key = b.dir.join(format!("{user}.pub"));
        // Asked first: with no terminal, only --yes adds them.
        let asked = b.user(&[
            "add",
            user,
            "--public-key",
            &key.display().to_string(),
            "--sandbox",
            &spec.display().to_string(),
        ]);
        assert!(
            !asked.status.success() && text(&asked).contains("--yes"),
            "{}",
            text(&asked)
        );
        assert!(!b.places().keys(user).exists(), "added unasked");
        let added = b.user(&[
            "add",
            user,
            "--public-key",
            &key.display().to_string(),
            "--sandbox",
            &spec.display().to_string(),
            "--yes",
        ]);
        assert!(added.status.success(), "{}", text(&added));
    }
    let host = std::fs::read_to_string(b.dir.join("host.log")).unwrap();
    assert!(
        host.contains(&format!(
            "--shell /bin/sh --user-group --groups lnk-users {ann}"
        )),
        "{host}"
    );
    // Each folder theirs alone, whatever useradd's default.
    use std::os::unix::fs::PermissionsExt;
    for user in [ann, ben] {
        let mode = std::fs::metadata(b.places().home(user))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "{user}: {mode:o}");
    }
    assert!(host.contains("systemctl reload ssh"), "{host}");
    let drop_in = std::fs::read_to_string(b.places().sshd()).unwrap();
    assert!(drop_in.contains("Match Group lnk-users"), "{drop_in}");
    // Their programs are root's copies, which every user reaches.
    assert!(b.places().sandbox().is_file());
    let listed = b.user(&["list", "--json"]);
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed[0]["name"], ann);
    assert_eq!(listed[1]["name"], ben);

    // Each session: their own folder, their own sandbox's reads, never
    // the other's.
    for (user, other) in [(ann, ben), (ben, ann)] {
        let folder = b.places().home(user);
        let ran = b.ssh(
            user,
            &format!(
                "echo made > made.txt; pwd; cat {mine}/notes.txt; cat {theirs}/notes.txt \
                 2>/dev/null; echo x > {other_home}/escaped 2>/dev/null; \
                 echo x > {root}/escaped 2>/dev/null; exit 7",
                mine = shared.join(user).display(),
                theirs = shared.join(other).display(),
                other_home = b.places().home(other).display(),
                root = b.root.display(),
            ),
        );
        let out = text(&ran);
        assert_eq!(ran.status.code(), Some(7), "{user}: {out}");
        assert!(out.contains(&folder.display().to_string()), "{out}");
        assert!(out.contains(&format!("NOTES-OF-{user}")), "{user}: {out}");
        assert!(
            !out.contains(&format!("NOTES-OF-{other}")),
            "{user} read {other}'s: {out}"
        );
        assert_eq!(
            std::fs::read_to_string(folder.join("made.txt")).unwrap(),
            "made\n"
        );
        assert!(!b.places().home(other).join("escaped").exists());
        assert!(!b.root.join("escaped").exists());
    }
    // No way around the sandbox through a forwarded port.
    let forwarded = Command::new("ssh")
        .arg("-i")
        .arg(b.dir.join(ann))
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-L",
            "0:127.0.0.1:22",
            "-p",
            &b.port.to_string(),
        ])
        .arg(format!("{}@127.0.0.1", link_test_kit::users::me()))
        .arg("true")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!forwarded.status.success(), "a user forwarded a port");

    // Removed: their key opens nothing; the other's still does.
    let removed = b.user(&["remove", ben]);
    assert!(removed.status.success(), "{}", text(&removed));
    assert!(!b.ssh(ben, "true").status.success());
    assert!(b.ssh(ann, "true").status.success());
    assert!(
        b.places().home(ben).join("made.txt").exists(),
        "the folder stays"
    );
    let host = std::fs::read_to_string(b.dir.join("host.log")).unwrap();
    assert!(host.contains(&format!("userdel {ben}")), "{host}");
    // Their sessions ended first, so userdel goes ahead.
    let ended = host.find(&format!("pkill -KILL -u {ben}"));
    let deleted = host.find(&format!("userdel {ben}"));
    assert!(ended.zip(deleted).is_some_and(|(e, d)| e < d), "{host}");
    // Kept as root's alone: a user given ben's ID later owns none of it.
    let kept = b.places().home(ben);
    assert!(
        host.contains(&format!("chown -R --no-dereference 0:0 {}", kept.display())),
        "{host}"
    );
    let mode = std::fs::metadata(&kept).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700, "{mode:o}");
    // A remove cut short after their key went: removing them again
    // takes the missing key as removed.
    std::fs::remove_file(b.places().keys(ann)).unwrap();
    let again = b.user(&["remove", ann]);
    assert!(again.status.success(), "{}", text(&again));
}

/// Only on a box, and only once per name: a computer has no users of
/// `lnk box user`'s, and a Linux user made otherwise isn't taken over.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_box_user_is_a_boxs_and_never_one_made_otherwise() {
    let dir = TempDir::new("box-users-refused");
    let run = |args: &[&str], kind: Option<&str>| {
        let mut command = Command::new(programs().join("lnk-box"));
        command
            .arg("user")
            .args(args)
            .env_clear()
            .env("HOME", &dir)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&dir))
            .env("PATH", "/usr/bin:/bin")
            .env(box_users::TEST_ROOT, dir.join("box"))
            .stdin(Stdio::null());
        if let Some(kind) = kind {
            std::fs::write(dir.join("kind.toml"), kind).unwrap();
            command.env("LNK_TEST_MACHINE_KIND", dir.join("kind.toml"));
        }
        text(&command.output().unwrap())
    };
    let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBgm7VJ2Pqg8qXU0j2c5y3fF7g2nq";
    let out = run(&["list"], None);
    assert!(out.contains("this machine is a computer"), "{out}");
    let a_box = Some("kind = \"box\"\nos = \"ubuntu-24.04\"\narch = \"x86_64\"\n");
    let out = run(&["list"], a_box);
    assert!(out.contains("No users"), "{out}");
    let out = run(
        &[
            "add",
            "nobody",
            "--public-key",
            key,
            "--sandbox",
            "x.toml",
            "--yes",
        ],
        a_box,
    );
    assert!(out.contains("not a user lnk box user add made"), "{out}");
    let out = run(
        &[
            "add",
            "root",
            "--public-key",
            key,
            "--sandbox",
            "x.toml",
            "--yes",
        ],
        a_box,
    );
    assert!(out.contains("root is the box's own"), "{out}");
}

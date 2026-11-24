//! `lnk sandbox enter` for real, on Linux: an sshd of the test's own, on a
//! random port, and a key let in with `enter` as its forced command, which
//! lands in the sandbox its spec makes. Skipped where sshd, ssh or
//! bubblewrap can't run.

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use link_test_kit::TempDir;

struct World {
    root: TempDir,
    home: PathBuf,
    lnk: PathBuf,
    client: PathBuf,
    port: u16,
    sshd: Option<Child>,
    _held: link_test_kit::HeldNumber,
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(mut sshd) = self.sshd.take() {
            let _ = sshd.kill();
            let _ = sshd.wait();
        }
    }
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

impl World {
    /// An sshd accepting the test home's `authorized_keys` for this user,
    /// in `root`, or None where it can't run here, said by its gate.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn at(root: TempDir) -> Option<Self> {
        use link_test_kit::gate;
        let sshd = gate::sshd()?;
        if !gate::bubblewrap() {
            return None;
        }
        let (home, bin) = (root.join("home"), root.join("bin"));
        std::fs::create_dir_all(home.join(".lnk/safe")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(home.join("secret.txt"), "SECRET").unwrap();
        std::fs::write(home.join(".lnk/safe/token"), "TOKEN").unwrap();
        // The core, as sshd runs it for a key let in: HOME is the test's, and
        // `lnk sandbox` runs its program without its namespace word.
        let lnk = bin.join("lnk");
        link_test_kit::write_program(
            &lnk,
            format!(
                "#!/bin/sh\nexport HOME='{}'\n[ \"$1\" = sandbox ] && shift && exec '{}' \"$@\"\nexit 1\n",
                home.display(),
                env!("CARGO_BIN_EXE_lnk-sandbox")
            ),
        )
        .unwrap();
        let client = root.join("client");
        keygen(&client);
        keygen(&root.join("hostkey"));
        // Its port, which sshd binds itself: held while the world is.
        let held = crate::proxy::free_port();
        let port = held.number() as u16;
        let sftp = [
            "/usr/lib/openssh/sftp-server",
            "/usr/libexec/openssh/sftp-server",
        ]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .unwrap_or("internal-sftp");
        let config = root.join("sshd_config");
        std::fs::write(
            &config,
            format!(
                "ListenAddress 127.0.0.1\nPort {port}\nHostKey {}\nAuthorizedKeysFile {}\n\
                 StrictModes no\nUsePAM no\nPasswordAuthentication no\nPidFile none\n\
                 Subsystem sftp {sftp}\n",
                root.join("hostkey").display(),
                home.join(".ssh/authorized_keys").display(),
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
        let mut world = Self {
            root,
            home,
            lnk,
            client,
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
            if let Some(Ok(Some(status))) = world.sshd.as_mut().map(|c| c.try_wait()) {
                gate::lacks("an sshd that starts", status, Some(gate::SANDBOX));
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(world)
    }

    fn lnk(&self, args: &[&str]) -> std::process::Output {
        self.lnk_in(args, &[])
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn lnk_in(&self, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.home),
            )
            .env("PATH", "/usr/bin:/bin")
            .env("LNK_EXE", &self.lnk)
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ssh_options(&self) -> Vec<String> {
        [
            "-i",
            &self.client.display().to_string(),
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "BatchMode=yes",
            "-o",
            "LogLevel=ERROR",
        ]
        .map(String::from)
        .to_vec()
    }

    /// This process's user, by its uid: `USER` can name another, or none.
    fn user(&self) -> String {
        // SAFETY: getpwuid's answer is read at once, on this thread.
        unsafe {
            let entry = libc::getpwuid(libc::getuid());
            if entry.is_null() {
                return "root".into();
            }
            std::ffi::CStr::from_ptr((*entry).pw_name)
                .to_string_lossy()
                .into_owned()
        }
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn ssh(&self, extra: &[&str], command: &str) -> std::process::Output {
        Command::new("ssh")
            .args(self.ssh_options())
            .args(["-p", &self.port.to_string()])
            .args(extra)
            .arg(format!("{}@127.0.0.1", self.user()))
            .arg(command)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_key_entering_by_spec_lands_in_that_specs_sandbox_and_folder() {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR"));
    // Outside /tmp, which no spec reads.
    if !link_test_kit::out_of_tmp(target) {
        return;
    }
    let Some(world) = World::at(TempDir::under(target, "enter-ssh")) else {
        return;
    };
    let shared = world.home.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("notes.txt"), "NOTES").unwrap();
    // The spec and the key's line are written by whoever lets it in (a
    // box's root): here, the test.
    let spec = world.root.join("specs/dana.toml");
    std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
    std::fs::write(
        &spec,
        "schema = 1\n[files]\nread = [\"~/shared\"]\n[commands]\ntoolchains = [\"shell\"]\n",
    )
    .unwrap();
    let folder = world.home.join("dana");
    let key = std::fs::read_to_string(world.client.with_extension("pub")).unwrap();
    std::fs::create_dir_all(world.home.join(".ssh")).unwrap();
    std::fs::write(
        world.home.join(".ssh/authorized_keys"),
        format!(
            "restrict,pty,command=\"{} sandbox enter --spec {} --folder {}\" {key}",
            world.lnk.display(),
            spec.display(),
            folder.display()
        ),
    )
    .unwrap();

    let ran = world.ssh(
        &[],
        "echo made > made.txt; pwd; cat ../shared/notes.txt; cat ../secret.txt 2>/dev/null; \
         cat ../.lnk/safe/token 2>/dev/null; echo x > ../escaped 2>/dev/null; exit 7",
    );
    let out = text(&ran);
    assert_eq!(ran.status.code(), Some(7), "{out}");
    assert!(out.contains(&folder.display().to_string()), "{out}");
    assert!(out.contains("NOTES"), "the spec's read: {out}");
    for secret in ["SECRET", "TOKEN"] {
        assert!(!out.contains(secret), "{secret} leaked: {out}");
    }
    assert_eq!(
        std::fs::read_to_string(folder.join("made.txt")).unwrap(),
        "made\n"
    );
    assert!(!world.home.join("escaped").exists());

    // A run's command, after `--`, in place of the session's.
    let ran = world.lnk_in(
        &[
            "enter",
            "--spec",
            &spec.display().to_string(),
            "--folder",
            &folder.display().to_string(),
            "--",
            "pwd; cat ../shared/notes.txt; cat ../secret.txt 2>/dev/null; exit 3",
        ],
        &[],
    );
    let out = text(&ran);
    assert_eq!(ran.status.code(), Some(3), "{out}");
    assert!(out.contains("NOTES") && !out.contains("SECRET"), "{out}");

    // Neither a spec without its folder, nor nothing.
    for refused in [&["enter", "--spec", "x.toml"][..], &["enter"]] {
        assert!(!world.lnk(refused).status.success(), "{refused:?}");
    }
}

/// `--check` says what the sandbox opens and runs nothing; a spec whose
/// sandbox can't hold a session, or that writes where it leads out, is
/// refused as any is.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_spec_entered_is_checked_and_said() {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR"));
    if !link_test_kit::out_of_tmp(target) {
        return;
    }
    let root = TempDir::under(target, "enter-check");
    let home = root.join("home");
    std::fs::create_dir_all(home.join("shared")).unwrap();
    let check_at = |spec: &str, folder: &Path| {
        let file = root.join("carol.toml");
        std::fs::write(&file, format!("schema = 1\n{spec}")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
            .args(["enter", "--check", "--spec"])
            .arg(&file)
            .arg("--folder")
            .arg(folder)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (output.status.success(), text(&output))
    };
    let check = |spec: &str| check_at(spec, &home.join("carol"));
    // Their session is bash: a spec that runs nothing can't be theirs.
    let (ok, out) = check("[files]\nread = [\"~/shared\"]\n");
    assert!(!ok && out.contains("/bin/bash"), "{out}");
    // A write that leads out is refused as for any spec.
    let (ok, out) = check("[files]\nwrite = [\"~/.ssh\"]\n[commands]\ntoolchains = [\"shell\"]\n");
    assert!(!ok && out.contains("dot folder"), "{out}");
    let (ok, out) = check("[files]\nread = [\"~/shared\"]\n[commands]\ntoolchains = [\"shell\"]\n");
    assert!(ok, "{out}");
    assert!(
        out.contains(&home.join("shared").display().to_string()),
        "{out}"
    );
    // Their folder is what the session writes, said as such.
    let folder = home.join("carol").display().to_string();
    assert!(
        out.lines()
            .any(|l| l.starts_with("writes") && l.contains(&folder)),
        "{out}"
    );
    assert!(!home.join("carol").exists(), "--check made their folder");
    // Their folder through a link a non-root user owns: refused, naming
    // where it leads, never written there.
    let linked = home.join("dave");
    std::os::unix::fs::symlink(home.join("shared"), &linked).unwrap();
    let owner =
        || std::os::unix::fs::MetadataExt::uid(&std::fs::symlink_metadata(&linked).unwrap());
    if owner() == 0 {
        let _ = std::os::unix::fs::lchown(&linked, Some(65534), None);
    }
    if owner() != 0 {
        let (ok, out) = check_at("[commands]\ntoolchains = [\"shell\"]\n", &linked);
        assert!(!ok && out.contains("a non-root user controls"), "{out}");
    }
}

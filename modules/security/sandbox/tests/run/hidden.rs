//! What a program can't see: hidden folders, the runtime folder's
//! sockets, `/etc` beyond what programs need, anything beside the
//! program, other users' folders.

use super::*;

/// The CA file `SSL_CERT_FILE` names, in a home folder the sandbox
/// hides, is read inside, that one file alone: not the key beside it, and
/// not written.
#[test]
fn the_named_ca_file_alone_is_read() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("ca-file");
    let certs = world.home.join("certs");
    std::fs::create_dir_all(&certs).unwrap();
    let ca = certs.join("corporate.pem");
    std::fs::write(&ca, "CORPORATE CA").unwrap();
    std::fs::write(certs.join("corporate.key"), "CORPORATE KEY").unwrap();
    let work = world.work.display().to_string();
    let script = format!(
        r#"cat "$SSL_CERT_FILE" > {work}/ca 2>/dev/null
cat {key} > {work}/key 2>/dev/null
echo more >> "$SSL_CERT_FILE" 2>/dev/null && echo written > {work}/written
true"#,
        key = certs.join("corporate.key").display(),
    );
    let output = world
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--files.write",
            &work,
            "--",
            "/bin/bash",
            "-c",
            &script,
        ])
        .env("SSL_CERT_FILE", &ca)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read(&world.work.join("ca")), "CORPORATE CA");
    assert_eq!(
        read(&world.work.join("key")),
        "",
        "the key beside it was read"
    );
    assert_eq!(
        read(&world.work.join("written")),
        "",
        "the CA file was written"
    );
    assert_eq!(read(&ca), "CORPORATE CA");
}

/// `--files.hide` closes a folder even to `read-home`, but for the `--files.write`
/// folder in it; a `--files.read` of a folder holding Link's settings doesn't
/// open them; and `check` tries each hidden folder.
#[test]
fn hidden_folders_stay_closed_but_for_what_is_named_in_them() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("hide");
    let agents = world.home.join("Link/Agents");
    let own = agents.join("main/home");
    std::fs::create_dir_all(&own).unwrap();
    std::fs::create_dir_all(agents.join("other/home")).unwrap();
    std::fs::write(agents.join("other/home/secret"), "ANOTHER AGENT'S").unwrap();
    let script = r#"cat "$HOME/Link/Agents/other/home/secret" > "$HOME/Link/Agents/main/home/other" 2>/dev/null
cat "$HOME/.lnk/safe/token" > "$HOME/Link/Agents/main/home/token" 2>/dev/null
cat "$HOME/.ssh/id_test" > "$HOME/Link/Agents/main/home/key" 2>/dev/null
echo mine > "$HOME/Link/Agents/main/home/mine"
true"#;
    let (agents_arg, own_arg, config_parent) = (
        agents.display().to_string(),
        own.display().to_string(),
        world.home.join(".config").display().to_string(),
    );
    let output = world
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--files.read",
            "~",
            "--files.hide",
            &agents_arg,
            "--files.write",
            &own_arg,
            "--files.read",
            &config_parent,
            "--",
            "/bin/bash",
            "-c",
            script,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read(&own.join("mine")), "mine\n");
    assert_eq!(
        read(&own.join("other")),
        "",
        "another agent's folder was read"
    );
    assert_eq!(read(&own.join("token")), "", "Link's settings were read");
    // read-home still reads the rest of the home folder.
    assert_eq!(read(&own.join("key")), "SECRET KEY");

    let check = world
        .lnk(&[
            "check",
            "--commands.toolchains",
            "shell",
            "--files.hide",
            &agents_arg,
            "--files.write",
            &own_arg,
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&check.stdout);
    assert!(check.status.success(), "{text}");
    assert!(
        text.contains("blocked  read what's hidden: ~/Link/Agents"),
        "{text}"
    );
    // Its bait is gone.
    let left: Vec<_> = std::fs::read_dir(&agents)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("lnk-check"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// A socket in the runtime folder, such as the one `ssh -R` binds for a
/// SOCKS upstream, stays hidden: every sandbox hides that folder, even
/// one reading the whole home folder; the folder is made first when it's
/// missing, so a socket put there later never shows in a sandbox already
/// running.
#[test]
fn a_runtime_socket_stays_hidden() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("runtime-socket");
    let runtime = link_files::layout::Root::Runtime.default_in(&world.short.join("run"));
    let socket = runtime.join("forward/upstream.sock");
    let script = format!(
        r#"ls -A "{}" > "$HOME/work/seen" 2>&1
test -S "{}" && echo reached >> "$HOME/work/seen"
true"#,
        runtime.display(),
        socket.display()
    );
    let run = || {
        let output = world
            .lnk(&[
                "run",
                "--commands.toolchains",
                "shell",
                "--files.read",
                "~",
                "--files.write",
                &world.work.display().to_string(),
                "--",
                "/bin/bash",
                "-c",
                &script,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        read(&world.work.join("seen"))
    };
    // A socket there, as sshd binds it.
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    assert_eq!(run(), "", "the sandbox saw the installation folder");
    // No installation folder yet: it's made, and the sandbox runs.
    drop(listener);
    std::fs::remove_dir_all(&runtime).unwrap();
    assert_eq!(run(), "");
    assert!(runtime.is_dir());
}

/// Of `/etc`, a program sees what programs need (who users are, the
/// certificates), never its secrets or what's put there by hand.
#[test]
fn of_etc_only_what_programs_need_is_shown() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("etc");
    let script = r#"for f in /etc/passwd /etc/nsswitch.conf /etc/ssl/certs; do
[ -e "$f" ] && echo "shown $f"
done
for f in /etc/shadow /etc/gshadow /etc/machine-id /etc/environment /etc/fstab /etc/ssh/ssh_host_ed25519_key /etc/ssh/ssh_host_rsa_key; do
[ -e "$f" ] && echo "LEAKED $f"
done
ls -A /etc/ssl/private 2>/dev/null | sed 's/^/LEAKED private /'
(echo x > /etc/ld.so.preload) 2>/dev/null && echo "LEAKED wrote /etc"
mkdir /etc/new 2>/dev/null && echo "LEAKED made /etc/new"
true"#;
    let output = world
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--",
            "/bin/sh",
            "-c",
            script,
        ])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!out.contains("LEAKED"), "{out}");
    for shown in ["/etc/passwd", "/etc/nsswitch.conf", "/etc/ssl/certs"] {
        if Path::new(shown).exists() {
            assert!(out.contains(&format!("shown {shown}")), "{shown}: {out}");
        }
    }
}

/// Run from `~/.lnk/exec/bin`, where `install.sh` puts it, the sandbox runs
/// `lnk-sandbox` (to confine, and to relay through the proxy) but reads
/// nothing else in that folder, with or without the proxy; `check` says
/// so.
#[test]
fn nothing_beside_the_program_is_readable() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("beside");
    let bin = world.home.join(".lnk/exec/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("lnk-sandbox");
    link_test_kit::copy_program(Path::new(env!("CARGO_BIN_EXE_lnk-sandbox")), &exe).unwrap();
    std::fs::write(bin.join("my-tool"), "SECRET TOOL").unwrap();
    let script = format!(
        "cat {bin}/my-tool; ls -A {bin}; echo end",
        bin = bin.display()
    );
    let sandbox = |args: &[&str]| {
        let mut command = link_test_kit::env::command(&exe, &world.home);
        command
            .args(args)
            .env("PATH", "/usr/bin:/bin")
            .current_dir(&world.work);
        command
    };
    for proxy in [&[][..], &["--internet.hosts", "*"]] {
        let output = sandbox(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--files.write",
            ".",
        ])
        .args(proxy)
        .args(["--", "/bin/sh", "-c", &script])
        .output()
        .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            text.ends_with("lnk-sandbox\nend\n"),
            "{proxy:?}: {text}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !text.contains("SECRET") && !text.contains("my-tool"),
            "{text}"
        );
    }
    let output = sandbox(&[
        "check",
        "--commands.toolchains",
        "shell",
        "--files.write",
        ".",
    ])
    .output()
    .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("blocked  list the folder lnk-sandbox is in (~/.lnk/exec/bin)"),
        "{text}"
    );
    assert!(!text.contains("leaked"), "{text}");
}

/// `/tmp`, `/var/tmp` and `/dev/shm`, which any user writes, are each
/// the sandbox's own: what's there outside isn't seen, and what it
/// writes there stays in it.
#[test]
fn every_folder_any_user_writes_is_its_own() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("shared-temp");
    let name = format!(".link-shared-{}", std::process::id());
    let shared: Vec<&Path> = ["/var/tmp", "/dev/shm"]
        .into_iter()
        .map(Path::new)
        .filter(|d| d.is_dir())
        .collect();
    for dir in &shared {
        std::fs::write(dir.join(&name), "OUTSIDE").unwrap();
    }
    let script = r#"for dir in /tmp /var/tmp /dev/shm; do
  [ -e "$dir/$1" ] && echo "$dir: seen"
  echo inside > "$dir/$1.made" && [ "$(cat "$dir/$1.made")" = inside ] && echo "$dir: its own"
done"#;
    let output = world
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--files.write",
            ".",
            "--",
            "/bin/bash",
            "-c",
            script,
            "probe",
            &name,
        ])
        .output()
        .unwrap();
    for dir in &shared {
        let _ = std::fs::remove_file(dir.join(&name));
    }
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        said.lines().collect::<Vec<_>>(),
        ["/tmp: its own", "/var/tmp: its own", "/dev/shm: its own"]
    );
    for dir in ["/tmp", "/var/tmp", "/dev/shm"] {
        let made = Path::new(dir).join(format!("{name}.made"));
        assert!(!made.exists(), "{} reached this machine", made.display());
    }
}

/// Under `--files.read ~`, a daemon's socket in a folder of daemons'
/// sockets (`~/.docker/run`) and an SSH connection's shared socket in
/// `~/.ssh` answer nothing inside, though both answer outside, and the
/// key beside the SSH socket is still read.
#[test]
fn sockets_in_the_home_folder_it_reads_answer_nothing() {
    if !link_test_kit::gate::bubblewrap()
        || !link_test_kit::gate::program("python3", &["--version"])
    {
        return;
    }
    let world = World::new("home-sockets");
    let run = world.home.join(".docker/run");
    std::fs::create_dir_all(&run).unwrap();
    let docker = run.join("docker.sock");
    let shared = world.home.join(".ssh/cm");
    // Bound and listening: a connection is taken into its backlog, unanswered.
    let _held: Vec<_> = [&docker, &shared]
        .iter()
        .map(|path| std::os::unix::net::UnixListener::bind(path).unwrap())
        .collect();
    for socket in [&docker, &shared] {
        assert!(std::os::unix::net::UnixStream::connect(socket).is_ok());
    }
    let work = world.work.display().to_string();
    let script = format!(
        r#"import socket, sys
for name, path in (("docker", sys.argv[1]), ("ssh", sys.argv[2])):
    s = socket.socket(socket.AF_UNIX)
    try:
        s.connect(path)
        open("{work}/" + name, "w").write("connected")
    except OSError:
        pass
open("{work}/key", "w").write(open(sys.argv[3]).read())
"#
    );
    let output = world
        .lnk(&[
            "run",
            "--commands.toolchains",
            "python",
            "--files.read",
            "~",
            "--files.write",
            &work,
            "--",
            "python3",
            "-c",
            &script,
            &docker.display().to_string(),
            &shared.display().to_string(),
            &world.home.join(".ssh/id_test").display().to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read(&world.work.join("key")), "SECRET KEY");
    assert_eq!(
        read(&world.work.join("docker")),
        "",
        "Docker's socket answered"
    );
    assert_eq!(
        read(&world.work.join("ssh")),
        "",
        "SSH's shared socket answered"
    );
}

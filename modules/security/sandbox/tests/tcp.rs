//! Raw protocols from a proxied sandbox, for real, on Linux, to the
//! endpoints it names (`--internet.tcp`), through its proxy:
//!
//! - Git over SSH: `git ls-remote ssh://...` in `lnk sandbox run
//!   --internet.hosts '*'`, through `GIT_SSH_COMMAND` (whose ProxyCommand is `sandbox
//!   connect`), to an sshd of the test's own serving a bare repository.
//! - A database's port, by name: a program connecting to
//!   `example.com:5432` reaches it at the address the sandbox's own
//!   `/etc/hosts` gives the name, and the proxy takes it there.
//!
//! The proxy's upstream is a SOCKS5 server of the test's, joining every
//! connection to a server of its own: the proxy refuses this machine, so
//! that server stands for a host on the internet (an address the proxy
//! allows).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use link_test_kit::{TempDir, gate};

fn run(command: &mut Command) -> String {
    let output = command.output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{command:?}: {text}");
    text
}

struct Sshd(Child);

impl Drop for Sshd {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// An sshd, the `program` installed, on `port` accepting `client`'s key
/// for this user.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn sshd(program: &Path, root: &Path, port: u16, client: &Path) -> Sshd {
    std::fs::copy(client.with_extension("pub"), root.join("authorized_keys")).unwrap();
    run(Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(root.join("hostkey")));
    let config = root.join("sshd_config");
    std::fs::write(
        &config,
        format!(
            "ListenAddress 127.0.0.1\nPort {port}\nHostKey {}\nAuthorizedKeysFile {}\n\
             StrictModes no\nUsePAM no\nPasswordAuthentication no\nPidFile none\n",
            root.join("hostkey").display(),
            root.join("authorized_keys").display(),
        ),
    )
    .unwrap();
    let _ = std::fs::create_dir_all("/run/sshd");
    let child = Command::new(program)
        .args(["-D", "-e", "-f"])
        .arg(&config)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "sshd didn't start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    Sshd(child)
}

/// A SOCKS5 server joining every connection, wherever it asks to go, to
/// 127.0.0.1:`to`. Its port.
fn socks_to(to: u16) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let _ = join(client, to);
            });
        }
    });
    port
}

fn join(mut client: TcpStream, to: u16) -> std::io::Result<()> {
    let mut head = [0u8; 2];
    client.read_exact(&mut head)?;
    client.read_exact(&mut vec![0u8; head[1] as usize])?;
    client.write_all(&[5, 0])?;
    let mut request = [0u8; 4];
    client.read_exact(&mut request)?;
    let skip = match request[3] {
        1 => 4,
        4 => 16,
        _ => {
            let mut n = [0u8; 1];
            client.read_exact(&mut n)?;
            n[0] as usize
        }
    };
    client.read_exact(&mut vec![0u8; skip + 2])?;
    client.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])?;
    let server = TcpStream::connect(("127.0.0.1", to))?;
    let (mut a, mut b) = (client.try_clone()?, server.try_clone()?);
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut a, &mut b);
        let _ = b.shutdown(std::net::Shutdown::Write);
    });
    let (mut a, mut b) = (server, client);
    std::io::copy(&mut a, &mut b)?;
    let _ = b.shutdown(std::net::Shutdown::Write);
    Ok(())
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn git_over_ssh_goes_through_the_proxy() {
    if !gate::bubblewrap() {
        return;
    }
    let Some(sshd_program) = gate::sshd() else {
        return;
    };
    if !gate::program("git", &["--version"]) || !gate::program("ssh", &["-V"]) {
        return;
    }
    let root = TempDir::new("tcp-ssh");
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    // A repository with one commit, and a key the sshd accepts.
    let repo = root.join("repo.git");
    run(Command::new("git")
        .args(["init", "-q", "--bare"])
        .arg(&repo));
    let seed = root.join("seed");
    run(Command::new("git").args(["init", "-q"]).arg(&seed));
    run(Command::new("git").arg("-C").arg(&seed).args([
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "one",
    ]));
    run(Command::new("git")
        .arg("-C")
        .arg(&seed)
        .args(["push", "-q"])
        .arg(&repo)
        .arg("HEAD:refs/heads/main"));
    let client = work.join("key");
    run(Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&client));
    // Its port, which sshd binds itself: held until the test ends.
    let held = crate::proxy::free_port();
    let sshd_port = held.number() as u16;
    let _sshd = sshd(&sshd_program, &root, sshd_port, &client);
    let upstream = socks_to(sshd_port);
    let user = run(Command::new("id").arg("-un")).trim().to_string();

    // In the sandbox: its GIT_SSH_COMMAND, with this test's key. A public
    // address stands for the host (the SOCKS server takes it to the sshd).
    let script = format!(
        "export GIT_SSH_COMMAND=\"$GIT_SSH_COMMAND -F /dev/null -i {key} -o BatchMode=yes \
         -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null\"\n\
         echo \"command: $GIT_SSH_COMMAND\"\n\
         git ls-remote ssh://{user}@93.184.215.14{repo} 2>&1\n\
         echo \"exit: $?\"\n",
        key = client.display(),
        repo = repo.display(),
    );
    let sandboxed = |tcp: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"));
        command
            .args([
                "run",
                "--commands.toolchains",
                "shell",
                "--commands.toolchains",
                "git",
                "--internet.hosts",
                "*",
                "--upstream",
            ])
            .arg(format!("socks5://127.0.0.1:{upstream}"))
            .arg("--files.write")
            .arg(&work);
        for endpoint in tcp {
            command.args(["--internet.tcp", endpoint]);
        }
        let output = command
            .args(["--", "/bin/sh", "-c", &script])
            .env("HOME", &root)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr)
    };
    // Port 22 isn't HTTPS's: refused unless the sandbox names the endpoint.
    let unnamed = sandboxed(&[]);
    let text = sandboxed(&["93.184.215.14:22"]);
    assert!(
        unnamed.contains("refused 93.184.215.14:22") && !unnamed.contains("refs/heads/main"),
        "{unnamed}"
    );
    assert!(text.contains("lnk-sandbox connect %h %p"), "{text}");
    assert!(
        text.contains("refs/heads/main"),
        "git over SSH didn't get through: {text}"
    );
    assert!(text.contains("exit: 0"), "{text}");
}

/// The names the proxy resolves, given a public address of IANA's own
/// block, 192.0.32.0/20, which the proxy reaches but the test never does
/// (the upstream joins it to the test's server), so no test needs this
/// machine's DNS. A documentation address would be refused.
const RESOLVE: (&str, &str) = (
    "LNK_TEST_RESOLVE",
    "example.com=192.0.32.1,www.example.com=192.0.32.1",
);

/// An echo server on this machine's loopback. Its port.
fn echo() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut client in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut reader = client.try_clone().unwrap();
                let _ = std::io::copy(&mut reader, &mut client);
            });
        }
    });
    port
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_database_port_is_reached_by_name() {
    if !gate::bubblewrap() {
        return;
    }
    let upstream = socks_to(echo());
    let program = r#"
import socket
out = []
for port in (5432, 5433):
    try:
        s = socket.create_connection(("example.com", port), timeout=5)
        s.sendall(b"ping %d" % port)
        out.append("%d:%s" % (port, s.recv(64).decode()))
    except OSError as e:
        out.append("%d:error %s" % (port, e))
out.append("hosts:" + " ".join(l for l in open("/etc/hosts").read().split("\n") if "example.com" in l))
print("\n".join(out))
"#;
    let root = TempDir::new("tcp-db");
    let output = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
        .args([
            "run",
            "--commands.toolchains",
            "python",
            "--internet.hosts",
            "*",
            "--upstream",
        ])
        .arg(format!("socks5://127.0.0.1:{upstream}"))
        .args([
            "--internet.tcp",
            "example.com:5432",
            "--",
            "python3",
            "-c",
            program,
        ])
        .env(RESOLVE.0, RESOLVE.1)
        .env("HOME", &root)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(
        text.contains("5432:ping 5432"),
        "the endpoint named wasn't reached: {text}"
    );
    // Its name is the endpoint's address in the sandbox, and nothing
    // listens there on a port not named.
    assert!(text.contains("hosts:127.0.1.1 example.com"), "{text}");
    assert!(text.contains("5433:error"), "{text}");
}

/// A `*.` endpoint: the sandbox's DNS gives a name under it an address of
/// the sandbox's own, through which the proxy reaches it; any other name
/// gets none.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_wildcard_endpoint_is_reached_through_the_sandboxs_dns() {
    if !gate::bubblewrap() {
        return;
    }
    let landlock = crate::landlock();
    let upstream = socks_to(echo());
    let program = r#"
import socket
out = []
for host in ("www.example.com", "example.org"):
    try:
        addr = socket.gethostbyname(host)
        s = socket.create_connection((host, 5432), timeout=5)
        s.sendall(b"ping " + host.encode())
        out.append("%s:%s %s" % (host, addr, s.recv(64).decode()))
    except OSError as e:
        out.append("%s:error %s" % (host, e))
print("\n".join(out))
"#;
    // As this user, and, run by root, as nobody too: the proxy's helper
    // joins the sandbox's network as its user namespace's owner, root's
    // or nobody's. Each with programs listed too, where the sandbox runs
    // under its exec watch.
    let root_run = unsafe { libc::geteuid() } == 0;
    let nobody = root_run && Path::new("/usr/bin/setpriv").exists() && {
        // Nobody reaches the sandbox program and a test's folder only
        // through folders anyone may enter: a checkout in a home closed to
        // others (mode 700) can't run that half.
        let enterable = |path: &Path| {
            use std::os::unix::fs::PermissionsExt;
            path.ancestors().skip(1).all(|dir| {
                std::fs::metadata(dir).is_ok_and(|m| m.permissions().mode() & 0o001 != 0)
            })
        };
        let here = [
            Path::new(env!("CARGO_BIN_EXE_lnk-sandbox")).to_path_buf(),
            std::env::temp_dir().join("x"),
        ];
        let open = here.iter().all(|p| enterable(p));
        if !open {
            gate::lacks(
                "its half run as nobody",
                "a folder above this checkout or the temp folder is closed to others",
                None,
            );
        }
        open
    };
    for (as_nobody, listed) in [(false, false), (false, true), (true, false), (true, true)] {
        if (as_nobody && !nobody) || (listed && !landlock) {
            continue;
        }
        let root = TempDir::new(&format!("tcp-wild-{as_nobody}-{listed}"));
        // Its runtime folder its own, as its user's.
        let run = link_test_kit::env::made_runtime(&root);
        let mut command = match as_nobody {
            true => {
                std::os::unix::fs::chown(&root, Some(65534), Some(65534)).unwrap();
                std::os::unix::fs::chown(&run, Some(65534), Some(65534)).unwrap();
                let mut setpriv = Command::new("/usr/bin/setpriv");
                setpriv
                    .args(["--reuid", "65534", "--regid", "65534", "--clear-groups"])
                    .arg(env!("CARGO_BIN_EXE_lnk-sandbox"));
                setpriv
            }
            false => Command::new(env!("CARGO_BIN_EXE_lnk-sandbox")),
        };
        let output = command
            .args(["run", "--upstream"])
            .arg(format!("socks5://127.0.0.1:{upstream}"))
            .args([
                "--internet.hosts",
                "*.example.com",
                "--internet.tcp",
                "*.example.com:5432",
            ])
            .args(match listed {
                true => &["--commands.allow", "python3"][..],
                false => &["--commands.toolchains", "python"],
            })
            .args(["--", "python3", "-c", program])
            .env(RESOLVE.0, RESOLVE.1)
            .env("HOME", &root)
            .env("XDG_RUNTIME_DIR", &run)
            .env("PATH", "/usr/bin:/bin")
            .current_dir(&root)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr);
        assert!(
            text.contains("www.example.com:127.1.0.1 ping www.example.com"),
            "the endpoint wasn't reached by its name (as nobody: {as_nobody}, programs \
             listed: {listed}): {text}"
        );
        assert!(text.contains("example.org:error"), "{text}");
        assert!(!text.contains("no DNS in the sandbox"), "{text}");
    }
}

//! A blank proxy, for real, on Linux (`lnk sandbox proxy --blank
//! --control <socket>`): started holding nothing of anyone's, it refuses
//! every call until one bind on its control socket gives it an agent's
//! rules, key and secrets, and is bound once. And a blank sandbox: a
//! policy naming no owner's path, reaching nothing but its proxy's
//! folder.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use link_sandbox_client::{ProxyBind, bind_proxy};
use link_sandbox_route::Placeholders;
use link_secretive::Duplicate;
use link_test_kit::TempDir;

/// The secret of the proxy's URLs in these tests.
const SECRET: &str = "0123456789abcdef0123456789abcdef";

/// The key the proxy is bound with.
const KEY: &str = "sk-ant-api03-the-real-secret";

/// A proxy, killed when dropped.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A blank proxy for the folder `net`, its control socket `control`,
/// held alone, run by `as_user` (`setpriv` and its arguments) if given.
fn blank_proxy(home: &Path, net: &Path, control: &Path, as_user: &[&str]) -> Running {
    blank_proxy_with(home, net, control, as_user, None)
}

/// [`blank_proxy`], with `--binder <uid>` when `binder` is given.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn blank_proxy_with(
    home: &Path,
    net: &Path,
    control: &Path,
    as_user: &[&str],
    binder: Option<&str>,
) -> Running {
    let mut command = match as_user.split_first() {
        Some((program, args)) => {
            let mut command = Command::new(program);
            command.args(args).arg(env!("CARGO_BIN_EXE_lnk-sandbox"));
            command
        }
        None => Command::new(env!("CARGO_BIN_EXE_lnk-sandbox")),
    };
    let child = command
        .args(["proxy", "--blank", "--control"])
        .arg(control)
        .arg("--dir")
        .arg(net)
        .arg("--hold")
        .args(binder.map(|uid| ["--binder", uid]).into_iter().flatten())
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        // IANA's own block, 192.0.32.0/20: public, so the proxy takes it,
        // and never reached.
        .env("LNK_TEST_RESOLVE", "api.anthropic.com=192.0.32.1")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !(control.exists() && net.join("out.sock").exists())
        && start.elapsed() < Duration::from_secs(20)
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(control.exists(), "the control socket never came");
    Running(child)
}

/// What the proxy at `net` answers `request`, whole.
fn ask(net: &Path, request: &str) -> String {
    let mut socket = UnixStream::connect(net.join("out.sock")).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    socket.write_all(request.as_bytes()).unwrap();
    let mut answer = Vec::new();
    let _ = socket.read_to_end(&mut answer);
    String::from_utf8_lossy(&answer).to_string()
}

/// A SOCKS5 server (no authentication) joining every connection to a
/// server of its own, which keeps what it's sent: the proxy refuses this
/// machine, so it stands for a host on the internet. Its port, and what
/// came.
fn upstream() -> (u16, Arc<Mutex<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    std::thread::spawn(move || {
        for mut client in listener.incoming().flatten() {
            let kept = kept.clone();
            std::thread::spawn(move || -> std::io::Result<()> {
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
                client.set_read_timeout(Some(Duration::from_secs(2)))?;
                let mut buf = [0u8; 4096];
                let n = client.read(&mut buf)?;
                kept.lock().unwrap().extend_from_slice(&buf[..n]);
                Ok(())
            });
        }
    });
    (port, seen)
}

/// A model runtime on this machine's loopback, answering every request
/// with its request line. Its port.
fn runtime() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = format!("saw {}", text.lines().next().unwrap_or_default());
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

fn request(line: &str, key: Option<&str>) -> String {
    let key = key.map_or(String::new(), |k| format!("x-api-key: {k}\r\n"));
    format!("{line} HTTP/1.1\r\nHost: 127.0.0.1\r\n{key}Content-Length: 2\r\n\r\n{{}}")
}

#[test]
fn a_blank_proxy_refuses_everything_until_it_is_bound_once() {
    let root = TempDir::new("blank-proxy");
    let (home, net, control) = (root.join("home"), root.join("net"), root.join("control"));
    std::fs::create_dir_all(&home).unwrap();
    link_files::create_private_dir(&net).unwrap();
    let _proxy = blank_proxy(&home, &net, &control, &[]);
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(&control).unwrap().permissions(),
    );
    assert_eq!(mode & 0o777, 0o600, "the control socket is only its user's");

    let model = runtime();
    let (socks, seen) = upstream();
    let local = format!("POST /_link/url/{SECRET}/here/{model}/api/chat");
    let made = Placeholders::new().unwrap();
    let key = link_model_contract::route::Hosted::new(
        &made,
        "anthropic",
        link_model_contract::route::HostedKey::Model,
        &KEY.to_string().into(),
        false,
    );
    let placeholder = key.placeholder.expose().clone();
    // Blank: every call refused, whatever it asks.
    for asked in [
        "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n".to_string(),
        "GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n".into(),
        request(&local, None),
        request("GET /_link/model/v1/models", Some(&placeholder)),
    ] {
        let answer = ask(&net, &asked);
        assert!(answer.starts_with("HTTP/1.1 503"), "{asked}: {answer}");
        assert!(answer.contains("isn't bound"), "{answer}");
    }

    // Its routes, each served by its module's bridge here, which the
    // first call starts; who calls them, bound in this home's keyring.
    let (routes, caller) = crate::routes_here(&home, &[model], Some((key, KEY)));
    let bind = ProxyBind {
        name: "slot-1".into(),
        upstream: format!("socks5://127.0.0.1:{socks}"),
        reach: [model].into(),
        routes,
        caller: Some(caller),
        url_secret: Some(SECRET.to_string().into()),
        ..ProxyBind::default()
    };
    // A bind that doesn't hold binds nothing: routes no one calls.
    let wrong = ProxyBind {
        caller: None,
        ..bind.duplicate()
    };
    let err = bind_proxy(&control, &wrong).unwrap_err();
    assert!(format!("{err:#}").contains("caller"), "{err:#}");
    assert!(ask(&net, &request(&local, None)).starts_with("HTTP/1.1 503"));

    bind_proxy(&control, &bind).unwrap();
    // Bound: as the proxy started with those arguments. A local model,
    // behind its URLs' secret.
    let answer = ask(&net, &request(&local, None));
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(answer.ends_with("saw POST /api/chat HTTP/1.1"), "{answer}");
    // The hosted model: a caller without the key's placeholder gets no
    // key added (one made for the same key by another run or holder
    // neither); with it, the call goes on to the provider, through the
    // upstream (which speaks no TLS, so it's a 502).
    let another = link_model_contract::route::placeholder(
        &Placeholders::new().unwrap(),
        &KEY.to_string().into(),
    );
    let stranger = ask(
        &net,
        &request("GET /_link/model/v1/models", Some(another.expose())),
    );
    assert!(stranger.starts_with("HTTP/1.1 403"), "{stranger}");
    let models = ask(
        &net,
        &request("GET /_link/model/v1/models", Some(&placeholder)),
    );
    assert!(
        models.starts_with("HTTP/1.1") && !models.contains(" 403 ") && !models.contains(" 503 "),
        "{models}"
    );
    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.windows(17).any(|w| w == b"api.anthropic.com"),
        "the provider wasn't dialed through the upstream"
    );
    // Its wire is the bound name's.
    let log = std::fs::read_to_string(home.join(".lnk/safe/sandboxes/wire.log")).unwrap();
    assert!(log.contains(r#""sandbox":"slot-1""#), "{log}");
    assert!(!log.contains(KEY), "{log}");

    // Bound once: the binding ends with the process.
    let again = bind_proxy(&control, &bind).unwrap_err();
    assert!(format!("{again:#}").contains("bound already"), "{again:#}");
    // A line that isn't a bind is refused, and said so.
    let mut socket = UnixStream::connect(&control).unwrap();
    socket
        .write_all(b"{\"upstream\":\"direct\",\"argv\":[]}\n")
        .unwrap();
    let mut answer = String::new();
    socket.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("{\"error\":"), "{answer}");
}

/// What the program in a blank sandbox tries: the harness's files, the
/// slot beside its own, this machine directly, and its proxy.
const PROGRAM: &str = r#"
cat "$1/harness.txt" 2>/dev/null && echo "harness: read"
echo x > "$1/harness.txt" 2>/dev/null && echo "harness: written"
cat "$2/owner.txt" 2>/dev/null && echo "other slot: read"
ls "$2" 2>/dev/null && echo "other slot: listed"
echo "home holds: $(ls -A "$HOME" | grep -vx .lnk | tr '\n' ' ')|"
echo x > "$HOME/scratch" && echo "home: written"
(exec 3<>/dev/tcp/127.0.0.1/$3) 2>/dev/null && echo "machine: reached"
exec 3<>/dev/tcp/127.0.0.1/40781 && printf 'CONNECT example.com:443 HTTP/1.1\r\n\r\n' >&3 \
  && echo "proxy: $(head -n 1 <&3 | tr -d '\r')"
echo done
"#;

/// A blank sandbox, wrapped as a machine that packs agents would: a
/// policy naming no owner's path (an empty home of its own, the
/// harness's files read only), reaching only a blank proxy's folder. It
/// reads the harness's files, writes none, reads nothing of the slot
/// beside it, and its only way out is its proxy, which refuses it.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_blank_sandbox_reaches_nothing_but_its_proxys_folder() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    // As this user, and, run by root, as nobody too, each slot's folders
    // its own user's.
    // SAFETY: geteuid has no preconditions.
    let root_run = unsafe { libc::geteuid() } == 0;
    let nobody = root_run && Path::new("/usr/bin/setpriv").exists() && {
        // Nobody reaches the sandbox program and a test's folder only
        // through folders anyone may enter.
        let enterable = |path: &Path| {
            use std::os::unix::fs::PermissionsExt;
            path.ancestors().skip(1).all(|dir| {
                std::fs::metadata(dir).is_ok_and(|m| m.permissions().mode() & 0o001 != 0)
            })
        };
        let here = [
            PathBuf::from(env!("CARGO_BIN_EXE_lnk-sandbox")),
            std::env::temp_dir().join("x"),
        ];
        let open = here.iter().all(|p| enterable(p));
        if !open {
            link_test_kit::gate::lacks(
                "its half run as nobody",
                "a folder above this checkout or the temp folder is closed to others",
                None,
            );
        }
        open
    };
    let machine = TcpListener::bind("127.0.0.1:0").unwrap();
    let machine_port = machine.local_addr().unwrap().port();
    for as_nobody in [false, true] {
        if as_nobody && !nobody {
            continue;
        }
        let root = TempDir::new(&format!("blank-sandbox-{as_nobody}"));
        let (slot, other) = (root.join("slot-1"), root.join("slot-2"));
        let (home, net, harness) = (slot.join("home"), slot.join("net"), root.join("harness"));
        for dir in [&home, &harness, &other] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(harness.join("harness.txt"), "the harness's own").unwrap();
        std::fs::write(other.join("owner.txt"), "another owner's").unwrap();
        let script = harness.join("program.sh");
        std::fs::write(&script, PROGRAM).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o700)).unwrap();
        let as_user: &[&str] = match as_nobody {
            true => {
                for dir in [&slot, &home] {
                    std::os::unix::fs::chown(dir, Some(65534), Some(65534)).unwrap();
                }
                std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
                &[
                    "/usr/bin/setpriv",
                    "--reuid",
                    "65534",
                    "--regid",
                    "65534",
                    "--clear-groups",
                ]
            }
            false => &[],
        };
        let lnk = |args: &[&str]| {
            let mut command = match as_user.split_first() {
                Some((program, rest)) => {
                    let mut command = Command::new(program);
                    command.args(rest).arg(env!("CARGO_BIN_EXE_lnk-sandbox"));
                    command
                }
                None => Command::new(env!("CARGO_BIN_EXE_lnk-sandbox")),
            };
            command
                .args(args)
                .env_clear()
                .env("HOME", &home)
                .env("PATH", "/usr/bin:/bin");
            command
        };
        let mut wrap = lnk(&["wrap"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let confinement = serde_json::json!({
            "name": "slot-1",
            "write": [],
            "read": [harness],
            "commands": "any",
            "net": {"reaching": {"reached": [{"dir": net, "port": 40781}], "ports": []}},
        });
        let request = serde_json::json!({
            "confinement": confinement,
            "argv": [
                "/bin/bash", script, harness, other, machine_port.to_string(),
            ],
        });
        wrap.stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let wrapped = wrap.wait_with_output().unwrap();
        assert!(
            wrapped.status.success(),
            "wrap failed (as nobody: {as_nobody})"
        );
        let wrapped: serde_json::Value = serde_json::from_slice(&wrapped.stdout).unwrap();
        let argv: Vec<String> = serde_json::from_value(wrapped["argv"].clone()).unwrap();
        let joined = argv.join(" ");
        assert!(joined.contains("--unshare-net"), "{joined}");
        // Nothing of the slot beside it is mounted, nor the folder holding
        // both.
        let mounted = |path: &Path| {
            argv.windows(2)
                .any(|w| w[0].contains("bind") && path.starts_with(&w[1]))
        };
        assert!(!mounted(&other), "{joined}");
        let _proxy = blank_proxy(&home, &net, &slot.join("control"), as_user);
        let mut sandbox = match as_user.split_first() {
            Some((program, rest)) => {
                let mut command = Command::new(program);
                command.args(rest).args(&argv);
                command
            }
            None => {
                let mut command = Command::new(&argv[0]);
                command.args(&argv[1..]);
                command
            }
        };
        let output = sandbox
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr);
        assert!(text.contains("done"), "as nobody: {as_nobody}: {text}");
        assert!(text.contains("harness: read"), "{text}");
        for never in [
            "harness: written",
            "other slot: read",
            "other slot: listed",
            "another owner's",
            "machine: reached",
        ] {
            assert!(
                !text.contains(never),
                "as nobody: {as_nobody}: {never}: {text}"
            );
        }
        // An empty home of its own, but for the folder holding the
        // personal Safe, which is closed over.
        assert!(text.contains("home holds: Link |"), "{text}");
        assert!(text.contains("home: written"), "{text}");
        assert!(text.contains("proxy: HTTP/1.1 503"), "{text}");
    }
}

/// The variable telling this program it's [`bind_as_stand_in`], and the
/// control socket it binds.
const BIND_AS: &str = "LNK_BLANK_BIND_AS";

/// Not a test of its own: a bind sent by another user, for
/// [`a_blank_proxy_takes_a_bind_from_its_binder_alone`], this program
/// run as that user with [`BIND_AS`] set. Without it, nothing.
#[test]
fn bind_as_stand_in() {
    let Ok(control) = std::env::var(BIND_AS) else {
        return;
    };
    let bind = ProxyBind {
        name: "slot-2".into(),
        upstream: "socks5://127.0.0.1:9".into(),
        ..ProxyBind::default()
    };
    match bind_proxy(Path::new(&control), &bind) {
        Ok(()) => println!("BOUND"),
        Err(err) => println!("REFUSED {err:#}"),
    }
}

/// A blank proxy run as one user with `--binder <uid>` takes a bind from
/// that uid, and from no other: not root's, not a third user's. Without
/// `--binder`, only its own user binds it. As root alone (it runs the
/// proxy and the binds as users of its own choosing).
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_blank_proxy_takes_a_bind_from_its_binder_alone() {
    // Only root runs the proxy and its binds as other users.
    if !link_test_kit::gate::root() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let base = 64_000 + std::process::id() % 500;
    let (proxy_uid, binder, stranger) = (base, base + 500, base + 1000);
    let root = TempDir::short("blank-binder");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o711)).unwrap();
    // This program, where the other users run it.
    let me = root.join("binds");
    link_test_kit::copy_program(&std::env::current_exe().unwrap(), &me).unwrap();
    let bind_as = |uid: u32, control: &Path| -> String {
        let output = Command::new("setpriv")
            .args([
                format!("--reuid={uid}"),
                format!("--regid={uid}"),
                "--clear-groups".into(),
                "--".into(),
            ])
            .arg(&me)
            .args([
                "--exact",
                "blank::bind_as_stand_in",
                "--nocapture",
                "--quiet",
            ])
            .env_clear()
            .env(BIND_AS, control)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|l| l.starts_with("BOUND") || l.starts_with("REFUSED"))
            .unwrap_or_else(|| panic!("the bind as {uid} said nothing: {output:?}"))
            .to_string()
    };
    let as_proxy = [
        "setpriv".to_string(),
        format!("--reuid={proxy_uid}"),
        format!("--regid={proxy_uid}"),
        "--clear-groups".into(),
        "--".into(),
    ];
    let as_proxy: Vec<&str> = as_proxy.iter().map(String::as_str).collect();
    for (name, with_binder) in [("with", true), ("without", false)] {
        // Its folders its own: its home, its proxy's folder and its
        // control socket's.
        let (home, net, at) = (
            root.join(format!("home-{name}")),
            root.join(format!("net-{name}")),
            root.join(format!("control-{name}")),
        );
        for folder in [&home, &net, &at] {
            std::fs::create_dir(folder).unwrap();
            std::os::unix::fs::chown(folder, Some(proxy_uid), Some(proxy_uid)).unwrap();
        }
        // The control socket's folder passable, for the binds.
        std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o711)).unwrap();
        let control = at.join("control.sock");
        let binder_flag = binder.to_string();
        let _proxy = blank_proxy_with(
            &home,
            &net,
            &control,
            &as_proxy,
            with_binder.then_some(binder_flag.as_str()),
        );
        // Others connect to it; the proxy decides who binds.
        std::fs::set_permissions(&control, std::fs::Permissions::from_mode(0o666)).unwrap();
        let refused = bind_proxy(
            &control,
            &ProxyBind {
                name: "slot-2".into(),
                upstream: "socks5://127.0.0.1:9".into(),
                ..ProxyBind::default()
            },
        )
        .unwrap_err();
        assert!(
            format!("{refused:#}").contains("only this proxy's user, or its binder, binds it"),
            "{name}: root: {refused:#}"
        );
        let third = bind_as(stranger, &control);
        assert!(
            third.starts_with("REFUSED"),
            "{name}: a third user: {third}"
        );
        let by_binder = bind_as(binder, &control);
        match with_binder {
            true => assert_eq!(by_binder, "BOUND", "{name}: its binder"),
            false => assert!(by_binder.starts_with("REFUSED"), "{name}: {by_binder}"),
        }
    }
}

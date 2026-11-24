//! A program with no sandbox, its proxy beside it, for real: what `lnk
//! sandbox wrap` makes of an `unwalled` request. The program runs as it
//! is, under `lnk sandbox proxy --listen`: it's pointed at the proxy,
//! which filters and logs what it's sent as its rules say, and nothing
//! holds the program there: it reaches this machine's ports and files
//! itself. An exit is refused: only a sandbox keeps a program to it.

use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use link_test_kit::TempDir;

/// What the program does: asks its proxy for a host its rules refuse,
/// with the run's secret and without, goes around it to one of this
/// machine's ports, reads a file no sandbox would show it, and prints
/// what happened.
const PROGRAM: &str = r#"
import base64, os, socket, sys
from urllib.parse import urlsplit
port, notes = int(sys.argv[1]), sys.argv[2]
proxy = os.environ.get("HTTPS_PROXY", "none")
print("proxy:" + proxy)
if proxy != "none":
    at = urlsplit(proxy)
    pair = base64.b64encode(f"{at.username}:{at.password}".encode()).decode()
    for name, auth in [("refused", f"Proxy-Authorization: Basic {pair}\r\n"), ("unauthorized", "")]:
        s = socket.create_connection((at.hostname, at.port), timeout=5)
        s.sendall(f"CONNECT refused.example:443 HTTP/1.1\r\nHost: refused.example:443\r\n{auth}\r\n".encode())
        print(name + ":" + s.recv(64).decode().splitlines()[0])
s = socket.create_connection(("127.0.0.1", port), timeout=2)
print("around:" + s.recv(64).decode())
print("notes:" + open(notes).read())
"#;

/// `lnk sandbox wrap` given `request`, with `home` its HOME.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn wrap(home: &Path, request: serde_json::Value) -> Output {
    let mut wrap = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
        .args(["wrap"])
        .env_clear()
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(home))
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wrap.stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    wrap.wait_with_output().unwrap()
}

/// A request to run `argv` with no wall, its proxy on `port` reaching
/// `allowed` only, through `upstream`, pointed there or not.
fn unwalled(port: u16, upstream: &str, pointed: bool, argv: &[String]) -> serde_json::Value {
    serde_json::json!({
        "unwalled": {
            "name": "bare-1",
            "proxy": {
                "upstream": upstream,
                "dir": "",
                "hosts": ["allowed.example"],
                "listen": port,
            },
            "pointed": pointed,
        },
        "argv": argv,
    })
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn with_no_wall_the_proxy_is_advice_the_program_may_ignore() {
    let home = TempDir::new("lnk-unwalled");
    let notes = home.join("notes.txt");
    std::fs::write(&notes, "kept outside any sandbox").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let around = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let _ = s.write_all(b"its own");
        }
    });
    // The proxy binds it: held, never found free and let go.
    let held = link_test_kit::HeldNumber::port();
    let port = held.port_number();
    let argv: Vec<String> = [
        "/usr/bin/python3".to_string(),
        "-c".into(),
        PROGRAM.into(),
        around.to_string(),
        notes.display().to_string(),
    ]
    .into();
    let wrapped = wrap(&home, unwalled(port, "direct", true, &argv));
    assert!(
        wrapped.status.success(),
        "{}",
        String::from_utf8_lossy(&wrapped.stderr)
    );
    let wrapped: link_sandbox_client::Wrapped = serde_json::from_slice(&wrapped.stdout).unwrap();
    // The proxy's command, and the program as it is: no wall around it.
    let joined = wrapped.argv.join(" ");
    let proxy = link_sandbox_contract::spec::SandboxProxyArgs::from_argv(&wrapped.argv[1..])
        .unwrap_or_else(|err| panic!("{joined}: {err}"));
    assert!(wrapped.argv[0].ends_with("lnk-sandbox"), "{joined}");
    assert_eq!(proxy.listen, Some(port), "{joined}");
    assert!(!joined.contains("bwrap"), "{joined}");
    assert_eq!(wrapped.argv[wrapped.argv.len() - argv.len()..], argv[..]);
    // The proxy's address names this wrap's secret, never its command line.
    let proxy = wrapped.env["HTTPS_PROXY"].clone();
    let secret = proxy
        .strip_prefix("http://lnk:")
        .and_then(|rest| rest.strip_suffix(&format!("@127.0.0.1:{port}")))
        .unwrap_or_else(|| panic!("{proxy}"));
    assert!(secret.len() >= 16, "{proxy}");
    assert!(!joined.contains(secret), "{joined}");
    let output = Command::new(&wrapped.argv[0])
        .args(&wrapped.argv[1..])
        .env_clear()
        .env("HOME", &*home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .envs(&wrapped.env)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    // Its proxy refuses what its rules do, and logs it under its name.
    assert!(text.contains(&format!("proxy:{proxy}")), "{text}");
    assert!(text.contains("refused:HTTP/1.1 403"), "{text}");
    // Without its secret, as a process another run left would ask:
    // refused before its rules are read.
    assert!(text.contains("unauthorized:HTTP/1.1 407"), "{text}");
    let log = std::fs::read_to_string(home.join(".lnk/safe/sandboxes/wire.log")).unwrap();
    assert!(log.contains(r#""sandbox":"bare-1""#), "{log}");
    assert!(log.contains("refused.example"), "{log}");
    // And nothing holds it there.
    assert!(text.contains("around:its own"), "{text}");
    assert!(text.contains("notes:kept outside any sandbox"), "{text}");
    assert!(output.status.success(), "{text}");

    // Not pointed at it (the network as it is): no proxy in its
    // environment, its proxy serving only Link's ways out.
    let wrapped = wrap(&home, unwalled(port, "direct", false, &argv));
    assert!(wrapped.status.success());
    let wrapped: link_sandbox_client::Wrapped = serde_json::from_slice(&wrapped.stdout).unwrap();
    assert!(
        !wrapped.env.contains_key("HTTPS_PROXY"),
        "{:?}",
        wrapped.env
    );
    assert!(!wrapped.env.contains_key("GIT_SSH_COMMAND"));

    // An exit remains advice: the program gets its proxy environment,
    // while no wall prevents it from ignoring that route.
    let routed = wrap(
        &home,
        unwalled(port, "socks5://127.0.0.1:1080", true, &argv),
    );
    assert!(
        routed.status.success(),
        "{}",
        String::from_utf8_lossy(&routed.stderr)
    );
    let routed: link_sandbox_client::Wrapped = serde_json::from_slice(&routed.stdout).unwrap();
    assert!(routed.env.contains_key("HTTPS_PROXY"), "{:?}", routed.env);
}

/// A formerly chosen-and-released port can be occupied between wrap and
/// execution. A dynamic proxy now owns its listener before its program
/// receives any addresses; inherited run URLs are filled then too.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_dynamic_proxy_binds_before_giving_its_program_the_port() {
    let home = TempDir::new("unwalled-bound-port");
    let argv = vec![
        "/usr/bin/python3".into(),
        "-c".into(),
        r#"
import os, socket
from urllib.parse import urlsplit
proxy = os.environ['HTTPS_PROXY']
print(proxy)
assert ':0/' not in os.environ['SERVICE_URL'], os.environ['SERVICE_URL']
import base64
at = urlsplit(proxy)
pair = base64.b64encode(f'{at.username}:{at.password}'.encode())
s = socket.create_connection((at.hostname, at.port), timeout=3)
s.sendall(b'CONNECT refused.example:443 HTTP/1.1\r\nHost: refused.example:443\r\nProxy-Authorization: Basic ' + pair + b'\r\n\r\n')
assert b'403' in s.recv(256)
"#
        .into(),
    ];
    let mut request = unwalled(1, "direct", true, &argv);
    request["unwalled"]["proxy"]
        .as_object_mut()
        .unwrap()
        .remove("listen");
    let output = wrap(&home, request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let wrapped: link_sandbox_client::Wrapped = serde_json::from_slice(&output.stdout).unwrap();
    let at = wrapped
        .argv
        .iter()
        .position(|arg| arg == "--listen")
        .unwrap();
    let chosen: u16 = wrapped.argv[at + 1].parse().unwrap();
    let launch = wrapped
        .argv
        .iter()
        .position(|arg| arg == "--launch")
        .unwrap();
    link_sandbox::Launch::given(
        Path::new(&wrapped.argv[launch + 1]),
        ["SERVICE_URL".to_string()],
    )
    .unwrap();
    let occupied = TcpListener::bind(("127.0.0.1", chosen)).unwrap();
    let output = Command::new(&wrapped.argv[0])
        .args(&wrapped.argv[1..])
        .env_clear()
        .env("HOME", &*home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .env("SERVICE_URL", "http://127.0.0.1:0/_link/example")
        .envs(&wrapped.env)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Its secret kept, the port the one bound.
    let root = String::from_utf8(output.stdout).unwrap();
    let root = root.trim();
    assert!(root.starts_with("http://lnk:"), "{root}");
    let port = occupied.local_addr().unwrap().port();
    assert!(!root.ends_with(&format!("@127.0.0.1:{port}")), "{root}");
    assert!(!root.ends_with("@127.0.0.1:0"), "{root}");
}

/// What a program beside a held port's proxy does: asks it for a host its
/// rules refuse, and says whether it holds a listener on the proxy's port
/// or was told of one.
const HELD: &str = r#"
import base64, os, socket, stat, sys
from urllib.parse import urlsplit
port = int(sys.argv[1])
at = urlsplit(os.environ["HTTPS_PROXY"])
pair = base64.b64encode(f"{at.username}:{at.password}".encode()).decode()
s = socket.create_connection((at.hostname, at.port), timeout=5)
s.sendall(f"CONNECT refused.example:443 HTTP/1.1\r\nHost: refused.example:443\r\nProxy-Authorization: Basic {pair}\r\n\r\n".encode())
print("refused:" + s.recv(64).decode().splitlines()[0])
leaked = False
for fd in range(3, 256):
    try:
        if not stat.S_ISSOCK(os.fstat(fd).st_mode):
            continue
        held = socket.socket(fileno=os.dup(fd))
        # No peer: a listener (macOS answers no SO_ACCEPTCONN question).
        try:
            held.getpeername()
        except OSError:
            leaked |= held.type == socket.SOCK_STREAM and held.getsockname()[1] == port
        held.close()
    except OSError:
        pass
print(f"leaked:{leaked}")
print("told:" + os.environ.get("LNK_PROXY_LISTENER_FD", "none"))
"#;

/// The port held for a proxy: never free from the moment it's chosen. A
/// port chosen by binding and letting go was free until the proxy bound
/// it again, for any program to take first. Held, it's handed to the
/// proxy open, which serves on that very listener and keeps it from its
/// program.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_held_port_is_its_proxys_from_choosing_to_serving() {
    let home = TempDir::new("unwalled-held-port");
    let held = link_sandbox_client::HeldPort::new().unwrap();
    let port = held.port().unwrap();
    let taken = TcpListener::bind(("127.0.0.1", port)).unwrap_err();
    assert_eq!(taken.kind(), std::io::ErrorKind::AddrInUse);
    let argv: Vec<String> = [
        "/usr/bin/python3".to_string(),
        "-c".into(),
        HELD.into(),
        port.to_string(),
    ]
    .into();
    let wrapped = wrap(&home, unwalled(port, "direct", true, &argv));
    assert!(
        wrapped.status.success(),
        "{}",
        String::from_utf8_lossy(&wrapped.stderr)
    );
    let wrapped: link_sandbox_client::Wrapped = serde_json::from_slice(&wrapped.stdout).unwrap();
    let run = |held: link_sandbox_client::HeldPort| {
        let mut command = Command::new(&wrapped.argv[0]);
        command
            .args(&wrapped.argv[1..])
            .env_clear()
            .env("HOME", &*home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
            .env("PATH", "/usr/bin:/bin")
            .envs(&wrapped.env);
        held.hand_to(&mut command);
        let output = command.output().unwrap();
        let text = String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr);
        (output.status.success(), text)
    };
    // This process holds the port as the proxy starts: had the proxy
    // bound the port itself, it couldn't have.
    let (ran, text) = run(held);
    assert!(ran, "{text}");
    assert!(text.contains("refused:HTTP/1.1 403"), "{text}");
    assert!(text.contains("leaked:False"), "{text}");
    assert!(text.contains("told:none"), "{text}");

    // Handed a listener on another port than its own: refused, and its
    // program never runs.
    let other = link_sandbox_client::HeldPort::new().unwrap();
    let (ran, text) = run(other);
    assert!(!ran, "{text}");
    assert!(text.contains(&format!("not 127.0.0.1:{port}")), "{text}");
    assert!(!text.contains("refused:"), "{text}");
}

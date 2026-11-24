//! A sandbox on the loopback alone, for real, on Linux: a program
//! (python) in the sandbox `lnk sandbox wrap` makes for a policy with
//! `loopback`. It reaches this machine's ports its policy names, and
//! nothing else: no other port, no internet, no proxy.

use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use link_test_kit::TempDir;

/// What the program does: tries the port its policy names, another of
/// this machine's, and the internet, and prints what happened, with
/// whether it was given a proxy.
const PROGRAM: &str = r#"
import os, socket, sys
allowed, other = int(sys.argv[1]), int(sys.argv[2])
def tried(name, address):
    try:
        s = socket.create_connection(address, timeout=2)
        return name + ":" + s.recv(64).decode()
    except OSError:
        return name + ":blocked"
print(tried("allowed", ("127.0.0.1", allowed)))
print(tried("other", ("127.0.0.1", other)))
print(tried("internet", ("1.1.1.1", 443)))
print("proxy:" + os.environ.get("HTTPS_PROXY", "none"))
"#;

/// A server on this machine's loopback answering `text`; its port.
fn serving(text: &'static [u8]) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let _ = s.write_all(text);
        }
    });
    port
}

/// `lnk sandbox wrap` given `confinement` and `argv`, with `home` its HOME.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn wrap(home: &Path, confinement: serde_json::Value, argv: &[String]) -> Output {
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
    let request = serde_json::json!({"confinement": confinement, "argv": argv});
    wrap.stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    wrap.wait_with_output().unwrap()
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_sandbox_on_the_loopback_reaches_its_ports_and_nothing_else() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let root = TempDir::new("loopback");
    let (home, work, net) = (root.join("home"), root.join("work"), root.join("net"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    let allowed = serving(b"harness here");
    let other = serving(b"another program");
    let script = work.join("program.py");
    std::fs::write(&script, PROGRAM).unwrap();
    let argv: Vec<String> = vec![
        "python3".into(),
        script.display().to_string(),
        allowed.to_string(),
        other.to_string(),
    ];
    let confinement = serde_json::json!({
        "name": "looped",
        "write": [work],
        "read": [],
        "commands": "any",
        "net": {"loopback": {"ports": [allowed], "dir": net}},
    });

    let wrapped = wrap(&home, confinement.clone(), &argv);
    assert!(
        wrapped.status.success(),
        "{}",
        String::from_utf8_lossy(&wrapped.stderr)
    );
    let wrapped: serde_json::Value = serde_json::from_slice(&wrapped.stdout).unwrap();
    let wrapped: Vec<String> = serde_json::from_value(wrapped["argv"].clone()).unwrap();
    let joined = wrapped.join(" ");
    assert!(joined.contains("--unshare-net"), "{joined}");
    assert!(!joined.contains("lnk-sandbox proxy"), "{joined}");
    let ran = Command::new(&wrapped[0])
        .args(&wrapped[1..])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&ran.stdout);
    assert!(
        ran.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        said.lines().collect::<Vec<_>>(),
        [
            "allowed:harness here",
            "other:blocked",
            "internet:blocked",
            "proxy:none"
        ]
    );
    // Its sockets' folder is only the user's.
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&net).unwrap().permissions());
    assert_eq!(mode & 0o777, 0o700);

    // No other way to a network beside it: another mode, or another
    // mode's key, is refused, not widened.
    for net in [
        serde_json::json!({
            "loopback": {"ports": [allowed], "dir": net},
            "host": {"ports": [], "localhost": true},
        }),
        serde_json::json!({"loopback": {
            "ports": [allowed],
            "dir": net,
            "proxy": {"upstream": "direct", "dir": root.join("proxy")},
        }}),
        serde_json::json!({"loopback": {"ports": [allowed], "dir": net, "localhost": true}}),
    ] {
        let mut wider = confinement.clone();
        wider["net"] = net.clone();
        let refused = wrap(&home, wider, &argv);
        assert!(!refused.status.success(), "{net}");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("reading the request"),
            "{}",
            String::from_utf8_lossy(&refused.stderr)
        );
    }
}

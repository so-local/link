//! The filtering proxy for real, on Linux: a program (python) in the
//! sandbox `lnk sandbox wrap` makes for a policy with a proxy. It reaches
//! the internet only through its proxy, this machine only at the ports its
//! policy names, and Link reaches the ports it serves.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use link_test_kit::TempDir;

/// A port for a server the sandbox starts, held by this test across test
/// programs until it's dropped. From below the ports the system hands
/// out, so the proxy's own listener, bound to a port of the system's
/// picking, never lands on it, and below the ports Link's own lookup
/// gives out.
pub(crate) fn free_port() -> link_test_kit::HeldNumber {
    let start = std::process::id() % 2000;
    let ports = (0..2000).map(|k| 16_500 + (start + k) % 2000);
    link_test_kit::HeldNumber::first("sandbox-port", ports, |port| {
        TcpListener::bind(("127.0.0.1", port as u16)).is_ok()
    })
}

/// What the program does: serves its port, tries this machine directly,
/// through its proxy and at the port its policy allows, and writes what
/// happened.
const PROGRAM: &str = r#"
import http.server, os, socket, sys, threading
port, host_port, model_port, out = int(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
class Hello(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200); self.end_headers(); self.wfile.write(b"harness here")
    def log_message(self, *a): pass
server = http.server.HTTPServer(("127.0.0.1", port), Hello)
threading.Thread(target=server.serve_forever, daemon=True).start()
results = []
try:
    socket.create_connection(("127.0.0.1", host_port), timeout=2); results.append("direct:reached")
except OSError:
    results.append("direct:blocked")
proxy = int(os.environ["HTTPS_PROXY"].rsplit(":", 1)[1])
try:
    s = socket.create_connection(("127.0.0.1", proxy), timeout=5)
    s.sendall(b"CONNECT 127.0.0.1:%d HTTP/1.1\r\n\r\n" % host_port)
    results.append("proxy:" + s.recv(64).decode().split("\r\n")[0])
except OSError as e:
    results.append("proxy:error " + str(e))
try:
    s = socket.create_connection(("127.0.0.1", model_port), timeout=5)
    results.append("model:" + s.recv(64).decode())
except OSError as e:
    results.append("model:error " + str(e))
open(out + ".tmp", "w").write("\n".join(results)); os.rename(out + ".tmp", out)
threading.Event().wait(30)
"#;

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_proxied_sandbox_reaches_nothing_but_its_proxy_and_its_ports() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let root = TempDir::new("proxy");
    let (home, work, net) = (root.join("home"), root.join("work"), root.join("net"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    let held = free_port();
    let port = held.number() as u16;
    // A server on this machine it must not reach, and a "model" it may.
    let host = TcpListener::bind("127.0.0.1:0").unwrap();
    let host_port = host.local_addr().unwrap().port();
    let model = TcpListener::bind("127.0.0.1:0").unwrap();
    let model_port = model.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in model.incoming().flatten() {
            let _ = s.write_all(b"model here");
        }
    });
    let script = work.join("program.py");
    std::fs::write(&script, PROGRAM).unwrap();
    let results = work.join("results");

    let lnk = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"));
        command
            .args(args)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
            .env("PATH", "/usr/bin:/bin");
        command
    };
    let mut wrap = lnk(&["wrap"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let request = serde_json::json!({
        "confinement": {
            "name": "proxied",
            "write": [work],
            "read": [],
            "commands": "any",
            "net": {"proxied": {
                "proxy": {"upstream": "direct", "dir": net, "serve": [port]},
                "ports": [model_port, port],
            }},
        },
        "argv": [
            "python3", script, port.to_string(), host_port.to_string(),
            model_port.to_string(), results,
        ],
    });
    wrap.stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let wrapped = wrap.wait_with_output().unwrap();
    assert!(wrapped.status.success());
    let wrapped: serde_json::Value = serde_json::from_slice(&wrapped.stdout).unwrap();
    let argv: Vec<String> = serde_json::from_value(wrapped["argv"].clone()).unwrap();
    let env: std::collections::BTreeMap<String, String> =
        serde_json::from_value(wrapped["env"].clone()).unwrap();
    assert_eq!(env["HTTPS_PROXY"], "http://127.0.0.1:40781");
    assert!(argv.join(" ").contains("--unshare-net"), "network given up");

    // Its wire's log is where wrap was asked, whatever HOME the program
    // gets (a harness's is its own folder).
    assert!(
        argv.join(" ").contains(&format!(
            "--wire {}",
            home.join(".lnk/safe/sandboxes").display()
        )),
        "{argv:?}"
    );
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .env_clear()
        .env("HOME", &work)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&work))
        .env("PATH", "/usr/bin:/bin")
        .envs(&env)
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !results.exists() && start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let got = std::fs::read_to_string(&results).unwrap_or_default();

    // Link reaches the port it serves from outside, through the relay.
    let mut answer = String::new();
    if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(b"GET / HTTP/1.0\r\n\r\n").unwrap();
        let _ = s.read_to_string(&mut answer);
    }
    let _ = child.kill();
    let _ = child.wait();
    // Each call on its wire, logged outside it by name.
    let log = lnk(&["log", "show", "--json", "--sandbox", "proxied"])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&log.stdout).to_string();

    assert!(got.contains("direct:blocked"), "{got}");
    assert!(got.contains("proxy:HTTP/1.1 403"), "{got}");
    assert!(got.contains("model:model here"), "{got}");
    assert!(answer.contains("harness here"), "{answer}");
    let calls: Vec<serde_json::Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let seen = |verb: &str, to: String, outcome: &str| {
        calls
            .iter()
            .any(|c| c["verb"] == verb && c["to"] == to && c["outcome"] == outcome)
    };
    assert!(
        seen("tcp", format!("127.0.0.1:{host_port}"), "refused"),
        "{log}"
    );
    assert!(
        seen("port", format!("127.0.0.1:{model_port}"), "allowed"),
        "{log}"
    );
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn check_through_the_proxy_finds_no_gaps() {
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    let root = TempDir::new("proxy-check");
    let home = root.join("home");
    std::fs::create_dir_all(home.join(".lnk")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
        .args(["check", "--internet.hosts", "*"])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for line in [
        "blocked  connect to another server on localhost",
        "blocked  ask its proxy for the server on localhost",
        "only through Link's proxy (direct)",
    ] {
        assert!(text.contains(line), "{line:?} in\n{text}");
    }
    assert!(
        !text.contains("open ") && !text.contains("leaked"),
        "{text}"
    );
}

/// The proxy as macOS runs it (`sandbox proxy --listen`), outside any
/// sandbox here: it refuses this machine, and the program's exit status
/// passes through.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn the_listening_proxy_refuses_this_machine() {
    let host = TcpListener::bind("127.0.0.1:0").unwrap();
    let host_port = host.local_addr().unwrap().port();
    let held = link_test_kit::Held::loopback();
    let port = held.port();
    let script = format!(
        r#"import socket, sys, time
for _ in range(50):
    try:
        s = socket.create_connection(("127.0.0.1", {port}), timeout=5); break
    except OSError:
        time.sleep(0.1)
s.sendall(b"CONNECT 127.0.0.1:{host_port} HTTP/1.1\r\n\r\n")
sys.exit(3 if s.recv(64).startswith(b"HTTP/1.1 403") else 1)
"#
    );
    let home = TempDir::new("proxy-listen");
    let lnk = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"));
        command
            .args(args)
            .env("HOME", &home)
            .env_remove("XDG_CONFIG_HOME")
            .env("LNK_EXE", "/nonexistent/lnk");
        command
    };
    let mut proxy = lnk(&["proxy", "--name", "run-1", "--listen", &port.to_string()]);
    proxy.args(["--upstream", "direct", "--", "python3", "-c", &script]);
    held.hand_to(&mut proxy, link_sandbox_client::HELD_LISTENER_ENV);
    let status = proxy.status().unwrap();
    assert_eq!(
        status.code(),
        Some(3),
        "the proxy didn't refuse this machine"
    );
    // The call is in the wire's log, outside the sandbox.
    let log = lnk(&["log", "show", "--json"]).output().unwrap();
    let log = String::from_utf8_lossy(&log.stdout);
    let entry: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
    assert_eq!(entry["sandbox"], "run-1");
    assert_eq!(entry["verb"], "tcp");
    assert_eq!(entry["to"], format!("127.0.0.1:{host_port}"));
    assert_eq!(entry["outcome"], "refused");
    // Stopped, and open again.
    assert!(lnk(&["stop"]).status().unwrap().success());
    let shown = lnk(&["log", "show"]).output().unwrap();
    assert!(String::from_utf8_lossy(&shown.stdout).contains("Every wire is stopped"));
    assert!(lnk(&["start"]).status().unwrap().success());
    let shown = lnk(&["log", "show", "--refused"]).output().unwrap();
    let shown = String::from_utf8_lossy(&shown.stdout);
    assert!(
        !shown.contains("stopped") && shown.contains("run-1"),
        "{shown}"
    );
}

/// `--host`: through its proxy, a sandbox reaches only the hosts listed,
/// refused by name before anything is resolved (no internet needed here).
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn only_the_hosts_listed_get_through() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let root = TempDir::new("proxy-hosts");
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let script = r#"ask() {
  exec 3<>"/dev/tcp/127.0.0.1/${HTTPS_PROXY##*:}"
  printf 'CONNECT %s:443 HTTP/1.1\r\n\r\n' "$1" >&3
  head -c 12 <&3; echo; exec 3<&-
}
echo "listed: $(ask www.example.org)"
echo "unlisted: $(ask example.com)""#;
    let output = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
        .args([
            "run",
            "--commands.toolchains",
            "shell",
            "--internet.hosts",
            "*.example.org",
        ])
        .args(["--", "/bin/bash", "-c", script])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("unlisted: HTTP/1.1 403"), "{text}");
    // Listed: dialed (200, or 502 without internet), never refused.
    let listed = text
        .lines()
        .find(|l| l.starts_with("listed: "))
        .unwrap_or_default();
    assert!(
        listed.starts_with("listed: HTTP/1.1") && !listed.contains("403"),
        "{text}"
    );
}

/// The listening proxy (macOS's) with a local model's route: a model
/// runtime on this machine reached on the route, through the models
/// module's bridge, for inference only.
#[test]
fn the_listening_proxy_serves_a_model_for_inference_only() {
    let runtime = TcpListener::bind("127.0.0.1:0").unwrap();
    let model = runtime.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in runtime.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = text.lines().next().unwrap_or_default().to_string();
            let host = text
                .lines()
                .find(|l| l.starts_with("Host:"))
                .unwrap_or_default()
                .to_string();
            let body = format!("saw {line} {host}");
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
        }
    });
    let held = free_port();
    let port = held.number() as u16;
    let script = format!(
        r#"import socket, sys, time
def ask(request):
    for _ in range(50):
        try:
            s = socket.create_connection(("127.0.0.1", {port}), timeout=5); break
        except OSError:
            time.sleep(0.1)
    s.sendall(request.encode() + b" HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 2\r\n\r\n{{}}")
    got = b""
    while True:
        chunk = s.recv(4096)
        if not chunk: break
        got += chunk
    return got.decode()
chat = ask("POST /_link/url/{SECRET}/here/{model}/api/chat")
pull = ask("POST /_link/url/{SECRET}/here/{model}/api/pull")
other = ask("POST /_link/url/{SECRET}/here/1/api/chat")
bare = ask("POST /_link/here/{model}/api/chat")
ok = chat.startswith("HTTP/1.1 200") and chat.endswith("saw POST /api/chat HTTP/1.1 Host: 127.0.0.1:{model}")
ok = ok and pull.startswith("HTTP/1.1 403") and other.startswith("HTTP/1.1 403")
ok = ok and bare.startswith("HTTP/1.1 403")
print(chat, pull, other, bare, sep="\n")
sys.exit(3 if ok else 1)
"#
    );
    let dir = TempDir::new("proxy-secret");
    let secret_file = dir.join("secret");
    std::fs::write(&secret_file, SECRET).unwrap();
    let (routes, caller) = crate::routes_here(&dir, &[model], None);
    let routes_file = dir.join("routes.json");
    let routed = serde_json::json!({"routes": routes, "caller": caller, "schema": 1});
    std::fs::write(&routes_file, routed.to_string()).unwrap();
    let output = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-sandbox"), &dir)
        .args(["proxy", "--listen", &port.to_string()])
        .args(["--upstream", "direct", "--routes"])
        .arg(&routes_file)
        .arg("--url-secret")
        .arg(&secret_file)
        .args(["--", "python3", "-c", &script])
        // Its own settings: a wire stopped or paused elsewhere never
        // reaches this proxy's.
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `[lan] *` on a cloud's machine is refused by the proxy itself, run
/// directly as by a spec: the cloud's private ranges are its network.
#[test]
fn the_proxy_refuses_a_clouds_whole_network() {
    let dir = TempDir::new("proxy-cloud-lan");
    let output = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-sandbox"), &dir)
        .args(["proxy", "--listen", "0", "--upstream", "direct"])
        .args(["--lan", "*", "--", "/bin/true"])
        .env("LNK_TEST_ON_CLOUD", "1")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{err}");
    assert!(err.contains("isn't taken on a cloud's machine"), "{err}");
}

/// `--localhost.models`: a local model runtime's port, reachable from a proxied
/// sandbox for inference only; its admin API is refused by Link and never
/// reaches the runtime.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_model_is_reached_for_inference_only() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    // The models module's bridge serves this route, found beside
    // lnk-sandbox: built there first, as a whole workspace's build would.
    let _bridge = crate::module("models");
    // Discovery is typed; inference still answers with its request line.
    let runtime = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = runtime.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for mut s in runtime.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = text.lines().next().unwrap_or_default().to_string();
            log.lock().unwrap().push(line.clone());
            let body = if line.starts_with("GET /v1/models ") {
                r#"{"data":[{"id":"qa-model","path":"/home/private/model.gguf"}]}"#.to_string()
            } else {
                format!("saw {line}")
            };
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
        }
    });
    let root = TempDir::new("proxy-model");
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let script = r#"PORT=$1
ask() {
  exec 3<>"/dev/tcp/127.0.0.1/$PORT"
  printf '%s HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\n{}' "$1" >&3
  head -n 1 <&3 | tr -d '\r'; exec 3<&-
}
echo "tags: $(ask 'GET /api/tags')"
echo "chat: $(ask 'POST /api/chat')"
echo "pull: $(ask 'POST /api/pull')"
echo "delete: $(ask 'DELETE /api/delete')"
echo "encoded: $(ask 'POST /api/%70ull')"
PORT=40781
eval "url=\${LNK_LOCAL_MODEL_$1}"
base="/${url#http://*/}"
echo "verb chat: $(ask "POST $base/api/chat")"
echo "verb pull: $(ask "POST $base/api/pull")"
echo "verb other port: $(ask "POST ${base%/*}/1/api/chat")"
echo "verb without its secret: $(ask "POST /_link/here/$1/api/chat")""#;
    let output = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
        .args([
            "run",
            "--commands.toolchains",
            "shell",
            "--internet.hosts",
            "*",
            "--localhost.models",
            &port.to_string(),
        ])
        .args(["--", "/bin/bash", "-c", script, "ask", &port.to_string()])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    for line in [
        "tags: HTTP/1.1 200 OK",
        "chat: HTTP/1.1 200 OK",
        "pull: HTTP/1.1 403 Forbidden",
        "delete: HTTP/1.1 403 Forbidden",
        "encoded: HTTP/1.1 403 Forbidden",
        "verb chat: HTTP/1.1 200 OK",
        "verb pull: HTTP/1.1 403 Forbidden",
        "verb other port: HTTP/1.1 403 Forbidden",
        "verb without its secret: HTTP/1.1 403 Forbidden",
    ] {
        assert!(
            text.contains(line),
            "{line:?} in\n{text}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "GET /v1/models HTTP/1.1",
            "POST /api/chat HTTP/1.1",
            "POST /api/chat HTTP/1.1"
        ],
        "the runtime saw only inference"
    );
}

/// A hosted model's key, added by the proxy: the program reaches the
/// `model` verb with no key, never sees the key (not in its environment,
/// nor the file the proxy reads it from), and the verb passes inference
/// only.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_hosted_models_key_stays_outside() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let root = TempDir::new("proxy-key");
    let (home, net) = (root.join("home"), root.join("net"));
    std::fs::create_dir_all(&home).unwrap();
    // This run's placeholder for the key, and one another run made for it.
    let key: link_secretive::Secret = "sk-ant-api03-the-real-secret".to_string().into();
    let made = link_model_contract::route::placeholder(
        &link_sandbox_route::Placeholders::new().unwrap(),
        &key,
    );
    let another = link_model_contract::route::placeholder(
        &link_sandbox_route::Placeholders::new().unwrap(),
        &key,
    );
    let script = r#"ask() {
  exec 3<>/dev/tcp/127.0.0.1/40781
  printf '%s HTTP/1.1\r\nHost: 127.0.0.1\r\nx-api-key: %s\r\nContent-Length: 2\r\n\r\n{}' "$1" "${2:-PLACEHOLDER}" >&3
  head -n 1 <&3 | tr -d '\r'; exec 3<&-
}
echo "stranger: $(ask 'GET /_link/model/v1/models' ANOTHER)"
echo "files: $(ask 'POST /_link/model/v1/files')"
echo "batches: $(ask 'POST /_link/model/v1/messages/batches')"
echo "models: $(ask 'GET /_link/model/v1/models')"
cat "$HOME/.lnk/safe/sandboxes/keyed.routes.json" 2>/dev/null && echo "key file: read"
env | grep -q "the-real-sec""ret" && echo "key: in the environment"
echo done"#
        .replace("PLACEHOLDER", made.expose())
        .replace("ANOTHER", another.expose());
    // Named on the route: the models module's bridge reads the key from
    // the secrets granted to its caller.
    let hosted = link_model_contract::route::Hosted {
        provider: "anthropic".into(),
        key: link_model_contract::route::HostedKey::Model,
        placeholder: made.duplicate(),
        server_tools: false,
        at: None,
    };
    let (routes, caller) = crate::routes_here(&home, &[], Some((hosted, key.expose())));
    let request = serde_json::json!({
        "confinement": {
            "name": "keyed",
            "write": [],
            "read": [],
            "commands": "any",
            "net": {"proxied": {
                "proxy": {
                    "upstream": "direct",
                    "dir": net,
                    "routes": routes,
                    "caller": caller,
                },
                "ports": [],
            }},
        },
        "argv": ["/bin/bash", "-c", script],
    });
    let mut wrap = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
        .args(["wrap"])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    wrap.stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let wrapped = wrap.wait_with_output().unwrap();
    assert!(wrapped.status.success());
    let wrapped: serde_json::Value = serde_json::from_slice(&wrapped.stdout).unwrap();
    let argv: Vec<String> = serde_json::from_value(wrapped["argv"].clone()).unwrap();
    assert!(
        !argv.join(" ").contains("the-real-secret"),
        "never on a command line"
    );
    let key_file = home.join(".lnk/safe/sandboxes/keyed.routes.json");
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(&key_file).unwrap().permissions(),
    );
    assert_eq!(mode & 0o077, 0, "only the user's");
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("done"), "{text}");
    // Only the program given the placeholder: a caller without it, as
    // another user on a Mac would be, gets no key added.
    assert!(text.contains("stranger: HTTP/1.1 403"), "{text}");
    assert!(text.contains("files: HTTP/1.1 403"), "{text}");
    assert!(text.contains("batches: HTTP/1.1 403"), "{text}");
    // Passed on: the provider's answer (401 for this key), or 502 with no
    // internet here; never refused by Link.
    let models = text
        .lines()
        .find(|l| l.starts_with("models: "))
        .unwrap_or_default();
    assert!(
        models.starts_with("models: HTTP/1.1") && !models.contains("403"),
        "{text}"
    );
    assert!(!text.contains("key file: read"), "{text}");
    assert!(!text.contains("key: in the environment"), "{text}");
}

/// A service's secret, the same, with the rule given as data (Telegram's,
/// as `lnk-telegram rule --json` gives it): the program reaches the
/// service on the `secret` verb with a placeholder, never sees the
/// secret, and can't send the bot elsewhere.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_services_secret_stays_outside() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let root = TempDir::new("proxy-bot");
    let (home, net) = (root.join("home"), root.join("net"));
    std::fs::create_dir_all(&home).unwrap();
    // This run's placeholder for the token, and one another run made for it.
    let token: link_secretive::Secret = "42:real-bot-secret".to_string().into();
    let placeholder = |made: link_sandbox_route::Placeholders| {
        link_secret_contract::route::placeholder(&made, "telegram", &token, Some(":"))
    };
    let made = placeholder(link_sandbox_route::Placeholders::new().unwrap());
    let another = placeholder(link_sandbox_route::Placeholders::new().unwrap());
    let script = r#"ask() {
  exec 3<>/dev/tcp/127.0.0.1/40781
  printf '%s HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n' "$1" >&3
  head -n 1 <&3 | tr -d '\r'; exec 3<&-
}
echo "stranger: $(ask 'GET /_link/secret/telegram/botANOTHER/getMe')"
echo "webhook: $(ask 'GET /_link/secret/telegram/botPLACEHOLDER/setWebhook?url=https://evil.example')"
echo "out: $(ask 'GET /_link/secret/telegram/botPLACEHOLDER/../../x')"
echo "getMe: $(ask 'GET /_link/secret/telegram/botPLACEHOLDER/getMe')"
cat "$HOME/.lnk/safe/sandboxes/botted.routes.json" 2>/dev/null && echo "token file: read"
env | grep -q "real-bot-sec""ret" && echo "token: in the environment"
echo done"#
        .replace("PLACEHOLDER", made.expose())
        .replace("ANOTHER", another.expose());
    // Named on the route: the secret module's bridge reads the token from
    // the secrets granted to its caller.
    crate::grant_here(
        &home,
        "AGENT_TELEGRAM_BOT_TOKEN",
        token.expose(),
        "{ kind = \"own\", purpose = \"connection:telegram:bot_token\" }",
    );
    let sealed = link_secret_contract::route::Named {
        name: "telegram".into(),
        assertion: None,
        kept: link_secret_contract::route::Kept::Own(
            link_secret_contract::Use::own("connection:telegram:bot_token").unwrap(),
        ),
        placeholder: made.duplicate(),
        rule: serde_json::from_value(serde_json::json!({
            "host": "api.telegram.org",
            "paths": [
                {"prefix": "bot{secret}/", "rest": "name"},
                {"prefix": "file/bot{secret}/", "rest": "path"},
            ],
            "refuse": ["bot{secret}/setwebhook", "bot{secret}/logout", "bot{secret}/close"],
            "public": ":",
        }))
        .unwrap(),
    };
    let bridge = link_sandbox_route::Bridge::at(
        &home.join("run"),
        &home.join(".lnk"),
        &crate::module("secret"),
        "secret",
        None,
        &home.join(".lnk/safe/agents/credentials"),
    );
    let routes =
        vec![link_secret_contract::route::route(bridge, Vec::new(), vec![sealed]).unwrap()];
    let request = serde_json::json!({
        "confinement": {
            "name": "botted",
            "write": [],
            "read": [],
            "commands": "any",
            "net": {"proxied": {"ports": [], "proxy": {
                "upstream": "direct",
                "dir": net,
                "routes": routes,
                "caller": crate::caller_here(&home),
            }}},
        },
        "argv": ["/bin/bash", "-c", script],
    });
    let mut wrap = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
        .args(["wrap"])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    wrap.stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let wrapped = wrap.wait_with_output().unwrap();
    assert!(wrapped.status.success());
    let wrapped: serde_json::Value = serde_json::from_slice(&wrapped.stdout).unwrap();
    let argv: Vec<String> = serde_json::from_value(wrapped["argv"].clone()).unwrap();
    assert!(
        !argv.join(" ").contains("real-bot-secret"),
        "never on a command line"
    );
    let token_file = home.join(".lnk/safe/sandboxes/botted.routes.json");
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(&token_file).unwrap().permissions(),
    );
    assert_eq!(mode & 0o077, 0, "only the user's");
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let log =
        std::fs::read_to_string(home.join(".lnk/safe/sandboxes/wire.log")).unwrap_or_default();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("done"), "{text}");
    assert!(text.contains("stranger: HTTP/1.1 403"), "{text}");
    assert!(text.contains("webhook: HTTP/1.1 403"), "{text}");
    assert!(text.contains("out: HTTP/1.1 403"), "{text}");
    // Passed on, never refused by Link: the answer is Telegram's (401
    // for this token), or whatever this machine's egress gives.
    assert!(text.contains("getMe: HTTP/1.1"), "{text}");
    assert_eq!(
        log.matches(r#""outcome":"refused""#).count(),
        3,
        "only the stranger, the webhook and the way out: {log}"
    );
    assert!(!text.contains("token file: read"), "{text}");
    assert!(!text.contains("token: in the environment"), "{text}");
    assert!(
        log.contains(r#""verb":"secret","to":"api.telegram.org:443""#),
        "{log}"
    );
    assert!(!log.contains("real-bot-secret"), "{log}");
}

/// The secret of the proxy's URLs in these tests.
const SECRET: &str = "0123456789abcdef0123456789abcdef";

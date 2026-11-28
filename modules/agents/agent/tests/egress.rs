//! An agent's network, for real, on Linux: `lnk agent start` runs the
//! harness in the sandbox module's proxied sandbox, with an exit or not,
//! and with `lan` in the network as it is, and its install behind a proxy
//! of its own, to its adapter's hosts (the proxy itself is tested in the
//! sandbox module's `tests/proxy.rs`).

use std::net::TcpListener;
use std::process::Command;

use link_test_kit::TempDir;

use crate::common;

mod upstream;

/// `lnk agent start --foreground` with `settings` added to the agent's:
/// the real sandbox, built by `up()`, around a fake harness that tries to
/// get out, when it runs (`step` `command`) or installs (`install`, its
/// adapter naming one host, `nowhere.invalid`); what it saw. None:
/// bubblewrap can't run here.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn try_from_inside(
    name: &str,
    settings: &str,
    sandbox: &str,
    step: &str,
    resolve: Option<&str>,
) -> Option<String> {
    if !link_test_kit::gate::sandbox_with_landlock() {
        return None;
    }
    let python = String::from_utf8(
        Command::new("sh")
            .args(["-c", "command -v python3"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let python = python.trim().to_string();
    let root = TempDir::new(&format!("egress-{name}"));
    let (home, bin) = (root.join("home"), root.join("bin"));
    let files = home.join("Link/Agents/main/home");
    std::fs::create_dir_all(home.join("Link/Agents/main")).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    crate::common::write_agent(
        &home,
        "main",
        &format!(
            "harness = \"fake\"\nfiles = \"{}\"\n{settings}\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-x\"\n",
            files.display()
        ),
    );
    // What you grant the agent, its sandbox file.
    if !sandbox.is_empty() {
        std::fs::create_dir_all(home.join(".lnk/safe/agents/main")).unwrap();
        let sandbox = format!("schema = 1\n{sandbox}");
        std::fs::write(home.join("Link/Safe/Agents/main/sandbox.toml"), sandbox).unwrap();
    }
    let host = TcpListener::bind("127.0.0.1:0").unwrap();
    let host_port = host.local_addr().unwrap().port();
    let script = bin.join("try.py");
    std::fs::write(
        &script,
        format!(
            r#"import os, socket, sys
out = []
try:
    socket.create_connection(("127.0.0.1", {host_port}), timeout=2); out.append("direct:reached")
except OSError:
    out.append("direct:blocked")
proxy = os.environ.get("HTTPS_PROXY", "")
out.append("env:" + proxy)
try:
    s = socket.create_connection(("127.0.0.1", int(proxy.rsplit(":", 1)[1])), timeout=5)
    s.sendall(b"CONNECT 127.0.0.1:{host_port} HTTP/1.1\r\n\r\n")
    out.append("proxy:" + s.recv(64).decode().split("\r\n")[0])
except Exception as e:
    out.append("proxy:error " + str(e))
for host in ["example.com", "nowhere.invalid"]:
    try:
        s = socket.create_connection(("127.0.0.1", int(proxy.rsplit(":", 1)[1])), timeout=5)
        s.sendall(("CONNECT " + host + ":443 HTTP/1.1\r\n\r\n").encode())
        out.append(host + ":" + s.recv(64).decode().split("\r\n")[0])
    except Exception as e:
        out.append(host + ":error " + str(e))
where = os.environ["LNK_FILES"] if sys.argv[1] == "command" else os.environ["HOME"]
open(where + "/results", "w").write("\n".join(out))
"#
        ),
    )
    .unwrap();
    // The harness reads its files folder. An install step reads its
    // adapter's program and nothing else beside it, so the script is in
    // the adapter itself.
    std::fs::copy(&script, files.join("try.py")).unwrap();
    let inline = std::fs::read_to_string(&script).unwrap();
    std::fs::remove_file(&script).unwrap();
    let adapter = bin.join("lnk-fake");
    crate::common::write_adapter(
        &adapter,
        format!(
            "#!/bin/sh\ncase \"$1\" in\n  command) printf '{{\"argv\":[\"{python}\",\"%s\",\"command\"],\"ports\":[]}}' \"$LNK_FILES/try.py\" ;;\n  internet-hosts) echo '[\"nowhere.invalid\"]' ;;\n  {step}) [ {step} = command ] || {python} - {step} <<'EOF'\n{inline}EOF\n ;;\n  *) exit 0 ;;\nesac\n",
        ),
    )
    .unwrap();
    let core = common::core_with_sandbox(&bin);

    let output = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args(["start", "--foreground"])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_EXE", &core)
        .env("LNK_FILES", &files)
        .envs(resolve.map(|names| ("LNK_TEST_RESOLVE", names)))
        .output()
        .unwrap();
    let results = match step {
        "command" => files.join("results"),
        _ => home.join(".lnk/data/agents/main/harnesses/fake/results"),
    };
    let results = std::fs::read_to_string(results).unwrap_or_default();
    Some(format!(
        "{results}\n--\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn only_its_proxy(results: &str) {
    assert!(results.contains("direct:blocked"), "{results}");
    assert!(results.contains("env:http://127.0.0.1:40781"), "{results}");
    assert!(results.contains("proxy:HTTP/1.1 403"), "{results}");
}

#[test]
fn with_an_exit_the_harness_has_nothing_but_its_proxy() {
    if let Some(results) = try_from_inside("exit", "upstream = \"direct\"\n", "", "command", None) {
        only_its_proxy(&results);
    }
}

#[test]
fn every_harness_with_network_has_nothing_but_its_proxy() {
    if let Some(results) = try_from_inside("default", "", "", "command", None) {
        only_its_proxy(&results);
    }
}

#[test]
fn an_install_reaches_only_its_adapters_hosts_through_a_proxy_of_its_own() {
    if let Some(results) = try_from_inside("install", "", "", "install", None) {
        only_its_proxy(&results);
        // Not listed: refused by name. Listed: dialed (and, as it doesn't
        // exist, not reached).
        assert!(results.contains("example.com:HTTP/1.1 403"), "{results}");
        assert!(
            results.contains("nowhere.invalid:HTTP/1.1 502"),
            "{results}"
        );
    }
}

#[test]
fn a_harness_reaches_any_host_unless_its_list_says_otherwise() {
    let upstream = upstream::Public::new();
    let settings = format!("upstream = \"{}\"\n", upstream.url());
    if let Some(results) = try_from_inside("any", &settings, "", "command", Some(upstream::RESOLVE))
    {
        only_its_proxy(&results);
        assert!(results.contains("example.com:HTTP/1.1 200"), "{results}");
        assert!(
            results.contains("nowhere.invalid:HTTP/1.1 502"),
            "{results}"
        );
        assert_eq!(
            upstream.finish(),
            ["example.com:443", "nowhere.invalid:443"]
        );
    }
}

#[test]
fn with_lan_the_harness_has_the_network_as_it_is() {
    let lan = "[network]\nmode = \"host\"\n[internet]\nhosts = [\"*\"]\n";
    if let Some(results) = try_from_inside("lan", "", lan, "command", None) {
        assert!(results.contains("direct:reached"), "{results}");
        assert!(results.contains("env:\n"), "{results}");
    }
}

/// Behind its proxy, only the ports its `Launch` serves (its gateway, its
/// health check) are on this machine's loopback, where every local user
/// can reach them: a connection's daemon beside it, on another of its ports,
/// stays inside its sandbox.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn only_the_served_ports_are_reachable_from_outside() {
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    // Its agent's block of two ports, which the harness binds itself: held
    // until the test ends, below Link's own allocation, as the teams
    // tests hold theirs. On Linux the last of a block, a Mac's proxy's, is
    // the harness's to use inside its sandbox.
    let held = link_test_kit::HeldNumber::first("ports", (9000..11000).step_by(2), |base| {
        (base..base + 2).all(|port| std::net::TcpListener::bind(("127.0.0.1", port as u16)).is_ok())
    });
    let (served, inside) = (held.port_number(), held.port_number() + 1);
    let root = TempDir::new("egress-served");
    let (home, bin) = (root.join("home"), root.join("bin"));
    let files = home.join("Link/Agents/main/home");
    std::fs::create_dir_all(home.join("Link/Agents/main")).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    crate::common::write_agent(
        &home,
        "main",
        &format!(
            "harness = \"fake\"\nfiles = \"{}\"\nport = {served}\nblock = 2\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-x\"\n",
            files.display()
        ),
    );
    // The harness listens on both its ports, answering what it's asked
    // with "gateway" and "daemon", until the test is done.
    std::fs::write(
        files.join("serve.py"),
        format!(
            r#"import os, socket, threading, time
def serve(port, word):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", port)); s.listen()
    while True:
        c, _ = s.accept(); c.recv(64); c.sendall(word.encode()); c.close()
for port, word in [({served}, "gateway"), ({inside}, "daemon")]:
    threading.Thread(target=serve, args=(port, word), daemon=True).start()
files = os.environ["LNK_FILES"]
open(files + "/listening", "w").close()
for _ in range(300):
    if os.path.exists(files + "/done"): break
    time.sleep(0.1)
"#
        ),
    )
    .unwrap();
    link_test_kit::write_program(
        &bin.join("lnk-fake"),
        format!(
            "#!/bin/sh\ncase \"$1\" in\n  contract) printf '{{\"version\":1,\"ports\":1}}' ;;\n  command) printf '{{\"argv\":[\"python3\",\"%s\"],\"ports\":[{served},{inside}],\"serve\":[{served}]}}' \"$LNK_FILES/serve.py\" ;;\n  *) exit 0 ;;\nesac\n"
        ),
    )
    .unwrap();
    let core = common::core_with_sandbox(&bin);
    let log = root.join("harness.stderr");
    let mut agent = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args(["start", "--foreground"])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_EXE", &core)
        .env("LNK_FILES", &files)
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    // Its readiness is the harness's own word, `listening`, or the agent
    // ending without it; the deadline only bounds a start that hangs, and
    // a whole start runs several sandboxed programs, slow on a busy
    // machine.
    let ready = link_test_kit::wait::within(
        "the harness to listen, or the agent to end",
        std::time::Duration::from_secs(120),
        || {
            if files.join("listening").exists() {
                return Some(true);
            }
            agent.try_wait().unwrap().map(|_| false)
        },
    );
    if ready != Ok(true) {
        let _ = agent.kill();
        let _ = agent.wait();
        panic!(
            "the harness never listened ({ready:?}): {}",
            std::fs::read_to_string(&log).unwrap()
        );
    }
    // A served port passes a connection in once it says something, so
    // each says a line before it reads the answer.
    let answer = |port: u16| {
        use std::io::{Read, Write};
        let mut text = String::new();
        std::net::TcpStream::connect(("127.0.0.1", port))
            .and_then(|mut s| {
                s.write_all(b"hello\n")
                    .and_then(|()| s.read_to_string(&mut text))
            })
            .map(|_| text)
            .ok()
    };
    // The proxy's forwarder may take a moment after the harness listens.
    let gateway = link_test_kit::wait::within(
        "the served port to answer",
        std::time::Duration::from_secs(10),
        || answer(served).filter(|t| t == "gateway"),
    )
    .ok();
    let daemon = answer(inside);
    let listened = files.join("listening").exists();
    std::fs::write(files.join("done"), "").unwrap();
    let _ = agent.wait();
    assert!(listened, "the harness never listened");
    assert_eq!(gateway.as_deref(), Some("gateway"));
    assert_eq!(
        daemon, None,
        "its daemon's port is open to every local user"
    );
}

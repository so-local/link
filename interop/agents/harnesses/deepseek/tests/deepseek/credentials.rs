//! The credentials DeepSeek Harness is handed and keeps: none on a
//! command line this adapter builds, the addresses with the proxy's
//! secret only in a file of this user's, and what it keeps in an agent's
//! home listed by `lnk secret list` (`link-core` beside this folder, built
//! there). A shell script stands in for `node`.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use link_test_kit::TempDir;
use serde_json::{Value, json};

/// The sandbox proxy's secret, in the addresses Link hands it.
const SECRET: &str = "0123456789abcdef0123456789abcdef";
const ENDPOINT_KEY: &str = "the-endpoint-key-0123456789";
const MODEL_KEY: &str = "sk-the-model-key";

fn adapter() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lnk-deepseek"))
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn no_secret_is_on_a_command_line_it_builds() {
    let root = TempDir::new("deepseek-argv");
    let home = root.join("home");
    let files = root.join("files");
    std::fs::create_dir_all(&files).unwrap();
    let proxy = |path: &str| format!("http://127.0.0.1:40781/_link/url/{SECRET}/{path}");
    let settings = json!({
        "files": files.display().to_string(),
        "model": {
            "provider": "ollama",
            "name": "qwen3:8b",
            "api_key": MODEL_KEY,
            "base_url": proxy("local/11434"),
        },
        "tools": {"gh": proxy("mcp/gh")},
        "endpoint_key": ENDPOINT_KEY,
    });
    // What a write that stopped halfway could leave, readable by all: a
    // patch it was about to rename into place.
    let stale = home.join(".deepseek/link.patch.new");
    write(&stale, &proxy("mcp/old"));
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut configure = Command::new(adapter())
        .arg("configure")
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    configure
        .stdin
        .take()
        .unwrap()
        .write_all(settings.to_string().as_bytes())
        .unwrap();
    let out = configure.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The addresses are in its patch, this user's alone.
    let patch = home.join(".deepseek/link.patch.yml");
    assert!(std::fs::read_to_string(&patch).unwrap().contains(SECRET));
    let mode = std::fs::metadata(&patch).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0, "{mode:o}");
    assert!(!stale.exists(), "the stale patch stayed");
    assert!(!std::fs::read_to_string(&patch).unwrap().contains("mcp/old"));

    // `dsh`, standing in: records its command line and waits.
    let argv = root.join("argv.log");
    let node = link_files::layout::Hidden::DeepseekPrograms
        .in_home(&home)
        .join(format!("node-v{}/bin/node", crate::node_version()));
    std::fs::create_dir_all(node.parent().unwrap()).unwrap();
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nexec sleep 300\n",
        argv.display()
    );
    link_test_kit::write_program(&node, &script).unwrap();
    // Its port, which it binds itself: held until the test ends.
    let held = link_test_kit::HeldNumber::port();
    let port = held.port_number();
    let mut serve = Command::new(adapter())
        .arg("serve")
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", "/usr/bin:/bin")
        .env("LNK_FILES", &files)
        .env("LNK_PORT", port.to_string())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Its health check, asked once it listens, starts `dsh`.
    let mut asked = None;
    let recorded = link_test_kit::wait::eventually("dsh to start", Duration::from_secs(30), || {
        assert!(serve.try_wait().unwrap().is_none(), "serve ended");
        let recorded = std::fs::read_to_string(&argv).unwrap_or_default();
        if recorded.ends_with('\n') {
            return Some(recorded);
        }
        if asked.is_none()
            && let Ok(mut tcp) = std::net::TcpStream::connect(("127.0.0.1", port))
        {
            let _ = tcp.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n");
            asked = Some(tcp);
        }
        None
    });
    // SAFETY: a signal to the child this test started.
    unsafe { libc::kill(serve.id() as i32, libc::SIGTERM) };
    serve.wait().unwrap();
    assert!(recorded.contains("--patch"), "{recorded}");
    for secret in [SECRET, ENDPOINT_KEY, MODEL_KEY] {
        assert!(
            !recorded.contains(secret),
            "{secret} on a command line: {recorded}"
        );
    }
}

/// An agent `main` whose DeepSeek Harness keeps its endpoint's key in its
/// home.
fn an_agent_with_deepseek(home: &Path) {
    write(
        &home.join(".lnk/data/agents/main/harnesses/deepseek/.deepseek/link.env"),
        &format!("ENDPOINT_KEY=\"{ENDPOINT_KEY}\"\n"),
    );
}

fn listed(held: &[Value], what: &str, of: &str) -> Value {
    held.iter()
        .find(|c| c["what"] == what && c["of"].as_str().unwrap_or("").contains(of))
        .unwrap_or_else(|| panic!("no {what} of {of} in {held:?}"))
        .clone()
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn it_says_what_it_keeps_in_each_agent_s_home() {
    let home = TempDir::new("deepseek-parts");
    an_agent_with_deepseek(&home);
    let out = Command::new(adapter())
        .args(["uninstall", "--parts", "--json"])
        .env_clear()
        .env("HOME", &*home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let kept: Value = serde_json::from_str(&text).unwrap();
    let held = kept["parts"][0]["credentials"].as_array().unwrap();
    let key = listed(held, "An agent's key", "main's endpoint");
    assert!(
        key["kept"]
            .as_str()
            .unwrap()
            .ends_with("harnesses/deepseek/.deepseek/link.env (only yours)"),
        "{key}"
    );
    assert!(!text.contains(ENDPOINT_KEY), "{text}");
    assert_eq!(kept["parts"][0]["paths"], json!([]));
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn lnk_secret_list_shows_what_it_keeps() {
    let home = TempDir::new("deepseek-secret-list");
    an_agent_with_deepseek(&home);
    // `lnk` asks the modules beside it: its own, and this adapter.
    let core = link_test_kit::programs_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for program in ["lnk", "lnk-secret"] {
        std::fs::copy(core.join(program), bin.join(program)).unwrap();
    }
    std::fs::copy(adapter(), bin.join("lnk-deepseek")).unwrap();
    let out = Command::new(bin.join("lnk"))
        .args(["secret", "list", "--json"])
        .env_clear()
        .env("HOME", &*home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(&home))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let listed_here: Value = serde_json::from_str(&text).unwrap();
    listed(
        listed_here["credentials"].as_array().unwrap(),
        "An agent's key",
        "main's endpoint",
    );
    assert!(!text.contains(ENDPOINT_KEY), "{text}");
}

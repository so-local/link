//! The credentials Hermes is handed and keeps: none on a command line
//! this adapter builds, and what it keeps in an agent's home listed by
//! `lnk secret list` (`link-core` beside this folder, built there).

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use link_test_kit::TempDir;
use serde_json::{Value, json};

/// The sandbox proxy's secret, in the addresses Link hands it.
const SECRET: &str = "0123456789abcdef0123456789abcdef";
const BOT_TOKEN: &str = "123:the-telegram-bot-token";
const API_KEY: &str = "the-api-server-key-0123456789";

fn adapter() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lnk-hermes"))
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// `lnk-hermes <args>` in home `home`, `stdin` on its standard input.
fn run(home: &Path, args: &[&str], stdin: &str) -> std::process::Output {
    use std::io::Write;
    let mut child = link_test_kit::env::command(adapter(), home)
        .args(args)
        .env("HOME", home)
        .env("LNK_AGENT_ID", "aaaaaaaaaaaa")
        .env("LNK_OWNER", "me@box")
        .env("PATH", "/usr/bin:/bin")
        .env("HERMES_RUNTIME_DIR", home.join("planted-tools"))
        .env("HERMES_INSTALL_ROOT", home.join("planted-stamp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn configure(home: &Path, settings: &Value) {
    let out = run(home, &["configure"], &settings.to_string());
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A sealed Hermes payload in `home`'s programs, its launcher and its ACP
/// entrypoint the script `launcher`; the payload's folder.
fn sealed_hermes(home: &Path, launcher: &str) -> PathBuf {
    let programs = link_files::layout::Hidden::HermesPrograms.in_home(home);
    for name in ["hermes", "hermes-acp"] {
        let program = programs.join("hermes-agent/.hermes/bin").join(name);
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        link_test_kit::write_program(&program, launcher).unwrap();
    }
    let venv = "installs/0000000000000000/environments/selected";
    write(&programs.join(venv).join("pyvenv.cfg"), "version = 3.11\n");
    write(
        &programs.join("manifest.json"),
        &json!({"repo":"hermes-agent", "venv":venv, "store":"tools"}).to_string(),
    );
    write(&programs.join("tools/python"), "fixture interpreter");
    std::fs::create_dir_all(programs.join("pm-runtime/lib/python3.11/site-packages")).unwrap();
    write(
        &programs.join("pm-runtime/pm-runtime.json"),
        r#"{"python":"../tools/python","sitePackages":"lib/python3.11/site-packages"}"#,
    );
    programs
}

/// Every file under `root`, with its contents, in order.
fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                files.push((path.clone(), std::fs::read(&path).unwrap()));
            }
        }
    }
    files.sort();
    files
}

fn env_file(home: &Path) -> String {
    std::fs::read_to_string(home.join(".hermes/.env")).unwrap()
}

#[test]
fn no_secret_is_on_a_command_line_it_builds() {
    let home = TempDir::new("hermes-argv");
    // Hermes, as one that records each command line it's run with.
    let programs = sealed_hermes(
        &home,
        "#!/bin/sh\n[ \"$HERMES_HOME\" = \"$HOME/.hermes\" ] || exit 9\n[ \"$HERMES_DISABLE_LAZY_INSTALLS\" = 1 ] || exit 9\n[ -z \"$HERMES_RUNTIME_DIR$HERMES_INSTALL_ROOT\" ] || exit 9\nprintf '%s\\n' \"$*\" >> \"$HOME/argv.log\"\n",
    );
    let sealed = tree(&programs);
    let proxy = |path: &str| format!("http://127.0.0.1:40781/_link/url/{SECRET}/{path}");
    configure(
        &home,
        &json!({
            "files": home.join("Link").display().to_string(),
            "model": {"provider": "ollama", "name": "qwen3:8b", "base_url": proxy("local/11434")},
            "tools": {"gh": proxy("mcp/gh")},
            "memory": proxy("mcp/memory-main"),
            "telegram": {
                "bot_token": BOT_TOKEN,
                "allowed_users": [1],
                "api_root": "http://127.0.0.1:40781/_link/secret/telegram",
            },
        }),
    );
    let argv = std::fs::read_to_string(home.join("argv.log")).unwrap();
    assert!(argv.contains("config set model.base_url"), "{argv}");
    // Its catalogue fixes were applied when the payload was sealed:
    // configuring writes nothing into the read-only programs.
    assert_eq!(tree(&programs), sealed);
    for secret in [SECRET, BOT_TOKEN] {
        assert!(!argv.contains(secret), "{secret} on a command line: {argv}");
    }
    // The address is in its `.env`, which its config names.
    assert!(argv.contains("${LNK_HERMES_MODEL_URL}"), "{argv}");
    let model = proxy("local/11434/v1");
    assert!(
        env_file(&home).contains(&format!("LNK_HERMES_MODEL_URL=\"{model}\"")),
        "{}",
        env_file(&home)
    );
    // The tools' addresses are in its config, this user's alone.
    let config = home.join(".hermes/config.yaml");
    assert!(std::fs::read_to_string(&config).unwrap().contains(SECRET));
    let mode = std::fs::metadata(&config).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0, "{mode:o}");

    // A model at its provider's own address: no address, set or kept.
    configure(
        &home,
        &json!({
            "files": home.join("Link").display().to_string(),
            "model": {"provider": "anthropic", "api_key": "sk-ant-api03-the-model-key"},
        }),
    );
    let argv = std::fs::read_to_string(home.join("argv.log")).unwrap();
    assert!(argv.contains("config unset model.base_url"), "{argv}");
    assert!(
        !argv.lines().any(|line| line.starts_with("pm ")),
        "configuration tried to install packages: {argv}"
    );
    assert!(!argv.contains("sk-ant-api03"), "{argv}");
    assert!(
        !env_file(&home).contains("LNK_HERMES_MODEL_URL"),
        "{}",
        env_file(&home)
    );
}

/// An agent `main` whose Hermes keeps its endpoint's key in its home,
/// beside channel settings in its `.env`.
fn an_agent_with_hermes(home: &Path) {
    let hermes = home.join(".lnk/data/agents/main/harnesses/hermes/.hermes");
    write(
        &hermes.join("link.env"),
        &format!("ENDPOINT_KEY=\"{API_KEY}\"\n"),
    );
    write(
        &hermes.join(".env"),
        &format!(
            "TELEGRAM_BOT_TOKEN=\"{BOT_TOKEN}\"\n\
             SIGNAL_ACCOUNT=\"+15550100\"\nSIGNAL_HTTP_URL=\"http://127.0.0.1:8644/{SECRET}\"\n"
        ),
    );
}

fn listed(held: &[Value], what: &str, of: &str) -> Value {
    held.iter()
        .find(|c| c["what"] == what && c["of"].as_str().unwrap_or("").contains(of))
        .unwrap_or_else(|| panic!("no {what} of {of} in {held:?}"))
        .clone()
}

#[test]
fn it_says_what_hermes_keeps_in_each_agent_s_home() {
    let home = TempDir::new("hermes-parts");
    an_agent_with_hermes(&home);
    let out = run(&home, &["uninstall", "--parts", "--json"], "");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let kept: Value = serde_json::from_str(&text).unwrap();
    let held = kept["parts"][0]["credentials"].as_array().unwrap();
    let token = listed(held, "A harness's token", "main's Hermes endpoint");
    assert!(
        token["kept"]
            .as_str()
            .unwrap()
            .ends_with("harnesses/hermes/.hermes/link.env (only yours)"),
        "{token}"
    );
    for value in [API_KEY, BOT_TOKEN, SECRET] {
        assert!(!text.contains(value), "{value} in {text}");
    }
    // Nothing of its own: it keeps none outside the agents' folders.
    assert_eq!(kept["parts"][0]["paths"], json!([]));
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn lnk_secret_list_shows_what_hermes_keeps() {
    let home = TempDir::new("hermes-secret-list");
    an_agent_with_hermes(&home);
    // `lnk` asks the modules beside it: its own, and this adapter.
    let core = link_test_kit::programs_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for program in ["lnk", "lnk-secret"] {
        std::fs::copy(core.join(program), bin.join(program)).unwrap();
    }
    std::fs::copy(adapter(), bin.join("lnk-hermes")).unwrap();
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
    let held = listed_here["credentials"].as_array().unwrap();
    listed(held, "A harness's token", "main's Hermes endpoint");
    for value in [API_KEY, BOT_TOKEN, SECRET] {
        assert!(!text.contains(value), "{value} in {text}");
    }
}

#[test]
fn rotating_tool_endpoints_refreshes_native_files_without_structural_cli_calls() {
    let home = TempDir::new("hermes-tool-rotation");
    sealed_hermes(
        &home,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/argv.log\"\n",
    );
    let mut settings = json!({
        "files": home.join("files"),
        "model": {"provider": "ollama", "name": "qwen3:8b", "base_url": "http://127.0.0.1:11434"},
        "tools": {"kit": "http://127.0.0.1:18001/first-tool-secret"},
        "memory": "http://127.0.0.1:18001/first-memory-secret"
    });
    configure(&home, &settings);
    let first = std::fs::read(home.join("argv.log")).unwrap();
    assert!(!first.is_empty());
    settings["tools"]["kit"] = "http://127.0.0.1:18002/rotated-tool-secret".into();
    settings["memory"] = "http://127.0.0.1:18002/rotated-memory-secret".into();
    configure(&home, &settings);
    assert_eq!(
        std::fs::read(home.join("argv.log")).unwrap(),
        first,
        "rotating endpoints reran structural configuration"
    );
    let path = home.join(".hermes/config.yaml");
    let native = std::fs::read_to_string(&path).unwrap();
    assert!(native.contains("rotated-tool-secret") && native.contains("rotated-memory-secret"));
    assert!(!native.contains("first-tool-secret") && !native.contains("first-memory-secret"));
    settings["tools"] = json!({});
    settings["memory"] = Value::Null;
    configure(&home, &settings);
    assert_eq!(std::fs::read(home.join("argv.log")).unwrap(), first);
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("rotated-tool-secret")
    );
    settings["model"]["name"] = "another-model".into();
    configure(&home, &settings);
    assert_ne!(
        std::fs::read(home.join("argv.log")).unwrap(),
        first,
        "model changes must still configure the runtime"
    );
}

//! Link Memory reached the way a harness reaches it, for real, on Linux:
//! `lnk sandbox tool add memory -- lnk-memory-tool serve <folder>`, its
//! three calls allowed, and a proxied sandbox calling it with curl at
//! `$LNK_TOOL_MEMORY`. The server runs outside and reads the folder; the
//! sandbox gets only its answers.
//!
//! It runs `lnk-sandbox`, built in `link-core` beside this folder
//! (`LNK_CORE` for another place); a module runs another, never links
//! it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CLIENT: &str = r#"
u=$LNK_TOOL_MEMORY
post() { curl -s --noproxy '*' -D - -H 'Content-Type: application/json' "$@" "$u"; }
id=$(post -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' | tr -d '\r' | sed -n 's/^Mcp-Session-Id: //p')
rpc() { post -H "Mcp-Session-Id: $id" -d "$1" | tail -n 1; }
call() { rpc '{"jsonrpc":"2.0","id":'$1',"method":"tools/call","params":{"name":"'$2'","arguments":'$3'}}'; }
printf 'TOOLS %s\n' "$(rpc '{"jsonrpc":"2.0","id":2,"method":"tools/list"}')"
# A call never waits for the first walk, and says so while it hasn't
# finished: searched again until it has, for at most ten seconds.
n=0
while found=$(call 3 search '{"query":"tomatoes"}'); case $found in *walking*) [ $n -lt 100 ];; *) false;; esac; do
  n=$((n + 1)); sleep 0.1
done
printf 'SEARCH %s\n' "$found"
printf 'READ %s\n' "$(call 4 read '{"path":"memory/notes/garden.md","from":3,"lines":1}')"
printf 'LIST %s\n' "$(call 5 list '{"kind":"conversation"}')"
"#;

/// `lnk-sandbox`, from `link-core` beside this folder, built there with
/// all its programs.
fn lnk_sandbox() -> PathBuf {
    let core = std::env::var_os("LNK_CORE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    link_test_kit::programs_in(&core.join("../modules/sandbox")).join("lnk-sandbox")
}

/// `lnk-sandbox` with `args`, HOME `root`, no desktop.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn lnk(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(lnk_sandbox());
    command
        .args(args)
        .current_dir(root)
        .env("HOME", root)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null());
    command
}

fn ran(command: &mut Command) -> (bool, String) {
    let output = command.output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    (output.status.success(), text)
}

/// The JSON after `tag` on its line of `text`.
fn line(text: &str, tag: &str) -> serde_json::Value {
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix(tag))
        .unwrap_or_else(|| panic!("no {tag} in {text}"));
    serde_json::from_str(line.trim()).unwrap_or_else(|_| panic!("{tag}: {line}\n{text}"))
}

#[test]
fn a_sandboxed_agent_searches_memory_through_the_tool_bridge() {
    let program = lnk_sandbox();
    link_test_kit::programs::require(&program, "link-core beside this folder builds it");
    if !link_test_kit::gate::sandbox_with_landlock()
        || !link_test_kit::gate::program("curl", &["--version"])
    {
        return;
    }
    let root = link_test_kit::TempDir::new("sandbox");
    let folder = root.join("memory");
    std::fs::create_dir_all(folder.join("memory/notes")).unwrap();
    std::fs::create_dir_all(folder.join("chats/telegram-42")).unwrap();
    std::fs::write(
        folder.join("memory/notes/garden.md"),
        "# The garden\n\nPlant the tomatoes in May.\n",
    )
    .unwrap();
    std::fs::write(
        folder.join("chats/telegram-42/conversation.md"),
        "**You:** When do the tomatoes go in?\n\n**Agent:** In May.\n",
    )
    .unwrap();

    let server = env!("CARGO_BIN_EXE_lnk-memory-tool");
    let folder_arg = folder.display().to_string();
    let (ok, text) = ran(lnk(
        &root,
        &[
            "tool",
            "add",
            "memory",
            "--add",
            "search",
            "--add",
            "read",
            "--add",
            "list",
            "--",
            server,
            "serve",
            &folder_arg,
        ],
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped()));
    assert!(ok, "{text}");

    let (ok, text) = ran(lnk(
        &root,
        &[
            "run",
            "--commands.toolchains",
            "shell",
            "--internet.hosts",
            "*",
            "--tools.names",
            "memory",
            "--",
            "sh",
            "-c",
            CLIENT,
        ],
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped()));
    assert!(ok, "{text}");

    let tools = line(&text, "TOOLS");
    assert_eq!(
        tools["result"]["tools"].as_array().unwrap().len(),
        4,
        "{text}"
    );
    let search = line(&text, "SEARCH");
    let results = search["result"]["structuredContent"]["results"]
        .as_array()
        .unwrap_or_else(|| panic!("{text}"));
    let mut kinds: Vec<&str> = results
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    kinds.sort();
    assert_eq!(kinds, ["conversation", "file"], "{text}");
    let read = line(&text, "READ");
    assert_eq!(
        crate::document(&read)["text"],
        "Plant the tomatoes in May.",
        "{text}"
    );
    let list = line(&text, "LIST");
    assert_eq!(list["result"]["structuredContent"]["total"], 1, "{text}");
}

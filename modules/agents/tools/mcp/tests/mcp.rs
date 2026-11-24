//! The real stdio server uses the CLI's output and refuses calls before execution.

link_test_kit::program!(system);

use link_test_kit::{TempDir, env, link_program, process::ChildGroup, wait, write_program};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::Path,
    process::Stdio,
    time::Duration,
};

fn fixture() -> TempDir {
    let dir = TempDir::new("link-mcp-protocol");
    link_program(
        Path::new(env!("CARGO_BIN_EXE_lnk-mcp")),
        &dir.join("lnk-mcp"),
    )
    .unwrap();
    let declaration = r#"schema = 1

[actions."example.example.show"]
description = "Show an example"
role = "secondary"
surfaces = ["cli", "tool"]
route = { bridge = "command", callers = ["agent"], limits = { bytes = 4096, time_ms = 2000 } }

[actions."example.example.show".args]
json = { x-link-cli = { shared = "json" } }
agent = { type = "string", description = "The agent it shows" }

[actions."example.example.slow"]
description = "Wait until cancelled"
role = "secondary"
surfaces = ["cli", "tool"]
route = { bridge = "command", callers = ["agent"], limits = { bytes = 4096, time_ms = 600000 } }

[actions."example.example.stop"]
description = "Stop an example"
role = "secondary"
surfaces = ["cli", "tool"]
route = { bridge = "command", callers = ["agent"], limits = { bytes = 4096, time_ms = 2000 } }
"#;
    let timeout = wait::scaled(Duration::from_secs(2)).as_millis();
    let actions = link_component::actions::parse(
        &declaration.replace("time_ms = 2000", &format!("time_ms = {timeout}")),
    )
    .unwrap();
    let registry = json!({"example":{
        "protocol":1,"component":{"id":"example","cargo_name":"link-example","program":"lnk-example","kind":"module","version":"1.0.0","license":"MIT","description":"A fixture"},
        "commands":link_component::actions::commands(&actions)
    }});
    write_program(&dir.join("lnk-example"), "#!/bin/sh\nexit 0\n").unwrap();
    write_program(
        &dir.join("lnk"),
        format!(
            r#"#!/bin/sh
case "$1" in
--link-registry) cat <<'REGISTRY'
{registry}
REGISTRY
;;
example)
  case "$2" in
    show) test "$3" = '--agent=main' && test "$4" = '--json' || exit 4
          echo '{{"name":"main","enabled":["memory"]}}' ;;
    slow) /bin/sleep 600 & touch "$HOME/started"; wait ;;
    stop) echo 'stopped' ;;
    *) exit 7 ;;
  esac ;;
*) exit 9 ;;
esac
"#
        ),
    )
    .unwrap();
    dir
}

fn command(dir: &Path) -> std::process::Command {
    let mut command = env::command(dir.join("lnk-mcp"), dir);
    command
        .args(["--agent", "main"])
        .env("LNK_EXE", dir.join("lnk"))
        .env("PATH", format!("{}:/usr/bin:/bin", dir.display()));
    command
}

#[test]
fn stdio_discovery_and_calls_preserve_cli_outputs_and_only_reads_bypass_approval() {
    let dir = fixture();
    let reads = command(&dir).arg("--read-calls").output().unwrap();
    assert!(
        reads.status.success(),
        "{}",
        String::from_utf8_lossy(&reads.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&reads.stdout).unwrap(),
        json!(["example__show"])
    );
    let mut command = command(&dir);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ChildGroup::spawn(&mut command).unwrap();
    let mut input = child.take_stdin().unwrap();
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"example__show","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"example__show","arguments":{"agent":"other"}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"example__stop","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"login","arguments":{}}}),
    ];
    for message in messages {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    assert!(
        wait::eventually(
            "MCP server exits after stdin closes",
            Duration::from_secs(10),
            || child.try_status().unwrap()
        )
        .success()
    );
    let mut text = String::new();
    child
        .take_stdout()
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let mut replies: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    replies.sort_by_key(|reply| reply["id"].as_u64());
    assert_eq!(replies.len(), 5);
    let tools = replies[0]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);
    let shown = tools
        .iter()
        .find(|tool| tool["name"] == "example__show")
        .unwrap();
    assert!(shown["inputSchema"]["properties"].get("agent").is_none());
    let direct = env::command(dir.join("lnk"), &dir)
        .args(["example", "show", "--agent=main", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        replies[1]["result"]["content"][0]["text"],
        String::from_utf8(direct.stdout).unwrap()
    );
    assert_eq!(replies[1]["result"]["structuredContent"]["name"], "main");
    assert_eq!(replies[2]["error"]["code"], -32602);
    assert_eq!(replies[3]["result"]["content"][0]["text"], "stopped\n");
    assert_eq!(replies[4]["error"]["code"], -32602);
}

#[test]
fn cancellation_stops_the_command_and_discovery_stays_responsive() {
    if !cfg!(target_os = "linux") {
        link_test_kit::gate::lacks(
            "Linux process inspection",
            "this cleanup proof requires /proc",
            None,
        );
        return;
    }
    let dir = fixture();
    let mut command = command(&dir);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ChildGroup::spawn(&mut command).unwrap();
    let mut input = child.take_stdin().unwrap();
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"example__slow"}})
    )
    .unwrap();
    wait::appears(
        "the command starts before cancellation",
        &dir.join("started"),
        Duration::from_secs(10),
    );
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}})
    )
    .unwrap();
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})
    )
    .unwrap();
    drop(input);
    assert!(
        wait::eventually(
            "cancelled server drains and exits",
            Duration::from_secs(10),
            || child.try_status().unwrap()
        )
        .success()
    );
    let mut output = String::new();
    child
        .take_stdout()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert!(
        output
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .any(|reply| reply["id"] == 2 && reply["result"]["tools"].is_array())
    );
    // Only Linux shows each process's folder to look under.
    #[cfg(target_os = "linux")]
    wait::eventually(
        "cancelled command leaves no live descendants",
        Duration::from_secs(10),
        || link_test_kit::process::left_under(&dir).is_empty(),
    );
}

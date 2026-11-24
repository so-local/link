//! A connection whose module installs a program into the home of a harness
//! speaking it (`describe`'s `cli`, as `lnk-signal` does `signal-cli`):
//! `lnk agent start` runs its `cli install` and `cli link` in the
//! harness's sandbox, and hands the adapter the program's path. Fakes
//! here, as scripts.

use std::path::Path;
use std::process::Command;

use link_test_kit::TempDir;

use crate::common;

/// An adapter speaking Signal, keeping what `configure` is handed.
const ADAPTER: &str = r#"#!/bin/sh
case "$1" in
  configure) cat > "$HOME/settings.json" ;;
  command) printf '{"argv":["/bin/true"],"ports":[]}' ;;
  connections) echo '["signal"]' ;;
  link) echo "$*" > "$HOME/adapter-linked"; exit 10 ;;
esac
exit 0
"#;

/// A Signal module installing its program where it runs, and trying to
/// write outside the harness's home. HOME is that home,
/// ~/.lnk/data/agents/main/harnesses/fake: the real home is six up.
const SIGNAL: &str = r#"#!/bin/sh
case "$1 $2" in
  "describe --json") echo '{"title": "Signal", "linked": true, "bridged": true, "cli": true}' ;;
  "cli install")
    cat > "$HOME/signal-asked.json"
    mkdir -p "$HOME/.signal-cli" && touch "$HOME/.signal-cli/signal-cli"
    touch "$HOME/../../../../../../escaped" 2>/dev/null
    printf '{"program": "%s/.signal-cli/signal-cli"}' "$HOME" ;;
  "cli link") echo "$*" > "$HOME/signal-linked"; exit 10 ;;
  *) exit 1 ;;
esac
"#;

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_connections_program_is_installed_and_linked_in_the_harnesss_sandbox() {
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    let root = TempDir::new("connection_cli");
    let (home, bin) = (root.join("home"), root.join("bin"));
    std::fs::create_dir_all(home.join("Link/Safe/Agents/main")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    crate::common::write_agent(
        &home,
        "main",
        &format!(
            "harness = \"fake\"\n{}",
            crate::common::connection(
                "signal",
                &[],
                "account = \"+15557654321\"\nallowed_users = [\"+15551234567\"]",
                "id = \"+15557654321\""
            )
        ),
    );
    crate::common::write_adapter(&bin.join("lnk-fake"), ADAPTER).unwrap();
    link_test_kit::write_program(&bin.join("lnk-signal"), SIGNAL).unwrap();
    // lnk-agent finds modules beside itself first: a copy in bin, so
    // the fake lnk-signal is the one it runs.
    let harness = bin.join("lnk-agent");
    link_test_kit::copy_program(Path::new(env!("CARGO_BIN_EXE_lnk-agent")), &harness).unwrap();
    let core = common::core_with_sandbox(&bin);
    std::os::unix::fs::symlink(common::sandbox_module(), bin.join("lnk-sandbox")).unwrap();
    let files = home.join("Link/Agents/main/home");
    let output = Command::new(&harness)
        .args(["start", "--foreground", "--run.files"])
        .arg(&files)
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_EXE", &core)
        .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let boxed = home.join(".lnk/data/agents/main/harnesses/fake");
    // Installed in the home, told the connection's settings, and kept in it.
    let asked: serde_json::Value =
        serde_json::from_str(&read(&boxed.join("signal-asked.json"))).unwrap();
    assert_eq!(asked["account"], "+15557654321");
    assert!(boxed.join(".signal-cli/signal-cli").is_file());
    assert!(!home.join("escaped").exists(), "it wrote outside the home");
    // The adapter was handed the program's path.
    let settings: serde_json::Value =
        serde_json::from_str(&read(&boxed.join("settings.json"))).unwrap();
    assert_eq!(
        settings["signal"]["program"],
        boxed.join(".signal-cli/signal-cli").display().to_string()
    );
    // Linked by its module, not the adapter: with no terminal it isn't.
    let home_arg = boxed.display().to_string();
    assert_eq!(
        read(&boxed.join("signal-linked")),
        format!("cli link --home {home_arg}\n")
    );
    assert!(!boxed.join("adapter-linked").exists());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Signal isn't linked on fake"), "{stderr}");
}

//! `lnk agent webhook create` refuses an agent whose harness can't say who a
//! message is from, or can't keep a conversation's tools: a webhook's
//! post is a stranger's, which the one would read as its owner's, and the
//! other give every tool. Fakes here, as scripts.

use std::path::Path;
use std::process::Command;

use link_test_kit::TempDir;

/// An adapter answering on an endpoint, whose contract says `from` and
/// `tools`.
fn adapter(from: bool, tools: bool) -> String {
    format!(
        "#!/bin/sh\ncase \"$1\" in\n  contract) echo '{{\"version\": 1, \"from\": {from}, \"tools\": {tools}}}' ;;\n  \
         endpoint) echo \"\\\"http://127.0.0.1:$LNK_PORT/v1/chat/completions\\\"\" ;;\n  \
         *) exit 1 ;;\nesac\n"
    )
}

/// `lnk agent webhook create deploy` for an agent on harness `harness`: its
/// output, and the agent's settings after.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn add(home: &Path, bin: &Path, harness: &str) -> (std::process::Output, String) {
    let agents = home.join("Link/Safe/Agents");
    for agent in ["main", "moltbot-2"] {
        std::fs::create_dir_all(agents.join(agent)).unwrap();
    }
    crate::common::write_agent(
        home,
        "main",
        &format!("harness = \"{harness}\"\nport = 30100\n"),
    );
    let output = Command::new(bin.join("lnk-agent"))
        .args(["webhook", "create", "deploy"])
        .env_clear()
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(home))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(home))
        .output()
        .unwrap();
    let settings = std::fs::read_to_string(agents.join("main/agent.toml")).unwrap();
    (output, settings)
}

#[test]
fn a_harness_that_cant_say_who_or_keep_tools_gets_no_webhook() {
    let root = TempDir::new("webhook-add");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for (name, from, tools) in [
        ("lnk-mute", false, true),
        ("lnk-open", true, false),
        ("lnk-tells", true, true),
    ] {
        link_test_kit::write_program(&bin.join(name), adapter(from, tools)).unwrap();
    }
    // The core, on its PATH: it answers only the release's history.
    link_test_kit::write_core(&bin.join("lnk"), "#!/bin/sh\nexit 1\n").unwrap();
    // Beside the fakes, so they're the adapters it finds.
    link_test_kit::copy_program(
        Path::new(env!("CARGO_BIN_EXE_lnk-agent")),
        &bin.join("lnk-agent"),
    )
    .unwrap();

    let (output, settings) = add(&root.join("mute"), &bin, "mute");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(said.contains("can't say who a message is from"), "{said}");
    assert!(!settings.contains("webhook"), "{settings}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Secret"));

    // One that says who, but can't keep a conversation's tools: refused.
    let (output, settings) = add(&root.join("open"), &bin, "open");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(said.contains("can't keep a conversation's tools"), "{said}");
    assert!(!settings.contains("webhook"), "{settings}");

    // The same, on a harness that can both: added.
    let (output, settings) = add(&root.join("tells"), &bin, "tells");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("Secret:"), "{said}");
    assert!(settings.contains("[webhooks.deploy]"), "{settings}");
}

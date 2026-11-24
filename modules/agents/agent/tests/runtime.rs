//! Status reads the sandbox owner's current flags, without a child query.

use link_test_kit::{TempDir, env};
use serde_json::Value;

#[test]
fn status_observes_wire_changes_without_starting_the_sandbox_command() {
    let root = TempDir::new("status-wire");
    let home = root.join("home");
    crate::common::own_machine(&home);
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let agent = bin.join("lnk-agent");
    let source = std::env::var_os("LNK_TEST_AGENT_PROGRAM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_lnk-agent").into());
    link_test_kit::copy_program(&source, &agent).unwrap();
    // Its presence permits observation. Executing it would fail the test.
    link_test_kit::write_program(&bin.join("lnk-sandbox"), "#!/bin/sh\nexit 92\n").unwrap();
    let core = bin.join("lnk");
    link_test_kit::write_core(
        &core,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$(dirname "$0")/asked"
exit 93
"#,
    )
    .unwrap();
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"mini\"\n");
    let status = || {
        let output = env::command(&agent, &home)
            .args(["show", "--json"])
            .env("LNK_EXE", &core)
            .env("LNK_SERVICES", "off")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let status = serde_json::from_slice::<Value>(&output.stdout).unwrap();
        link_test_kit::ids::assert_named(&status);
        status
    };
    assert_eq!(status()["wire"], Value::Null);
    let pause = home.join(".lnk/safe/sandboxes/paused/main-mini");
    link_files::write_private(&pause, b"by the regression check\n").unwrap();
    assert!(
        status()["wire"]
            .as_str()
            .unwrap()
            .contains("regression check")
    );
    std::fs::remove_file(&pause).unwrap();
    assert_eq!(status()["wire"], Value::Null);
    let stop = home.join(".lnk/safe/sandboxes/stopped");
    link_files::write_private(&stop, b"stop").unwrap();
    assert!(
        status()["wire"]
            .as_str()
            .unwrap()
            .contains("every wire is stopped")
    );
    std::fs::remove_file(&stop).unwrap();
    assert_eq!(status()["wire"], Value::Null);
    let calls = std::fs::read_to_string(bin.join("asked")).unwrap_or_default();
    assert!(!calls.lines().any(|c| c.starts_with("sandbox ")), "{calls}");
}

/// Status uses no secret, so it never runs Apple's `security`.
#[test]
fn status_runs_no_security_program() {
    let root = TempDir::new("status-keychain");
    let home = root.join("home");
    let calls = root.join("calls.log");
    let security = root.join("logged-security");
    link_test_kit::write_program(
        &security,
        format!("#!/bin/sh\necho \"$1\" >> {calls:?}\nexit 1\n"),
    )
    .unwrap();
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"mini\"\n");
    let output = env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
        .args(["show", "--json"])
        .env("LNK_SECURITY", &security)
        .env("LNK_SERVICES", "off")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(&calls).unwrap_or_default(), "");
}

/// The global `-a` together with a positional name is refused.
#[test]
fn the_agent_flag_and_a_positional_name_are_refused_together() {
    let root = TempDir::new("agent-flag-and-name");
    let home = root.join("home");
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"mini\"\n");
    let refused = |args: &[&str], says: &str| {
        let output = env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
            .args(args)
            .env("LNK_SERVICES", "off")
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?} was accepted");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(says), "{args:?}: {stderr}");
    };
    refused(&["-a", "main", "show", "main"], "once");
    refused(
        &["-a", "main", "delete", "main", "--yes"],
        "isn't one of this command's arguments",
    );
}

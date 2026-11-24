//! Settings name only what they hold: a table Link no longer has is
//! refused, not left out and dropped by the next save.

use link_test_kit::{TempDir, env};

#[test]
fn settings_with_a_channels_table_are_refused() {
    let root = TempDir::new("settings-unknown");
    let home = root.join("home");
    std::fs::create_dir_all(home.join("Link/Safe/Agents/main")).unwrap();
    std::fs::write(
        home.join("Link/Safe/Agents/main/agent.toml"),
        "harness = \"fake\"\n[channels.telegram]\nsettings = \"\"\n",
    )
    .unwrap();
    let output = env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
        .args(["show", "--agent", "main"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let said = String::from_utf8_lossy(&output.stderr).to_string()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(said.contains("channels"), "{said}");
}

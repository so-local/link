//! An agent's services in the service manager: none left behind.

use super::*;

/// A delete stops an agent's services, removes their files, and has
/// systemd forget them, so a service that failed before isn't left on
/// its list as "not-found failed".
#[test]
fn a_deleted_agents_services_leave_the_failed_list() {
    let world = World::new("forget");
    link_test_kit::write_program(&world.bin.join("systemctl"), SYSTEMCTL).unwrap();
    let files = world.agent("work").join("home").display().to_string();
    ok(&world.lnk(&[
        "create",
        "work",
        "--harness",
        "fake",
        "--no-sandbox",
        "--run.files",
        &files,
    ]));
    let id = id_of(&world, "work");
    let log = world.bin.join("systemctl.log");
    std::fs::write(&log, "").unwrap();
    ok(&world.lnk(&["delete", "work", "--yes"]));
    let calls = read(&log);
    for unit in [
        format!("lnk-agent-{id}.service"),
        format!("lnk-bridge-{id}.service"),
    ] {
        let forgot = format!("--user reset-failed {unit}");
        assert!(calls.lines().any(|l| l == forgot), "{calls}");
    }
}

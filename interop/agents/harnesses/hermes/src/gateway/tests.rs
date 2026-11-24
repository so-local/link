use super::*;
use link_test_kit::{ChildGroup, TempDir};

fn fixture(mode: &'static str, other: Option<u32>) -> (TempDir, Gateway) {
    let home = TempDir::new("hermes-gateway-owner");
    let program = home.join("gateway.py");
    link_test_kit::write_program(&program, include_str!("fixture.py")).unwrap();
    let directory = home.to_path_buf();
    let gateway = Gateway::new(
        {
            let directory = directory.clone();
            move || {
                let mut command = link_test_kit::env::command(&program, &directory);
                command
                    .env("HERMES_HOME", &directory)
                    .env("LNK_TEST_GATEWAY_MODE", mode);
                if let Some(pid) = other {
                    command.env("LNK_TEST_OTHER_PID", pid.to_string());
                }
                Ok(command)
            }
        },
        directory,
        Duration::from_secs(30),
    );
    (home, gateway)
}

#[test]
fn native_gateway_is_ready_owned_and_closes_cleanly_without_stopping_other_processes() {
    if !link_test_kit::gate::program("python3", &["--version"])
        || !link_test_kit::gate::program("sh", &["-c", "command -v sleep"])
    {
        return;
    }
    let unrelated_home = TempDir::new("hermes-unrelated-gateway");
    let other = ChildGroup::spawn(link_test_kit::env::command("sleep", &unrelated_home).arg("300"))
        .unwrap();
    let (home, gateway) = fixture("ready", None);
    gateway.ensure().unwrap();
    let first = std::fs::read(home.join("started")).unwrap();
    gateway.ensure().unwrap();
    assert_eq!(std::fs::read(home.join("started")).unwrap(), first);
    let stopped = gateway.stop();
    assert!(
        stopped.is_ok(),
        "{stopped:?}; native receipt: {:?}",
        std::fs::read_to_string(home.join("closed"))
    );
    assert_eq!(
        std::fs::read_to_string(home.join("closed")).unwrap(),
        "native cleanup completed"
    );
    assert!(other.try_status().unwrap().is_none());
    assert!(
        gateway.ensure().is_err(),
        "stopped ownership restarted a gateway"
    );
    gateway.stop().unwrap();
}

#[test]
fn stopping_during_startup_interrupts_readiness_and_refuses_foreign_readiness() {
    if !link_test_kit::gate::program("python3", &["--version"]) {
        return;
    }
    for mode in ["starting", "foreign"] {
        // This process's PID is certainly not the fixture child's identity.
        let (home, gateway) = fixture(mode, Some(std::process::id()));
        let gateway = std::sync::Arc::new(gateway);
        let starting = std::thread::spawn({
            let gateway = gateway.clone();
            move || gateway.ensure()
        });
        link_test_kit::eventually(
            "the native gateway blocked its stop signals before readiness",
            Duration::from_secs(30),
            || {
                std::fs::read(home.join("started"))
                    .ok()
                    .and_then(|text| serde_json::from_slice::<serde_json::Value>(&text).ok())
            },
        );
        gateway.stop().unwrap();
        assert!(
            starting.join().unwrap().is_err(),
            "accepted foreign or incomplete readiness"
        );
        assert!(
            home.join("closed").is_file(),
            "startup stop bypassed native cleanup"
        );
    }
}

#[test]
fn native_gateway_startup_failure_stays_a_failure_and_is_reaped() {
    if !link_test_kit::gate::program("python3", &["--version"]) {
        return;
    }
    let (home, gateway) = fixture("fail", None);
    assert!(gateway.ensure().is_err());
    assert!(gateway.stop().is_err(), "native exit code was hidden");
    assert!(!home.join("closed").exists());
    gateway.stop().unwrap();
}

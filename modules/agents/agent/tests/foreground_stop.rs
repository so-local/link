//! Ctrl-C keeps the foreground log reader alive until its child closes.
#[path = "../src/stops.rs"]
mod stops;

use link_test_kit::{ChildGroup, TempDir};
use std::{path::Path, sync::atomic::Ordering::SeqCst, time::Duration};

const ROLE: &str = "LNK_TEST_FOREGROUND_STOP_ROLE";
const ROOT: &str = "LNK_TEST_FOREGROUND_STOP_ROOT";
const CASE: &str = "LNK_TEST_FOREGROUND_STOP_CASE";
const TEST: &str =
    "foreground_stop::ctrl_c_drains_output_and_honors_a_stop_before_the_child_starts";

/// This test again as its `role`, its output in `<home>/<role>.log`.
fn fixture(home: &Path, role: &str) -> std::process::Command {
    let mut command =
        link_test_kit::process::rerun(TEST, home, &home.join(format!("{role}.log"))).unwrap();
    command.env(ROLE, role);
    command
}

fn supervisor(home: &Path, before_child: bool) {
    let stopped = stops::catch();
    if before_child {
        std::fs::write(home.join("ready"), "caught").unwrap();
        link_test_kit::eventually("the startup stop delivered", Duration::from_secs(5), || {
            home.join("go").is_file()
        });
    }
    let mut command = link_test_kit::env::command("python3", home);
    // The native harness, stopped twice at once: Ctrl-C reaches its
    // process group, and the supervisor passes a stop on. It closes on
    // the first and ignores the second, which would otherwise end it by
    // its default action while the interpreter shuts down.
    if before_child {
        command.args(["-c", "import time; time.sleep(60)"]);
    } else {
        command.args(["-c", "import pathlib,signal,sys\nclosing=False\ndef stop(*args):\n global closing\n if closing:return\n closing=True\n signal.signal(signal.SIGINT,signal.SIG_IGN)\n signal.signal(signal.SIGTERM,signal.SIG_IGN)\n print('native closed cleanly',flush=True)\n pathlib.Path(sys.argv[2]).write_text('closed')\n raise SystemExit(0)\nsignal.signal(signal.SIGINT,stop)\nsignal.signal(signal.SIGTERM,stop)\npathlib.Path(sys.argv[1]).touch()\nwhile True:signal.pause()"])
            .arg(home.join("native-ready")).arg(home.join("closed"));
    }
    let log = link_agent::service::Log {
        file: std::fs::File::create(home.join("harness.log")).unwrap(),
        agent: None,
    };
    let mut child = link_agent::service::spawn_logged(&mut command, &log).unwrap();
    stops::pass_to(Some(child.id()));
    if !before_child {
        link_test_kit::eventually(
            "the native stop handler installed",
            Duration::from_secs(5),
            || home.join("native-ready").is_file(),
        );
        std::fs::write(home.join("ready"), "running").unwrap();
    }
    let status = child.wait().unwrap();
    link_agent::service::copied(Duration::from_secs(5));
    assert!(stopped.load(SeqCst));
    if !before_child {
        assert!(status.success(), "native shutdown failed: {status}");
        assert_eq!(
            std::fs::read_to_string(home.join("closed")).unwrap(),
            "closed"
        );
        assert!(
            std::fs::read_to_string(home.join("harness.log"))
                .unwrap()
                .contains("native closed cleanly")
        );
    }
}

#[test]
fn ctrl_c_drains_output_and_honors_a_stop_before_the_child_starts() {
    if !link_test_kit::gate::program("python3", &["--version"]) {
        return;
    }
    if std::env::var(ROLE).as_deref() == Ok("supervisor") {
        supervisor(
            Path::new(&std::env::var_os(ROOT).unwrap()),
            std::env::var(CASE).as_deref() == Ok("before-child"),
        );
        return;
    }
    let home = TempDir::new("foreground-stop-owner");
    if std::env::var(ROLE).as_deref() != Ok("owner") {
        let process = ChildGroup::spawn(&mut fixture(&home, "owner")).unwrap();
        let status =
            link_test_kit::eventually("the dedicated stop owner", Duration::from_secs(30), || {
                process.try_status().unwrap()
            });
        assert!(
            status.success(),
            "stop owner failed: {status}\n{}",
            link_test_kit::process::rerun_said(&home.join("owner.log"))
        );
        return;
    }
    let adopter = link_test_kit::process::Subreaper::for_fixture().unwrap();
    for before_child in [false, true] {
        let case_home = TempDir::new("foreground-stop-case");
        let mut command = fixture(&case_home, "supervisor");
        command.env(ROOT, case_home.path()).env(
            CASE,
            if before_child {
                "before-child"
            } else {
                "active"
            },
        );
        let process = ChildGroup::spawn(&mut command).unwrap();
        link_test_kit::eventually("the foreground owner ready", Duration::from_secs(5), || {
            case_home.join("ready").is_file()
        });
        // SAFETY: this guard owns the group's unreaped leader and every
        // fixture descendant; no other test belongs to this group.
        assert_eq!(
            unsafe { libc::kill(-(process.id() as i32), libc::SIGINT) },
            0
        );
        if before_child {
            std::fs::write(case_home.join("go"), "start").unwrap();
        }
        let status = link_test_kit::eventually(
            "the owner reaped its child",
            Duration::from_secs(10),
            || process.try_status().unwrap(),
        );
        drop(process);
        link_test_kit::eventually(
            "the owned descendants reaped",
            Duration::from_secs(5),
            || adopter.reaped().unwrap(),
        );
        assert!(
            status.success(),
            "foreground stop failed: {status}\n{}",
            link_test_kit::process::rerun_said(&case_home.join("supervisor.log"))
        );
    }
}

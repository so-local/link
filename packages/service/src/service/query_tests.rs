use super::*;
use std::{io::Read, time::Duration};

#[test]
#[ignore = "private subprocess entry for native service queries"]
fn entry() {
    let services: Vec<(String, String)> = vec![(
        "link.query-fixture".into(),
        "lnk-query-fixture.service".into(),
    )];
    assert!(running(&services[0].0, &services[0].1));
    assert_eq!(running_each(&services), [true]);
    let expected = Status {
        running: true,
        pid: Some(17),
        runs: Some(1),
    };
    assert_eq!(status(&services[0].0, &services[0].1), expected);
    assert_eq!(status_each(&services), [expected]);
}

#[test]
fn native_service_queries_find_the_user_bus_without_a_login_environment() {
    query(None);
}

#[test]
fn native_service_queries_preserve_an_explicit_user_bus_location() {
    query(Some("/owned-runtime"));
}

fn query(runtime: Option<&str>) {
    let home = link_test_kit::TempDir::new("native-service-query");
    let bin = home.join("bin");
    std::fs::create_dir(&bin).unwrap();
    link_test_kit::write_program(
        &bin.join("systemctl"),
        r#"#!/bin/sh
[ "$XDG_RUNTIME_DIR" = "$LNK_TEST_EXPECTED_RUNTIME" ] || exit 1
[ "$1" = --user ] || exit 1
shift
case "$1" in
  is-active)
    shift
    [ "$1" != --quiet ] || exit 0
    for unit do printf 'active\n'; done
    ;;
  show)
    shift
    [ "$1" = -p ] || exit 1
    shift 2
    for unit do
      printf 'Id=%s\nActiveState=active\nMainPID=17\nNRestarts=0\n\n' "$unit"
    done
    ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    // SAFETY: geteuid has no preconditions.
    let default = format!("/run/user/{}", unsafe { libc::geteuid() });
    let mut command = link_test_kit::env::command(std::env::current_exe().unwrap(), &home);
    command
        .args([
            "--exact",
            "service::query_tests::entry",
            "--ignored",
            "--nocapture",
        ])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_SERVICES", "1")
        .env("LNK_TEST_EXPECTED_RUNTIME", runtime.unwrap_or(&default))
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(runtime) = runtime {
        command.env("XDG_RUNTIME_DIR", runtime);
    } else {
        command.env_remove("XDG_RUNTIME_DIR");
    }
    let mut child = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
    let mut status = None;
    link_test_kit::eventually("native service query", Duration::from_secs(10), || {
        status = child.try_status().unwrap();
        status.is_some()
    });
    let mut output = String::new();
    child
        .take_stdout()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    child
        .take_stderr()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert!(status.unwrap().success(), "{output}");
}

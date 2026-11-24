use super::*;
use std::{io::Read, os::unix::ffi::OsStringExt, time::Duration};

#[test]
#[ignore = "private subprocess entry for native unit registration"]
fn entry() {
    let home = link_files::home_dir();
    let job = Job {
        argv: vec!["/bin/true".into()],
        env: Default::default(),
        workdir: home.clone(),
        log: home.join("job.log"),
    };
    start_systemd(&[(
        "link.registration-fixture",
        "lnk-registration-fixture.service",
        &job,
    )])
    .unwrap();
}

#[test]
fn native_registration_finds_unit_files_outside_the_managers_search_path() {
    let home = link_test_kit::TempDir::new("native-unit-registration");
    let bin = home.join("bin");
    std::fs::create_dir(&bin).unwrap();
    link_test_kit::write_program(
        &bin.join("systemctl"),
        r#"#!/bin/sh
[ "$1" = --user ] || exit 1
shift
case "$1" in
  enable)
    shift
    [ "$1" = --no-reload ] || exit 1
    shift
    for unit do
      case "$unit" in /*) ;; *) echo "Unit is outside the manager's search path" >&2; exit 1 ;; esac
      [ -s "$unit" ] || exit 1
    done
    ;;
  daemon-reload|restart) ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    let config = home.join(std::ffi::OsString::from_vec(
        b"outside manager-\xff".to_vec(),
    ));
    let mut command = link_test_kit::env::command(std::env::current_exe().unwrap(), &home);
    command
        .args([
            "--exact",
            "service::native_tests::entry",
            "--ignored",
            "--nocapture",
        ])
        .env("PATH", &bin)
        .env("XDG_CONFIG_HOME", &config)
        .env("LNK_SERVICES", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
    let mut status = None;
    link_test_kit::eventually("native unit registration", Duration::from_secs(10), || {
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
    assert!(
        config
            .join("systemd/user/lnk-registration-fixture.service")
            .is_file()
    );
}

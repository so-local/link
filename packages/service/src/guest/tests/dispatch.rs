//! Public dispatch runs in an owned child, with no mutation of the test runner.
use crate::*;
use std::{io::Read, os::unix::fs::PermissionsExt, time::Duration};

const SERVICES: [(&str, &str); 3] = [
    ("link.disabled.first", "lnk-disabled-first.service"),
    ("link.disabled.second", "lnk-disabled-second.service"),
    ("link.disabled.single", "lnk-disabled-single.service"),
];

#[test]
#[ignore = "private subprocess entry for the public service dispatch regressions"]
fn entry() {
    let home = link_files::home_dir();
    let job = Job {
        argv: vec!["/bin/true".into()],
        env: Default::default(),
        workdir: home.clone(),
        log: home.join("logs/job.log"),
    };
    inactive();
    match std::env::var("LNK_TEST_SERVICE_API").unwrap().as_str() {
        "start" => {
            start_all(&[
                (SERVICES[0].0, SERVICES[0].1, &job),
                (SERVICES[1].0, SERVICES[1].1, &job),
            ])
            .unwrap();
            start(SERVICES[2].0, SERVICES[2].1, &job).unwrap();
            for (_, unit) in SERVICES {
                assert!(
                    unit_path_of(unit).is_file(),
                    "disabled install lost its file"
                );
            }
            lifecycle();
        }
        "lifecycle" => {
            inert_files();
            lifecycle();
        }
        "enabled-refusal" => {
            assert!(start_all(&[(SERVICES[0].0, SERVICES[0].1, &job)]).is_err());
            assert!(start(SERVICES[1].0, SERVICES[1].1, &job).is_err());
            for (_, unit) in SERVICES {
                assert!(!unit_path_of(unit).exists());
            }
            inert_files();
            for (label, unit) in SERVICES {
                assert!(pause(label, unit).is_err());
                assert!(resume(label, unit).is_err());
                assert!(stop_before_start(label, unit).is_err());
                assert!(stop(label, unit).is_err());
                assert_eq!(
                    std::fs::read(unit_path_of(unit)).unwrap(),
                    b"inert definition"
                );
            }
        }
        _ => panic!("unknown public service fixture"),
    }
    inactive();
    assert!(
        !link_files::layout::Hidden::Services.path().exists(),
        "public dispatch created guest supervisor state"
    );
}

fn inert_files() {
    for (_, unit) in SERVICES {
        let path = unit_path_of(unit);
        link_files::create_private_dir(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"inert definition").unwrap();
    }
}

fn inactive() {
    for (label, unit) in SERVICES {
        assert!(!running(label, unit));
        assert_eq!(status(label, unit), Status::default());
    }
    let services: Vec<_> = SERVICES
        .iter()
        .map(|(label, unit)| (label.to_string(), unit.to_string()))
        .collect();
    assert_eq!(running_each(&services), vec![false; services.len()]);
    assert_eq!(
        status_each(&services),
        vec![Status::default(); services.len()]
    );
}

fn lifecycle() {
    for (label, unit) in SERVICES {
        let path = unit_path_of(unit);
        let before = std::fs::read(&path).unwrap();
        pause(label, unit).unwrap();
        resume(label, unit).unwrap();
        unload_other(unit);
        load_other(&path);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    keep_after_logout("the disabled fixture");
    stop_before_start(SERVICES[0].0, SERVICES[0].1).unwrap();
    stop(SERVICES[1].0, SERVICES[1].1).unwrap();
    stop_from_within(&unit_path_of(SERVICES[2].1));
    reload();
    for (_, unit) in SERVICES {
        assert!(
            !unit_path_of(unit).exists(),
            "disabled removal kept its file"
        );
    }
}

fn run(role: &str, mode: &str) {
    let home = link_test_kit::TempDir::new("service-disabled-dispatch");
    let marker = home.join("untrusted-service.toml");
    std::fs::write(&marker, "version = 999\nmode = \"unknown\"\n").unwrap();
    std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o666)).unwrap();
    let mut command = link_test_kit::env::command(std::env::current_exe().unwrap(), &home);
    command
        .args([
            "--exact",
            "guest::tests::dispatch::entry",
            "--ignored",
            "--nocapture",
        ])
        .current_dir(&home)
        .env("LNK_SERVICES", mode)
        .env("LNK_TEST_SERVICE_API", role)
        .env("LNK_TEST_SERVICE_SETTINGS", &marker)
        .env("LNK_INSTALLATION_DIR", home.join(".lnk"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
    let mut status = None;
    link_test_kit::eventually(
        "the public service dispatch fixture",
        Duration::from_secs(10),
        || {
            status = child.try_status().unwrap();
            status.is_some()
        },
    );
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
    assert!(status.unwrap().success(), "{role}, {mode}: {output}");
    assert_eq!(
        std::fs::read(marker).unwrap(),
        b"version = 999\nmode = \"unknown\"\n"
    );
}

#[test]
fn disabled_public_start_and_status_ignore_the_guest_marker() {
    for mode in ["off", "0"] {
        run("start", mode);
    }
}

#[test]
fn disabled_public_lifecycle_preserves_inert_file_semantics() {
    for mode in ["off", "0"] {
        run("lifecycle", mode);
    }
}

#[test]
fn enabled_public_lifecycle_keeps_untrusted_marker_refusals() {
    run("enabled-refusal", "1");
}

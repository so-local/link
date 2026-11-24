use super::*;
mod dispatch;
use std::io::Read;
use std::{
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::PermissionsExt,
    },
    sync::atomic::{AtomicUsize, Ordering},
};
const UNIT: &str = "lnk-fixture.service";

#[test]
#[ignore = "subprocess entry used by the guest lifecycle tests"]
fn entry() {
    let root = std::env::var_os("LNK_TEST_GUEST_ROOT").expect("fixture root");
    let driver = Driver {
        root: root.into(),
        fixture: true,
        after_owner_observed: None,
    };
    let args = [
        "LNK_TEST_GUEST_ROLE",
        "LNK_TEST_GUEST_UNIT",
        "LNK_TEST_GUEST_EPOCH",
    ]
    .map(|name| std::env::var(name).expect("fixture entry argument"));
    use clap::Parser;
    #[derive(clap::Parser)]
    struct Entry {
        #[command(flatten)]
        given: GuestEntry,
    }
    let entry =
        Entry::try_parse_from(std::iter::once("guest-service".to_string()).chain(args)).unwrap();
    driver.entry(entry.given).unwrap();
}
struct Fixture {
    home: link_test_kit::TempDir,
    driver: Driver,
    guardians: std::sync::Mutex<Vec<OwnedFd>>,
}
impl Fixture {
    fn new() -> Self {
        let home = link_test_kit::TempDir::new("guest-service");
        let driver = Driver {
            root: home.join(".lnk"),
            fixture: true,
            after_owner_observed: None,
        };
        Self {
            home,
            driver,
            guardians: std::sync::Mutex::new(Vec::new()),
        }
    }
    fn store(&self) -> Store {
        Store::new(&self.driver.root, UNIT).unwrap()
    }
    fn job(&self, script: &str) -> Job {
        let command = link_test_kit::env::command("sh", &self.home);
        Job {
            argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
            env: command
                .get_envs()
                .filter_map(|(key, value)| {
                    value.map(|value| {
                        (
                            key.to_string_lossy().into_owned(),
                            value.to_string_lossy().into_owned(),
                        )
                    })
                })
                .collect(),
            workdir: self.home.to_path_buf(),
            log: self.home.join("logs/service.log"),
        }
    }
    fn start(&self, script: &str) -> Record {
        self.driver.start(UNIT, &self.job(script), true).unwrap();
        let record = self.store().read().unwrap().unwrap();
        self.guardians
            .lock()
            .unwrap()
            .push(pidfd(record.guardian.as_ref().unwrap()));
        record
    }
    fn stop(&self) {
        stop(&self.driver, UNIT, true).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if stop(&self.driver, UNIT, true).is_err() {
            for fd in self.guardians.lock().unwrap().iter() {
                signal(fd, libc::SIGKILL);
            }
        }
    }
}
fn pidfd(identity: &Identity) -> OwnedFd {
    // SAFETY: pidfd_open creates a descriptor for the current process identity.
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, identity.pid, 0) } as i32;
    assert!(
        raw >= 0,
        "pidfd fixture prerequisite: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: the new descriptor has exactly one owner.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    assert_eq!(Identity::of(identity.pid).unwrap(), *identity);
    fd
}
fn signal(fd: &OwnedFd, signal: i32) {
    // SAFETY: the live pidfd, never its numeric PID, is this fixture's signal target.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    assert!(result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH));
}
fn gone(identity: &Identity) {
    link_test_kit::eventually(
        "the complete owned guest worker group to disappear",
        Duration::from_secs(10),
        || identity.group_empty().unwrap(),
    );
}
#[test]
fn explicit_marker_is_closed_bounded_and_owned_with_no_link_or_mode_fallback() {
    let home =
        link_test_kit::TempDir::under(Path::new(env!("CARGO_MANIFEST_DIR")), "guest-settings");
    let path = home.join("service.toml");
    let uid = unsafe { libc::geteuid() };
    // This container maps root-owned ancestors to nobody. The owner injection
    // is private to this test; production always requires the real root UID.
    use std::os::unix::fs::MetadataExt;
    let owners = [0, uid, std::fs::metadata("/").unwrap().uid()];
    assert!(!configured(&path, &owners).unwrap());
    std::fs::write(&path, "schema = 1\nmode = \"guest\"\n").unwrap();
    assert!(configured(&path, &owners).unwrap());
    if uid != 0 {
        assert!(
            configured(&path, &[0]).is_err(),
            "test path must not relax production root ownership"
        );
    }
    for text in [
        "schema=2\nmode=\"guest\"",
        "schema=1\nmode=\"native\"",
        "schema=1\nmode=\"guest\"\nextra=true",
        "schema=\"1\"\nmode=\"guest\"",
    ] {
        std::fs::write(&path, text).unwrap();
        assert!(configured(&path, &owners).is_err(), "accepted {text}");
    }
    std::fs::write(&path, "x".repeat(4097)).unwrap();
    assert!(configured(&path, &owners).is_err());
    std::fs::write(&path, "schema=1\nmode=\"guest\"").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(configured(&path, &owners).is_err());
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(home.join("missing"), &path).unwrap();
    assert!(
        configured(&path, &owners).is_err(),
        "dangling marker must not select native"
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(configured(&path, &owners).is_err());
}
#[test]
fn start_pause_resume_replace_and_stop_report_actual_main_and_remove_owned_groups() {
    let fixture = Fixture::new();
    let first = fixture.start("printf '\\001binary'; sleep 300 & wait");
    assert_eq!(
        fixture.driver.status(UNIT).pid,
        first.main.as_ref().map(|id| id.pid)
    );
    assert!(fixture.driver.status(UNIT).running);
    pause(&fixture.driver, UNIT).unwrap();
    gone(first.worker.as_ref().unwrap());
    assert!(!fixture.driver.status(UNIT).running);
    assert!(
        fixture
            .store()
            .read()
            .unwrap()
            .unwrap()
            .guardian_live()
            .unwrap(),
        "pause stopped the parent-death guardian"
    );
    resume(&fixture.driver, UNIT).unwrap();
    let second = fixture.store().read().unwrap().unwrap();
    assert_ne!(first.worker, second.worker);
    assert!(fixture.driver.status(UNIT).running);
    fixture.start("sleep 300 & wait");
    gone(second.worker.as_ref().unwrap());
    let third = fixture.store().read().unwrap().unwrap();
    fixture.stop();
    gone(third.worker.as_ref().unwrap());
    assert!(!fixture.driver.status(UNIT).running);
}
#[test]
fn guardian_loss_cleans_a_running_group_and_restart_and_paused_loss_are_recoverable() {
    let fixture = Fixture::new();
    let first = fixture.start("sleep 300 & wait");
    let guardian = pidfd(first.guardian.as_ref().unwrap());
    signal(&guardian, libc::SIGKILL);
    gone(first.worker.as_ref().unwrap());
    assert!(!fixture.driver.status(UNIT).running);
    fixture.stop();
    assert!(fixture.store().read().unwrap().unwrap().worker.is_none());
    let second = fixture.start("sleep 300 & wait");
    pause(&fixture.driver, UNIT).unwrap();
    gone(second.worker.as_ref().unwrap());
    signal(&pidfd(second.guardian.as_ref().unwrap()), libc::SIGKILL);
    resume(&fixture.driver, UNIT).unwrap();
    let third = fixture.store().read().unwrap().unwrap();
    fixture
        .guardians
        .lock()
        .unwrap()
        .push(pidfd(third.guardian.as_ref().unwrap()));
    fixture.stop();
    gone(third.worker.as_ref().unwrap());
}
#[test]
fn worker_leader_exit_cleans_descendants_before_a_restart_and_stop_fences_late_work() {
    let fixture = Fixture::new();
    let first = fixture.start("sleep 300 & wait");
    signal(&pidfd(first.worker.as_ref().unwrap()), libc::SIGKILL);
    gone(first.worker.as_ref().unwrap());
    resume(&fixture.driver, UNIT).unwrap();
    let second = fixture.store().read().unwrap().unwrap();
    assert_ne!(first.worker, second.worker);
    assert_eq!(fixture.driver.status(UNIT).runs, Some(2));
    fixture.stop();
    gone(second.worker.as_ref().unwrap());
    assert_eq!(
        fixture.store().read().unwrap().unwrap().desired,
        Desired::Removed
    );
}
#[test]
fn natural_job_exit_restarts_after_backoff_and_guardian_loss_during_backoff_recovers() {
    let fixture = Fixture::new();
    let first = fixture.start("sleep 300 & wait");
    signal(&pidfd(first.main.as_ref().unwrap()), libc::SIGKILL);
    gone(first.worker.as_ref().unwrap());
    link_test_kit::eventually(
        "automatic service restart after its real crash backoff",
        Duration::from_secs(15),
        || fixture.driver.status(UNIT).running && fixture.driver.status(UNIT).runs == Some(2),
    );
    let second = fixture.store().read().unwrap().unwrap();
    assert_ne!(first.worker, second.worker);
    signal(&pidfd(second.main.as_ref().unwrap()), libc::SIGKILL);
    gone(second.worker.as_ref().unwrap());
    link_test_kit::eventually(
        "the guardian to acknowledge the dead worker before backoff",
        Duration::from_secs(5),
        || fixture.store().read().unwrap().unwrap().worker.is_none(),
    );
    signal(&pidfd(second.guardian.as_ref().unwrap()), libc::SIGKILL);
    let third = fixture.start("sleep 300 & wait");
    assert!(fixture.driver.status(UNIT).running);
    fixture.stop();
    gone(third.worker.as_ref().unwrap());
}
#[test]
fn reused_identity_and_uncertain_old_group_refuse_replacement_without_signalling_it() {
    let fixture = Fixture::new();
    let other_home = link_test_kit::TempDir::new("guest-service-unrelated");
    let other = link_test_kit::ChildGroup::spawn(
        link_test_kit::env::command("sleep", &other_home).arg("300"),
    )
    .unwrap();
    let first = fixture.start("sleep 300 & wait");
    fixture.stop();
    let mut wrong = Identity::of(std::process::id()).unwrap();
    wrong.ticks += 1;
    assert!(
        !wrong.live().unwrap(),
        "PID reuse must not match a stored owner"
    );
    let mut group = Identity::of(other.id()).unwrap();
    group.ticks += 1;
    fixture
        .store()
        .update(|record| {
            let record = record.as_mut().unwrap();
            record.guardian = Some(wrong);
            record.worker = Some(group);
            Ok(())
        })
        .unwrap();
    assert!(
        fixture
            .driver
            .start(UNIT, &fixture.job("sleep 300"), true)
            .unwrap_err()
            .to_string()
            .contains("uncertain")
    );
    assert_eq!(other.try_status().unwrap(), None);
    fixture
        .store()
        .update(|record| {
            let record = record.as_mut().unwrap();
            record.worker = None;
            record.guardian = None;
            Ok(())
        })
        .unwrap();
    gone(first.worker.as_ref().unwrap());
}
#[test]
fn concurrent_stop_supersedes_start_without_resurrecting_a_stopped_job() {
    let fixture = Fixture::new();
    fixture.start("sleep 300 & wait");
    let rendezvous = std::sync::Barrier::new(2);
    let attempted = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            rendezvous.wait();
            let _ = fixture
                .driver
                .start(UNIT, &fixture.job("sleep 300 & wait"), true);
            attempted.fetch_add(1, Ordering::Release);
        });
        rendezvous.wait();
        stop(&fixture.driver, UNIT, true).unwrap_or_else(|error| {
            assert!(error.to_string().contains("superseded"), "{error:#}");
        });
    });
    assert_eq!(attempted.load(Ordering::Acquire), 1);
    fixture.stop();
    let record = fixture.store().read().unwrap().unwrap();
    assert_eq!(record.desired, Desired::Removed);
    assert!(record.cleanup_confirmed().unwrap());
    assert!(!fixture.driver.status(UNIT).running);
}

#[test]
fn disabled_start_does_not_mutate_live_intent_or_launch_and_invalid_jobs_are_refused() {
    let fixture = Fixture::new();
    let first = fixture.start("sleep 300 & wait");
    fixture
        .driver
        .start(UNIT, &fixture.job("exit 1"), false)
        .unwrap();
    let after = fixture.store().read().unwrap().unwrap();
    assert_eq!(after.revision, first.revision);
    assert_eq!(after.worker, first.worker);
    assert!(fixture.driver.status(UNIT).running);
    for unit in ["../outside", "..", "has space", ""] {
        assert!(
            fixture
                .driver
                .start(unit, &fixture.job("sleep 300"), true)
                .is_err()
        );
    }
    let mut job = fixture.job("sleep 300");
    job.argv.clear();
    assert!(fixture.driver.start(UNIT, &job, true).is_err());
    job.argv = vec!["x".repeat(MAX_STATE as usize)];
    assert!(fixture.driver.start(UNIT, &job, true).is_err());
    assert_eq!(
        fixture.store().read().unwrap().unwrap().worker,
        first.worker
    );
    fixture.stop();
    gone(first.worker.as_ref().unwrap());
}
#[test]
fn failed_exec_reports_refusal_and_leaves_no_live_worker_group() {
    let fixture = Fixture::new();
    let mut job = fixture.job("sleep 300");
    job.argv = vec![fixture.home.join("absent-program").display().to_string()];
    let error = fixture.driver.start(UNIT, &job, true).unwrap_err();
    assert!(error.to_string().contains("failed to start"), "{error:#}");
    let record = fixture.store().read().unwrap().unwrap();
    if let Some(worker) = record.worker {
        gone(&worker);
    }
    assert!(!fixture.driver.status(UNIT).running);
    fixture.stop();
}
#[test]
fn binary_output_progresses_before_exit_and_large_output_stays_bounded() {
    let fixture = Fixture::new();
    let record = fixture.start("printf '\\001raw\\000bytes'; sleep 300 & wait");
    link_test_kit::eventually(
        "binary service output before job EOF",
        Duration::from_secs(10),
        || {
            std::fs::read(fixture.home.join("logs/service.log"))
                .is_ok_and(|bytes| bytes == b"\x01raw\x00bytes")
        },
    );
    fixture.stop();
    gone(record.worker.as_ref().unwrap());
    let path = fixture.home.join("logs/bounded.log");
    let file = crate::open_log(&path).unwrap();
    worker::pump(
        std::io::repeat(0x81).take(worker::MAX_LOG + 16384),
        std::sync::Arc::new(std::sync::Mutex::new(file)),
        &crate::start_path(&path),
    )
    .unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() <= worker::MAX_LOG);
    assert_eq!(std::fs::read(crate::start_path(&path)).unwrap(), b"0");
}

#[test]
fn guardian_death_after_live_observation_is_recovered_in_the_same_resume() {
    let fixture = Fixture::new();
    let first = fixture.start("sleep 300 & wait");
    pause(&fixture.driver, UNIT).unwrap();
    let identity = first.guardian.unwrap();
    let fd = pidfd(&identity);
    let called = AtomicUsize::new(0);
    let mut driver = fixture.driver.clone();
    driver.after_owner_observed = Some(std::sync::Arc::new(move || {
        if called.fetch_add(1, Ordering::AcqRel) == 0 {
            signal(&fd, libc::SIGKILL);
            link_test_kit::eventually(
                "guardian death immediately after live-owner observation",
                Duration::from_secs(5),
                || !identity.live().unwrap(),
            );
        }
    }));
    resume(&driver, UNIT).unwrap();
    let record = fixture.store().read().unwrap().unwrap();
    fixture
        .guardians
        .lock()
        .unwrap()
        .push(pidfd(record.guardian.as_ref().unwrap()));
    assert!(fixture.driver.status(UNIT).running);
    fixture.stop();
    gone(record.worker.as_ref().unwrap());
}

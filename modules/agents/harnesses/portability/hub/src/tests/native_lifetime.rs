//! The native ACP process must stop before resources captured by its factory.

use super::*;

#[tokio::test]
async fn native_process_is_reaped_before_its_launcher_resources_even_with_a_held_connection() {
    if !link_test_kit::gate::program("tail", &["--version"]) {
        return;
    }
    struct Resource {
        process: Arc<Mutex<Option<link_files::nofollow::Dir>>>,
        observed: Arc<Mutex<Option<bool>>>,
    }
    impl Resource {
        fn started(&self, pid: u32) -> std::io::Result<()> {
            // Hold this process's proc directory so a reused PID cannot satisfy the check.
            *self.process.lock().unwrap() = Some(link_files::nofollow::Dir::open_unlinked(
                &std::path::PathBuf::from(format!("/proc/{pid}")),
            )?);
            Ok(())
        }
    }
    impl Drop for Resource {
        fn drop(&mut self) {
            let gone = self
                .process
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|p| p.read(std::ffi::OsStr::new("stat"), 4096).is_err());
            *self.observed.lock().unwrap() = Some(gone);
        }
    }
    let dir = temp("hub-native-lifetime");
    let observed = Arc::new(Mutex::new(None));
    let resource = Resource {
        process: Arc::default(),
        observed: observed.clone(),
    };
    let native = crate::spawning(move || {
        let mut child = link_test_kit::env::command("tail", &dir);
        child.args(["-f", "/dev/null"]);
        child
    });
    let start: Start = Box::new(move || {
        let mut native = native()?;
        resource.started(native.child.as_mut().unwrap().0.child().id())?;
        let mut pipes = fake(Asked::default(), 1, false);
        // The process owner is real; the protocol stays the in-process fixture's.
        pipes.child = native.child;
        Ok(pipes)
    });
    let harness = harness::Harness::new(start, &Default::default());
    let up = harness.up_within(Duration::from_secs(5)).await.unwrap();
    drop(harness);
    assert_eq!(
        *observed.lock().unwrap(),
        None,
        "launcher resources ended while ACP was held"
    );
    drop(up);
    assert_eq!(
        *observed.lock().unwrap(),
        Some(true),
        "native process outlived its launcher resources"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn native_launcher_descendants_stop_even_when_the_launcher_has_exited() {
    const FIXTURE: &str = "LNK_TEST_ACP_DESCENDANTS";
    let dir = temp("hub-native-descendants");
    if std::env::var_os(FIXTURE).is_none() {
        let log = dir.join("fixture.log");
        let mut command = link_test_kit::process::rerun(
            "tests::native_lifetime::native_launcher_descendants_stop_even_when_the_launcher_has_exited",
            &dir,
            &log,
        )
        .unwrap();
        command.env(FIXTURE, "1");
        let process = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
        let status = link_test_kit::eventually(
            "the dedicated ACP descendant fixture",
            Duration::from_secs(15),
            || process.try_status().unwrap(),
        );
        assert!(
            status.success(),
            "ACP descendant fixture failed: {status}\n{}",
            link_test_kit::process::rerun_said(&log)
        );
        return;
    }
    // Adoption belongs only to this dedicated fixture; it cannot reap
    // a child owned by another concurrently running Hub test.
    let adopter = link_test_kit::process::Subreaper::for_fixture().unwrap();
    for finished_launcher in [false, true] {
        let dir = dir.to_path_buf();
        let start = crate::spawning(move || {
            let mut command = link_test_kit::env::command("sh", &dir);
            command.args([
                "-c",
                if finished_launcher {
                    "tail -f /dev/null & printf 'ready\\n'; exit 0"
                } else {
                    "tail -f /dev/null & printf 'ready\\n'; wait"
                },
            ]);
            command
        });
        let mut native = start().unwrap();
        let mut line = String::new();
        use tokio::io::AsyncBufReadExt;
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::io::BufReader::new(&mut native.read).read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(line, "ready\n");
        if finished_launcher {
            link_test_kit::eventually(
                "the native launcher's natural exit without reaping it",
                Duration::from_secs(5),
                || native.child.as_ref().unwrap().0.exited().unwrap(),
            );
        }
        drop(native);
        link_test_kit::eventually(
            "all ACP launcher descendants stopped and reaped",
            Duration::from_secs(5),
            || adopter.reaped().unwrap(),
        );
    }
}

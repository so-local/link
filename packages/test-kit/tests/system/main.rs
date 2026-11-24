//! The runner's own proof: a system test program whose tests each find
//! themselves alone in a process, and one test that runs this program
//! again to read what the runner says of a failure, a hang and a process
//! left behind.

link_test_kit::program!(system);

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Set by whichever of the two tests below runs first in a process. In one
/// process, the second would find it set.
static TAKEN: AtomicBool = AtomicBool::new(false);

#[test]
fn a_static_is_this_tests_alone() {
    assert!(
        !TAKEN.swap(true, Ordering::SeqCst),
        "another test ran in this process"
    );
}

#[test]
fn a_static_is_this_tests_alone_too() {
    assert!(
        !TAKEN.swap(true, Ordering::SeqCst),
        "another test ran in this process"
    );
}

/// The process runs this test only, named by the variable the runner set,
/// with a home and runtime folder made new for it.
#[test]
fn a_test_has_a_new_home_of_its_own() {
    assert_eq!(
        std::env::var(link_test_kit::program::ONE).unwrap(),
        "a_test_has_a_new_home_of_its_own"
    );
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    let mut held: Vec<_> = std::fs::read_dir(&home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    held.sort();
    assert_eq!(held, [".run"], "{}", home.display());
    assert_eq!(
        std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap()),
        home.join(".run")
    );
}

/// What the runner says of a failure, a hang and a process left behind,
/// read from this program run again with only those three fixtures.
#[test]
fn the_runner_ends_a_hang_and_says_each_failure() {
    let home = link_test_kit::TempDir::new("runner");
    let output = link_test_kit::env::command(std::env::current_exe().unwrap(), &home)
        .args(["fixture::", "--include-ignored"])
        .env(FIXTURE, "1")
        // The fixtures' deadline: 300 seconds times 0.01.
        .env(link_test_kit::wait::TIME_SCALE, "0.01")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(101), "{said}");
    for line in [
        "running 4 tests",
        "test fixture::fails ... FAILED",
        "test fixture::hangs ... FAILED",
        "test fixture::passes ... ok",
        "the fixture failed on purpose",
        "the runner ended fixture::hangs at its deadline of 3s",
        "test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured;",
    ] {
        assert!(said.contains(line), "no {line:?} in:\n{said}");
    }
    let left = "fixture::leaves_a_process left these running, ended by the runner";
    assert_eq!(said.contains(left), cfg!(target_os = "linux"), "{said}");
    assert_eq!(
        said.matches("test result: ").count(),
        1,
        "a test's own summary reached the run's output:\n{said}"
    );
}

/// Run only by [`the_runner_ends_a_hang_and_says_each_failure`].
const FIXTURE: &str = "LNK_TEST_KIT_RUNNER_FIXTURE";

mod fixture {
    use super::*;

    fn wanted() -> bool {
        std::env::var_os(FIXTURE).is_some()
    }

    #[test]
    #[ignore = "a fixture of the_runner_ends_a_hang_and_says_each_failure"]
    fn passes() {}

    #[test]
    #[ignore = "a fixture of the_runner_ends_a_hang_and_says_each_failure"]
    fn fails() {
        assert!(!wanted(), "the fixture failed on purpose");
    }

    #[test]
    #[ignore = "a fixture of the_runner_ends_a_hang_and_says_each_failure"]
    fn hangs() {
        if wanted() {
            #[expect(
                clippy::disallowed_methods,
                reason = "hangs on purpose, past the runner's deadline"
            )]
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    /// Starts a program in a session of its own, out of the test's process
    /// group, naming the test's home, and returns without stopping it.
    #[test]
    #[ignore = "a fixture of the_runner_ends_a_hang_and_says_each_failure"]
    fn leaves_a_process() {
        if !wanted() || !cfg!(target_os = "linux") {
            return;
        }
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        let script = home.join("daemon");
        link_test_kit::write_program(&script, "#!/bin/sh\nexec sleep 3600\n").unwrap();
        let child = link_test_kit::ChildGroup::spawn_session(&mut link_test_kit::env::command(
            &script, &home,
        ))
        .unwrap();
        std::mem::forget(child);
    }
}

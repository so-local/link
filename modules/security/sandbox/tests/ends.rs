//! Nothing inside a sandbox outlives its bubblewrap. Bubblewrap ended
//! before any stop reached its sandbox leaves its first process (the
//! sandbox's process 1), which ignores every stop from outside its
//! namespace but SIGKILL. The stage in front of bubblewrap, its
//! subreaper, ends the first process bubblewrap's report named once
//! bubblewrap has ended.
//!
//! Proved with a stand-in for bubblewrap that does just that: it reports
//! its child on `--info-fd`, as bubblewrap does, and ends, leaving the
//! child running in a session of its own, ignoring SIGTERM.

use std::time::Duration;

use link_test_kit::{ChildGroup, TempDir, wait};

/// Bubblewrap standing in: its first process forked, reported, left.
const LEAVING: &str = r#"#!/usr/bin/python3
import os, signal, sys, time
args = sys.argv[1:]
report = int(args[args.index("--info-fd") + 1])
ready, told = os.pipe()
child = os.fork()
if child == 0:
    os.setsid()
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    os.close(report)
    os.close(ready)
    # Holding nothing of the stage's: its output ends when it does.
    null = os.open(os.devnull, os.O_RDWR)
    for fd in (0, 1, 2):
        os.dup2(null, fd)
    os.write(told, b"y")
    os.close(told)
    while True:
        time.sleep(1)
os.close(told)
os.read(ready, 1)
with open(os.path.join(os.path.dirname(sys.argv[0]), "child"), "w") as f:
    f.write(str(child))
os.write(report, ('{"child-pid": %d}\n' % child).encode())
"#;

/// Whether process `pid` runs: one ended, reaped or not, doesn't.
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, rest)| !rest.trim_start().starts_with(['Z', 'X']))
    })
}

/// The stand-in's child, ended if the stage left it: a failing run leaves
/// nothing running.
struct Left(u32);

impl Drop for Left {
    fn drop(&mut self) {
        if alive(self.0) {
            // SAFETY: a signal to the stand-in's own child, still running.
            unsafe { libc::kill(self.0 as libc::pid_t, libc::SIGKILL) };
        }
    }
}

#[test]
fn what_bubblewrap_leaves_running_ends_with_its_stage() {
    if !link_test_kit::gate::program("python3", &["--version"]) {
        return;
    }
    let root = TempDir::new("ends");
    let (home, state) = (root.join("home"), root.join("state"));
    std::fs::create_dir_all(&home).unwrap();
    let bwrap = root.join("bwrap");
    link_test_kit::write_program(&bwrap, LEAVING).unwrap();
    let output = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-sandbox"), &home)
        .arg("placeholders")
        .arg("--dir")
        .arg(&state)
        .arg("--")
        .arg(&bwrap)
        .args(["--new-session", "--", "/bin/true"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let child: u32 = std::fs::read_to_string(root.join("child"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let _left = Left(child);
    wait::eventually(
        "the first process bubblewrap left to end with its stage",
        Duration::from_secs(10),
        || !alive(child),
    );
}

/// Bubblewrap standing in, with a first process that ignores a stop, as a
/// sandbox's process 1 does, and has started nothing yet: it reports it
/// and waits for it, as bubblewrap does.
const SETTING_UP: &str = r#"#!/usr/bin/python3
import os, signal, sys, time
args = sys.argv[1:]
report = int(args[args.index("--info-fd") + 1])
ready, told = os.pipe()
child = os.fork()
if child == 0:
    os.setsid()
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    os.close(report)
    os.close(ready)
    null = os.open(os.devnull, os.O_RDWR)
    for fd in (0, 1, 2):
        os.dup2(null, fd)
    os.write(told, b"y")
    os.close(told)
    while True:
        time.sleep(1)
os.close(told)
os.read(ready, 1)
with open(os.path.join(os.path.dirname(sys.argv[0]), "child"), "w") as f:
    f.write(str(child))
os.write(report, ('{"child-pid": %d}\n' % child).encode())
os.close(report)
os.waitpid(child, 0)
"#;

/// A stop that comes while the sandbox runs its process 1 alone still
/// ends it: sent to that process's group it would reach no one, as
/// process 1 ignores it, so it goes to bubblewrap, and the stage ends the
/// process 1 bubblewrap leaves.
#[test]
fn a_stop_while_the_sandbox_sets_up_ends_it() {
    if !link_test_kit::gate::program("python3", &["--version"]) {
        return;
    }
    let root = TempDir::new("setting-up");
    let (home, state) = (root.join("home"), root.join("state"));
    std::fs::create_dir_all(&home).unwrap();
    let bwrap = root.join("bwrap");
    link_test_kit::write_program(&bwrap, SETTING_UP).unwrap();
    let mut command = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-sandbox"), &home);
    command
        .arg("placeholders")
        .arg("--dir")
        .arg(&state)
        .arg("--")
        .arg(&bwrap)
        .args(["--new-session", "--", "/bin/true"]);
    let stage = ChildGroup::spawn(&mut command).unwrap();
    // Once the stage holds the first process its report named (a pidfd).
    wait::eventually(
        "the stage to read bubblewrap's report",
        Duration::from_secs(10),
        || {
            std::fs::read_dir(format!("/proc/{}/fd", stage.id()))
                .into_iter()
                .flatten()
                .flatten()
                .any(|fd| {
                    std::fs::read_link(fd.path())
                        .is_ok_and(|to| to.to_string_lossy() == "anon_inode:[pidfd]")
                })
        },
    );
    let child: u32 = std::fs::read_to_string(root.join("child"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let _left = Left(child);
    stage.terminate();
    wait::eventually("the stopped stage to end", Duration::from_secs(10), || {
        stage.try_status().unwrap()
    });
    wait::eventually(
        "its first process to end with it",
        Duration::from_secs(10),
        || !alive(child),
    );
}

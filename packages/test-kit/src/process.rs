//! Stopping what a test started.

#[cfg(windows)]
compile_error!(
    "link-test-kit: no Windows implementation of process::ChildGroup, process::kill_group or process::interrupt, which EO1 adds"
);

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
compile_error!("link-test-kit: no implementation of process::ChildGroup::try_status for this Unix");

/// A background child and its process group, stopped when their owner drops.
///
/// The leader stays unreaped while the guard owns it, so its process-group
/// number cannot be reused before cleanup. Pipes may be taken for fixture
/// handshakes and diagnostics; the child itself cannot escape the guard.
#[cfg(unix)]
pub struct ChildGroup {
    child: Option<std::process::Child>,
    id: u32,
}

#[cfg(unix)]
impl ChildGroup {
    /// Starts `command` in its own process group. Set its environment with
    /// [`crate::env::command`] or [`crate::env::clean`] before calling this.
    pub fn spawn(command: &mut std::process::Command) -> std::io::Result<Self> {
        use std::os::unix::process::CommandExt;
        let child = command.process_group(0).spawn()?;
        Ok(Self {
            id: child.id(),
            child: Some(child),
        })
    }

    /// Starts a fixture in its own session and group, without inheriting
    /// the test runner's controlling terminal. A configured `setsid`
    /// callback is also accepted; no process group is created before it.
    pub fn spawn_session(command: &mut std::process::Command) -> std::io::Result<Self> {
        use std::os::unix::process::CommandExt;
        // SAFETY: only async-signal-safe session getters and setsid run
        // in the child between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::getsid(0) != libc::getpid() && libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        Ok(Self {
            id: child.id(),
            child: Some(child),
        })
    }

    /// The leader's process number, also after it has been stopped.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Observes a natural exit without reaping the leader. Unlike a child's
    /// `try_wait`, this leaves the group's identity owned until cleanup.
    /// Calling it after [`Self::stop`] is an error.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn try_status(&self) -> std::io::Result<Option<std::process::ExitStatus>> {
        use std::os::unix::process::ExitStatusExt;
        let child = self.child.as_ref().ok_or_else(|| {
            std::io::Error::other("the test's process group has already been stopped")
        })?;
        // SAFETY: siginfo_t is a C record with no invalid all-zero fields.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        loop {
            // SAFETY: info points to initialized writable memory. WNOWAIT
            // observes this guard's child without releasing its process ID.
            let waited = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id() as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if waited == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        // SAFETY: waitid's exit observation initializes these fields.
        if unsafe { info.si_pid() } == 0 {
            return Ok(None);
        }
        let status = unsafe { info.si_status() };
        let raw = match info.si_code {
            libc::CLD_EXITED => status << 8,
            libc::CLD_KILLED => status,
            libc::CLD_DUMPED => status | 0x80,
            code => {
                return Err(std::io::Error::other(format!(
                    "unexpected process exit observation {code}"
                )));
            }
        };
        Ok(Some(std::process::ExitStatus::from_raw(raw)))
    }

    /// Takes the leader's piped stdin to release a fixture handshake.
    pub fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.child.as_mut()?.stdin.take()
    }

    /// Takes the leader's piped stdout for a readiness handshake.
    pub fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.child.as_mut()?.stdout.take()
    }

    /// Takes the leader's piped stderr for fixture diagnostics.
    pub fn take_stderr(&mut self) -> Option<std::process::ChildStderr> {
        self.child.as_mut()?.stderr.take()
    }

    /// Sends Ctrl-C to the still-owned leader without reaping it. Observe
    /// its exit with [`Self::try_status`] before final group cleanup.
    pub fn interrupt(&self) {
        if let Some(child) = &self.child {
            interrupt(child);
        }
    }

    /// Sends a stop (SIGTERM) to the still-owned leader alone, without
    /// reaping it, as a service manager or `kill` would. Observe its exit
    /// with [`Self::try_status`] before final group cleanup.
    pub fn terminate(&self) {
        if let Some(child) = &self.child {
            let id = child.id() as libc::pid_t;
            if id > 1 {
                // SAFETY: a plain signal to the one unreaped process this
                // guard owns.
                unsafe { libc::kill(id, libc::SIGTERM) };
            }
        }
    }

    /// Stops the group and reaps the direct child, once. Descendants that
    /// escaped into another group are outside this guard's ownership.
    /// Orphaned grandchildren are reaped by their adopter, not this guard.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            kill_group(&mut child);
        }
    }
}

#[cfg(unix)]
impl Drop for ChildGroup {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Kills `child`'s process group (a child started with `process_group(0)`)
/// and then `child`, and waits for it: whatever it started goes with it.
///
/// Never by running `kill -KILL -<pid>`: procps-ng's `kill` reads `-1234`
/// as a signal's option and sends to group `-1`, every process the user
/// may signal. As root in a virtual machine that's the machine's own
/// init, which then restarts.
#[cfg(unix)]
pub fn kill_group(child: &mut std::process::Child) {
    let id = child.id() as libc::pid_t;
    if id > 1 {
        // SAFETY: a plain kill to the group `child` leads.
        unsafe { libc::kill(-id, libc::SIGKILL) };
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Sends `child` alone what Ctrl-C sends (SIGINT), as a person at its
/// terminal would: never through the `kill` program ([`kill_group`]).
#[cfg(unix)]
pub fn interrupt(child: &std::process::Child) {
    let id = child.id() as libc::pid_t;
    if id > 1 {
        // SAFETY: a plain signal to the one process `child` is.
        unsafe { libc::kill(id, libc::SIGINT) };
    }
}

/// Every process still running that names `folder`, a test's own, in its
/// command line or its environment, but this one: each as `<pid> <its
/// command line>`. A stop or remove that leaves one behind left a process
/// outliving its owner. One ended but not yet reaped (a zombie) isn't
/// running. Only the processes this user may read are seen.
#[cfg(target_os = "linux")]
pub fn left_under(folder: &std::path::Path) -> Vec<String> {
    let needle = folder.as_os_str().as_encoded_bytes();
    let names = |bytes: &[u8]| {
        bytes.windows(needle.len()).enumerate().any(|(at, window)| {
            window == needle
                && bytes.get(at + needle.len()).is_none_or(|next| {
                    next.is_ascii_whitespace()
                        || matches!(next, b'/' | b'\0' | b'\'' | b'"' | b';' | b')' | b'}')
                })
        })
    };
    let own = std::process::id().to_string();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut left = Vec::new();
    for entry in entries.flatten() {
        let pid = entry.file_name().to_string_lossy().into_owned();
        if pid == own || !pid.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dir = entry.path();
        let stat = std::fs::read_to_string(dir.join("stat")).unwrap_or_default();
        // The state follows the command's name, in parentheses.
        let zombie = stat
            .rsplit_once(')')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with(['Z', 'X']));
        let cmdline = std::fs::read(dir.join("cmdline")).unwrap_or_default();
        let environ = std::fs::read(dir.join("environ")).unwrap_or_default();
        if !zombie && (names(&cmdline) || names(&environ)) {
            let command = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            left.push(format!("{pid} {}", command.trim_end()));
        }
    }
    left
}

/// Ends (SIGKILL) every process [`left_under`] `folder`, a test's own:
/// for a fixture's teardown, what the programs it ran started in a
/// session or group of their own (a daemon, a bridge), which outlive the
/// group it owns. Each as [`left_under`] said it, once signalled.
#[cfg(target_os = "linux")]
pub fn end_under(folder: &std::path::Path) -> Vec<String> {
    let left = left_under(folder);
    for line in &left {
        let pid = line
            .split_once(' ')
            .map_or(line.as_str(), |(pid, _)| pid)
            .parse::<libc::pid_t>()
            .unwrap_or(0);
        if pid > 1 {
            // SAFETY: a plain kill to one process that names the test's
            // own folder.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
    left
}

/// This test program run again as a dedicated fixture of its own test
/// `test` (`--exact <test> --nocapture`), with [`crate::env::command`]'s
/// environment over `home`. Its output, its libtest summary among it, goes
/// to `log`, never to this run's: whoever reads this run's output (the
/// timing report) finds one summary, the test's own. [`rerun_said`] reads
/// the log back for a failure message.
pub fn rerun(
    test: &str,
    home: &std::path::Path,
    log: &std::path::Path,
) -> std::io::Result<std::process::Command> {
    let out = std::fs::File::create(log)?;
    let err = out.try_clone()?;
    let mut command = crate::env::command(std::env::current_exe()?, home);
    command
        .args(["--exact", test, "--nocapture"])
        .env(crate::program::ONE, test)
        .stdout(out)
        .stderr(err);
    Ok(command)
}

/// What a [`rerun`] fixture wrote to `log`, each line marked as its own
/// (`fixture| `), so its libtest summary never reads as this test's.
pub fn rerun_said(log: &std::path::Path) -> String {
    match std::fs::read_to_string(log) {
        Ok(said) => said
            .lines()
            .map(|line| format!("fixture| {line}\n"))
            .collect(),
        Err(err) => format!("fixture| its log {} is unreadable: {err}\n", log.display()),
    }
}

/// An adopter for a dedicated Linux fixture process, never a shared test
/// harness. It reaps orphaned descendants when the container's PID 1
/// does not. It never signals a process.
#[cfg(target_os = "linux")]
pub struct Subreaper {
    previous: libc::c_int,
}

#[cfg(target_os = "linux")]
impl Subreaper {
    /// Call before starting fixture children. Only a dedicated supervisor
    /// may adopt: other tests can still own unreaped ChildGroup leaders.
    pub fn for_fixture() -> std::io::Result<Self> {
        let mut previous: libc::c_int = 0;
        // SAFETY: GET writes one initialized C int; SET changes only
        // this calling process's child-adoption behavior.
        if unsafe {
            libc::prctl(
                libc::PR_GET_CHILD_SUBREAPER,
                &mut previous as *mut libc::c_int,
            )
        } != 0
            || unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1 as libc::c_ulong) } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { previous })
    }

    /// Whether this dedicated fixture still owns a live or unreaped child.
    /// Observe without consuming an exit status owned by a ChildGroup.
    pub fn has_children(&self) -> std::io::Result<bool> {
        loop {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: waitid only observes this fixture's children; WNOWAIT
            // leaves every exit status for its owner or reaped().
            if unsafe {
                libc::waitid(
                    libc::P_ALL,
                    0,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            } == 0
            {
                return Ok(true);
            }
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::ECHILD) => return Ok(false),
                Some(libc::EINTR) => continue,
                _ => return Err(error),
            }
        }
    }

    /// Reap adopted exited children after every owned ChildGroup has
    /// dropped. True means no children remain. A live child is pending,
    /// never killed or silently treated as cleaned up.
    pub fn reaped(&self) -> std::io::Result<bool> {
        loop {
            let mut status = 0;
            // SAFETY: a nonblocking wait for this supervisor's children.
            let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
            if pid > 0 {
                continue;
            }
            if pid == 0 {
                return Ok(false);
            }
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::ECHILD) => return Ok(true),
                Some(libc::EINTR) => continue,
                _ => return Err(error),
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for Subreaper {
    fn drop(&mut self) {
        let _ = self.reaped();
        // SAFETY: restore only this process's previous adoption setting.
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, self.previous as libc::c_ulong) };
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_detached_session_owns_only_its_own_group() {
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        let home = crate::TempDir::new("owned-session");
        let bystander = ChildGroup::spawn(crate::env::command("sleep", &home).arg("300")).unwrap();
        // SAFETY: getsid only reads this process's session identity.
        let parent = unsafe { libc::getsid(0) };
        let mut child = ChildGroup::spawn_session(
            crate::env::command("sh", &home)
                .args(["-c", "echo ready; read -r done"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped()),
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(child.take_stdout().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line, "ready\n");
        let id = child.id() as libc::pid_t;
        // SAFETY: these read the direct child's group and session identities.
        assert_eq!(unsafe { libc::getsid(id) }, id);
        assert_eq!(unsafe { libc::getpgid(id) }, id);
        assert_eq!(child.try_status().unwrap(), None);
        child.stop();
        assert_eq!(bystander.try_status().unwrap(), None);
        // SAFETY: the calling process's session must remain unchanged.
        assert_eq!(unsafe { libc::getsid(0) }, parent);
    }

    /// A rerun fixture's output, its libtest summary among it, is in its
    /// log alone, read back with each line marked as the fixture's.
    #[test]
    fn a_rerun_fixture_keeps_its_libtest_summary_in_its_log() {
        const ROLE: &str = "LNK_TEST_KIT_RERUN_FIXTURE";
        if std::env::var_os(ROLE).is_some() {
            println!("the fixture ran");
            return;
        }
        let home = crate::TempDir::new("rerun");
        let log = home.join("fixture.log");
        let fixture = ChildGroup::spawn(
            rerun(
                "process::tests::a_rerun_fixture_keeps_its_libtest_summary_in_its_log",
                &home,
                &log,
            )
            .unwrap()
            .env(ROLE, "1"),
        )
        .unwrap();
        let status = crate::eventually(
            "the rerun fixture",
            std::time::Duration::from_secs(30),
            || fixture.try_status().unwrap(),
        );
        let said = rerun_said(&log);
        assert!(status.success(), "{said}");
        assert!(said.contains("fixture| the fixture ran\n"), "{said}");
        assert!(
            said.contains("fixture| test result: ok. 1 passed"),
            "{said}"
        );
        assert!(
            said.lines().all(|line| line.starts_with("fixture| ")),
            "{said}"
        );
    }

    /// A process naming the folder is found while it runs, and not once
    /// it has ended.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_naming_its_folder_is_left_until_it_ends() {
        let root = crate::TempDir::new("left-under");
        let folder = root.join("owned");
        let sibling = root.join("owned-elsewhere");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let _other = ChildGroup::spawn(
            crate::env::command("/bin/sleep", &sibling)
                .arg("30")
                .env("QA_FOLDER", &sibling),
        )
        .unwrap();
        assert!(left_under(&folder).is_empty());
        // This sleep only keeps the fixture alive until its owner stops it.
        let mut child = ChildGroup::spawn(
            crate::env::command("/bin/sleep", &folder)
                .arg("30")
                .env("QA_FOLDER", folder.as_os_str()),
        )
        .unwrap();
        let left = left_under(&folder);
        assert!(
            left.iter()
                .any(|l| l.starts_with(&format!("{} /bin/sleep", child.id()))),
            "{left:?}"
        );
        child.stop();
        assert!(left_under(&folder).is_empty());
    }

    /// A process naming the folder, in a session of its own and so no
    /// group the test owns, is ended by `end_under`.
    #[cfg(target_os = "linux")]
    #[test]
    fn end_under_ends_what_names_its_folder() {
        let folder = crate::TempDir::new("end-under");
        // This sleep stands in for a daemon its fixture started and lost.
        let mut child = ChildGroup::spawn(
            crate::env::command("/bin/sleep", &folder)
                .arg("30")
                .env("QA_FOLDER", folder.as_os_str()),
        )
        .unwrap();
        let ended = end_under(&folder);
        assert!(
            ended
                .iter()
                .any(|l| l.starts_with(&format!("{} /bin/sleep", child.id()))),
            "{ended:?}"
        );
        crate::Deadline::new("end_under", std::time::Duration::from_secs(30)).eventually(
            "the process to end",
            std::time::Duration::from_secs(10),
            || left_under(&folder).is_empty().then_some(()).ok_or(()),
        );
        child.stop();
    }

    #[test]
    fn interrupting_a_group_preserves_its_exit_and_leaves_another_group_running() {
        use std::os::unix::process::ExitStatusExt;

        let home = crate::TempDir::new("interrupted-group");
        // These sleeps keep owned fixtures alive until explicitly interrupted.
        let mut group =
            ChildGroup::spawn(crate::env::command("/bin/sleep", &home).arg("30")).unwrap();
        let other = ChildGroup::spawn(crate::env::command("/bin/sleep", &home).arg("30")).unwrap();
        group.interrupt();
        let status = crate::wait::eventually(
            "the interrupted leader to exit",
            std::time::Duration::from_secs(5),
            || group.try_status().unwrap(),
        );
        assert_eq!(status.signal(), Some(libc::SIGINT));
        assert_eq!(group.try_status().unwrap(), Some(status));
        assert!(other.try_status().unwrap().is_none());
        group.stop();
        group.interrupt();
        assert!(group.try_status().is_err());
        assert!(other.try_status().unwrap().is_none());
    }

    #[test]
    fn observing_an_exit_keeps_its_status() {
        use std::os::unix::process::ExitStatusExt;
        for (script, code, signal) in [
            ("exit 0", Some(0), None),
            ("exit 7", Some(7), None),
            ("kill -KILL $$", None, Some(libc::SIGKILL)),
        ] {
            let home = crate::TempDir::new("observed-group");
            let group =
                ChildGroup::spawn(crate::env::command("sh", &home).args(["-c", script])).unwrap();
            let observed = crate::eventually(
                "the guarded leader's natural exit",
                std::time::Duration::from_secs(10),
                || group.try_status().unwrap(),
            );
            assert_eq!(observed.code(), code);
            assert_eq!(observed.signal(), signal);
            assert_eq!(group.try_status().unwrap(), Some(observed));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn observing_an_exit_owns_cleanup_of_its_descendants() {
        let home = crate::TempDir::new("observed-group");
        // The sleep keeps a grandchild alive for the cleanup proof.
        let mut group = ChildGroup::spawn(
            crate::env::command("sh", &home)
                .args(["-c", "sleep 300 & echo $!; exit 0"])
                .stdout(std::process::Stdio::piped()),
        )
        .unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(group.take_stdout().unwrap()),
            &mut line,
        )
        .unwrap();
        let grandchild = line.trim().parse::<u32>().unwrap();
        crate::eventually(
            "the guarded leader's natural exit",
            std::time::Duration::from_secs(10),
            || group.try_status().unwrap(),
        );
        assert!(
            left_under(&home)
                .iter()
                .any(|line| line.starts_with(&format!("{grandchild} ")))
        );
        drop(group);
        crate::eventually(
            "the exited leader's descendants to stop",
            std::time::Duration::from_secs(10),
            || left_under(&home).is_empty(),
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_panic_stops_the_owned_group_reaps_its_leader_and_leaves_another_alone() {
        let home = crate::TempDir::new("unwinding-group");
        let other_home = crate::TempDir::new("other-group");
        // These sleeps keep the fixtures alive; neither synchronizes a step.
        let other =
            ChildGroup::spawn(crate::env::command("sleep", &other_home).arg("300")).unwrap();
        let leader_id = std::cell::Cell::new(None);
        let unwound: std::thread::Result<()> =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut leader = ChildGroup::spawn(
                    crate::env::command("sh", &home)
                        .args(["-c", "sleep 300 & echo $!; wait"])
                        .stdout(std::process::Stdio::piped()),
                )
                .unwrap();
                leader_id.set(Some(leader.id()));
                let mut line = String::new();
                std::io::BufRead::read_line(
                    &mut std::io::BufReader::new(leader.take_stdout().unwrap()),
                    &mut line,
                )
                .unwrap();
                let grandchild: u32 = line.trim().parse().unwrap();
                let owned = left_under(&home);
                for pid in [leader.id(), grandchild] {
                    assert!(
                        owned
                            .iter()
                            .any(|line| line.starts_with(&format!("{pid} ")))
                    );
                }
                panic!("exercise fixture cleanup during assertion unwinding");
            }));
        let panic = unwound.expect_err("the fixture must exercise unwinding");
        assert_eq!(
            panic.downcast_ref::<&str>(),
            Some(&"exercise fixture cleanup during assertion unwinding"),
            "fixture setup must complete before its deliberate panic"
        );
        crate::eventually(
            "the unwound fixture's child and grandchild to stop",
            std::time::Duration::from_secs(10),
            || left_under(&home).is_empty(),
        );
        let mut status = 0;
        // SAFETY: a nonblocking wait for this test's recorded direct child.
        let waited = unsafe {
            libc::waitpid(
                leader_id.get().expect("the fixture started") as libc::pid_t,
                &mut status,
                libc::WNOHANG,
            )
        };
        let wait_error = std::io::Error::last_os_error();
        assert_eq!(waited, -1, "the guard must reap its direct child");
        assert_eq!(wait_error.raw_os_error(), Some(libc::ECHILD));
        assert!(
            left_under(&other_home)
                .iter()
                .any(|line| line.starts_with(&format!("{} ", other.id())))
        );
    }
}

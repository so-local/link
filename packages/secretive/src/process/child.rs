//! Keep the leader waitable until its whole ordinary-call group is stopped.
use std::{
    io,
    process::{Child, Command, ExitStatus},
};

/// An ordinary protocol subprocess and its group, with exclusive wait ownership.
/// Cleanup keeps the leader unreaped until the owned group is stopped. Descendants
/// must stay in that group; durable resources need a separate recorded owner.
pub struct OwnedChild(Option<Child>);
impl OwnedChild {
    /// Spawns the configured command in its own process group.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        use std::os::unix::process::CommandExt;
        command
            .process_group(0)
            .spawn()
            .map(|child| Self(Some(child)))
    }
    /// Accesses pipes and process identity without transferring wait ownership.
    /// Never call `wait` or `try_wait` on this borrow; use `exited` or `finish`.
    pub fn child(&mut self) -> &mut Child {
        self.0.as_mut().expect("owned protocol child")
    }
    /// Observes completion without reaping the leader or releasing its identity.
    pub fn exited(&self) -> io::Result<bool> {
        observed(self.0.as_ref().expect("owned protocol child"))
    }
    /// Stops the owned group before reaping the leader and returning its status.
    /// Lost wait ownership refuses numeric process-group signalling.
    pub fn finish(mut self) -> io::Result<ExitStatus> {
        stop(self.0.take().expect("owned protocol child"))
    }
    /// Transfers the child, pipes and complete group-cleanup obligation to the caller.
    /// This guard performs no cleanup after transfer.
    pub fn release(mut self) -> Child {
        self.0.take().expect("owned protocol child")
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.take() {
            let _ = stop(child);
        }
    }
}
fn observed(child: &Child) -> io::Result<bool> {
    // SAFETY: siginfo_t is a C record with a valid all-zero representation.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: the writable record observes our direct child without reaping it.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            // SAFETY: a successful waitid initializes the observed child identity.
            return Ok(unsafe { info.si_pid() } != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}
fn stop(mut child: Child) -> io::Result<ExitStatus> {
    // In particular, ECHILD means identity ownership was lost to an external
    // reaper. Refuse to signal a numeric group after that ownership was lost.
    observed(&child)?;
    // SAFETY: this unreaped direct child still reserves its group identity.
    let result = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    if result != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) && !only_its_finished_leader(&error, &child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    }
    let _ = child.kill();
    child.wait()
}
/// A Mac refuses a signal to a group holding nothing but its finished leader
/// with `EPERM`, where Linux signals the leader and succeeds. A leader still
/// running means the refusal is real.
#[cfg(target_os = "macos")]
fn only_its_finished_leader(error: &io::Error, child: &Child) -> bool {
    error.raw_os_error() == Some(libc::EPERM) && observed(child).unwrap_or(false)
}
#[cfg(not(target_os = "macos"))]
fn only_its_finished_leader(_: &io::Error, _: &Child) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leader_remains_owned_until_cleanup_and_external_reaping_refuses_signals() {
        let home = link_test_kit::TempDir::new("vm-protocol-leader");
        let other_home = link_test_kit::TempDir::new("vm-protocol-unrelated");
        let other = link_test_kit::ChildGroup::spawn(
            link_test_kit::env::command("sleep", &other_home).arg("300"),
        )
        .unwrap();
        let mut command = link_test_kit::env::command("sh", &home);
        command.args(["-c", "exit 7"]);
        let owned = OwnedChild::spawn(&mut command).unwrap();
        link_test_kit::eventually(
            "the owned leader's exit",
            std::time::Duration::from_secs(10),
            || owned.exited().unwrap(),
        );
        assert!(owned.exited().unwrap(), "observing exit reaped the leader");
        assert_eq!(owned.finish().unwrap().code(), Some(7));
        let mut owned = OwnedChild::spawn(&mut command).unwrap();
        assert_eq!(owned.child().wait().unwrap().code(), Some(7));
        assert_eq!(
            owned.finish().unwrap_err().raw_os_error(),
            Some(libc::ECHILD)
        );
        let mut command = link_test_kit::env::command("sleep", &home);
        command.arg("300");
        let owned = OwnedChild::spawn(&mut command).unwrap();
        // Re-own the transferred child immediately so every assertion has cleanup.
        let transferred = OwnedChild(Some(owned.release()));
        assert!(
            !transferred.exited().unwrap(),
            "stream handoff stopped its caller's child"
        );
        drop(transferred);
        assert_eq!(other.try_status().unwrap(), None);
    }
    #[test]
    fn a_finished_leader_still_has_its_group_stopped() {
        let home = link_test_kit::TempDir::new("vm-protocol-group");
        #[cfg(target_os = "linux")]
        let adopter = {
            const CHILD: &str = "LNK_TEST_PROTOCOL_GROUP";
            if std::env::var_os(CHILD).is_none() {
                let output = link_test_kit::env::command(std::env::current_exe().unwrap(), &home)
                    .args([
                        "--exact",
                        "process::child::tests::a_finished_leader_still_has_its_group_stopped",
                        "--show-output",
                    ])
                    .env(CHILD, "1")
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}\n{}{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                return;
            }
            link_test_kit::process::Subreaper::for_fixture().unwrap()
        };
        let mut command = link_test_kit::env::command("sh", &home);
        command.args(["-c", "sleep 300 & exit 7"]);
        let owned = OwnedChild::spawn(&mut command).unwrap();
        let group = owned.0.as_ref().unwrap().id() as libc::pid_t;
        link_test_kit::eventually(
            "the owned leader's exit",
            std::time::Duration::from_secs(10),
            || owned.exited().unwrap(),
        );
        assert_eq!(owned.finish().unwrap().code(), Some(7));
        #[cfg(target_os = "linux")]
        link_test_kit::eventually(
            "the stopped descendants reaped",
            std::time::Duration::from_secs(10),
            || adopter.reaped().unwrap(),
        );
        link_test_kit::eventually(
            "the group's other process stopped",
            std::time::Duration::from_secs(10),
            // SAFETY: signal 0 only asks whether the group is there.
            || unsafe { libc::kill(-group, 0) } != 0,
        );
    }
}

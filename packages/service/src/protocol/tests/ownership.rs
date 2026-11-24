//! A pidfd owns cleanup of the deliberately exposed grandchild even on a red run.
use super::super::*;
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::OpenOptionsExt,
    },
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

struct Tracked(OwnedFd);
impl Tracked {
    fn open(pid: u32) -> Self {
        // SAFETY: pid identifies this fixture's live process at its handshake.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0u32) };
        assert!(fd >= 0, "pidfd_open: {}", std::io::Error::last_os_error());
        // SAFETY: the successful syscall returned a new owned descriptor.
        Self(unsafe { OwnedFd::from_raw_fd(fd as i32) })
    }
    fn exited(&self) -> bool {
        let mut poll = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll points to one initialized descriptor owned by this guard.
        let result = unsafe { libc::poll(&mut poll, 1, 0) };
        assert!(result >= 0);
        poll.revents & libc::POLLIN != 0
    }
}
impl Drop for Tracked {
    fn drop(&mut self) {
        // SAFETY: the descriptor pins this fixture process, never a recycled PID.
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.0.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0u32,
            );
        }
    }
}
struct Release {
    gate: File,
    expired: Arc<AtomicBool>,
    inspection: Arc<(Mutex<bool>, Condvar)>,
}
impl Release {
    fn go(&mut self) {
        self.gate.write_all(b"go\n").unwrap();
    }
    fn inspect_done(&self) {
        *self.inspection.0.lock().unwrap() = true;
        self.inspection.1.notify_all();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.expired.store(true, Ordering::SeqCst);
        let _ = self.gate.write_all(b"go\n");
        self.inspect_done();
    }
}
fn fixture(home: &std::path::Path, mode: &str, hold_stdout: bool) -> (Command, File) {
    let gate = home.join("release");
    let path = CString::new(gate.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: path is an owned NUL-terminated path in this test's directory.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let gate = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(gate)
        .unwrap();
    let script = if hold_stdout {
        r#"IFS= read -r request; sleep 300 </dev/null & printf '%s %s\n' "$$" "$!" >"$VM_PIDS"; IFS= read -r go <"$VM_GATE"; printf '{}\n'"#
    } else {
        r#"IFS= read -r request; sleep 300 </dev/null >/dev/null 2>&1 & printf '%s %s\n' "$$" "$!" >"$VM_PIDS"; IFS= read -r go <"$VM_GATE"; if [ "$VM_MODE" = json ]; then printf 'broken\n'; else printf '{}\n'; fi; if [ "$VM_MODE" = status ]; then exit 7; fi"#
    };
    let mut command = link_test_kit::env::command("sh", home);
    command
        .args(["-c", script])
        .env("VM_PIDS", home.join("pids"))
        .env("VM_GATE", home.join("release"))
        .env("VM_MODE", mode);
    (command, gate)
}
fn pids(home: &std::path::Path) -> (u32, u32) {
    link_test_kit::eventually(
        "the exchange fixture's process handshake",
        Duration::from_secs(10),
        || {
            let text = std::fs::read_to_string(home.join("pids")).ok()?;
            let mut ids = text.split_whitespace().map(str::parse::<u32>);
            Some((ids.next()?.ok()?, ids.next()?.ok()?))
        },
    )
}
fn released_leader(pid: u32) -> bool {
    // SAFETY: observing our child with WNOWAIT never releases its process identity.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let status = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    status == 0 && unsafe { info.si_pid() } == pid as i32
}
struct Panics;
impl<'de> serde::Deserialize<'de> for Panics {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        panic!("fixture reply decoder unwound")
    }
}
#[test]
fn ordinary_exchange_cleans_descendants_on_success_error_and_decoder_unwind() {
    for mode in ["success", "status", "json", "panic", "pump-panic"] {
        let home = link_test_kit::TempDir::new("protocol-exchange-owned");
        let other_home = link_test_kit::TempDir::new("protocol-exchange-unrelated");
        let other = link_test_kit::ChildGroup::spawn(
            link_test_kit::env::command("sleep", &other_home).arg("300"),
        )
        .unwrap();
        let (command, gate) = fixture(&home, mode, false);
        let expired = Arc::new(AtomicBool::new(false));
        let panic_armed = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let mut release = Release {
                gate,
                expired: expired.clone(),
                inspection: Arc::new((Mutex::new(false), Condvar::new())),
            };
            let worker = scope.spawn(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if mode == "panic" {
                        exchange_until::<_, Panics>(command, &(), || expired.load(Ordering::SeqCst))
                            .map(|_| serde_json::Value::Null)
                    } else {
                        exchange_until::<_, serde_json::Value>(command, &(), || {
                            assert!(
                                !(mode == "pump-panic" && panic_armed.load(Ordering::SeqCst)),
                                "fixture protocol pump unwound"
                            );
                            expired.load(Ordering::SeqCst)
                        })
                    }
                }))
            });
            let (leader, descendant) = pids(&home);
            let _leader = Tracked::open(leader);
            let descendant = Tracked::open(descendant);
            panic_armed.store(true, Ordering::SeqCst);
            release.go();
            let result = worker.join().unwrap();
            match mode {
                "success" => assert_eq!(result.unwrap().unwrap(), serde_json::json!({})),
                "panic" | "pump-panic" => assert!(result.is_err()),
                _ => assert!(result.unwrap().is_err()),
            }
            link_test_kit::eventually(
                "the exchange's owned descendant to stop",
                Duration::from_secs(1),
                || descendant.exited(),
            );
            assert_eq!(
                other.try_status().unwrap(),
                None,
                "cleanup signalled an unrelated group"
            );
        });
    }
}
#[test]
fn deadline_after_leader_exit_keeps_its_identity_until_group_cleanup() {
    let home = link_test_kit::TempDir::new("protocol-exchange-held-output");
    let (command, gate) = fixture(&home, "success", true);
    let expired = Arc::new(AtomicBool::new(false));
    let inspect = AtomicBool::new(false);
    let inspection = Arc::new((Mutex::new(false), Condvar::new()));
    let (entered, waiting) = mpsc::channel();
    std::thread::scope(|scope| {
        let mut release = Release {
            gate,
            expired: expired.clone(),
            inspection: inspection.clone(),
        };
        let worker = scope.spawn(|| {
            exchange_until::<_, serde_json::Value>(command, &(), || {
                if inspect.swap(false, Ordering::SeqCst) {
                    entered.send(()).unwrap();
                    let mut done = inspection.0.lock().unwrap();
                    while !*done {
                        done = inspection.1.wait(done).unwrap();
                    }
                    return true;
                }
                expired.load(Ordering::SeqCst)
            })
        });
        let (leader, descendant) = pids(&home);
        let leader_owner = Tracked::open(leader);
        let descendant = Tracked::open(descendant);
        release.go();
        link_test_kit::eventually(
            "the protocol leader's exit",
            Duration::from_secs(10),
            || leader_owner.exited(),
        );
        inspect.store(true, Ordering::SeqCst);
        link_test_kit::eventually(
            "the armed deadline observation",
            Duration::from_secs(10),
            || waiting.try_recv().ok(),
        );
        let still_owned = released_leader(leader);
        release.inspect_done();
        assert!(worker.join().unwrap().is_err());
        assert!(
            still_owned,
            "the protocol loop reaped its leader before stdout EOF and deadline cleanup"
        );
        link_test_kit::eventually(
            "the timed-out exchange's descendant to stop",
            Duration::from_secs(1),
            || descendant.exited(),
        );
    });
}

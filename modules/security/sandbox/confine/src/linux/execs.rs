//! What a sandbox runs, seen as each program starts (Linux).
//!
//! `sandbox confine`, first inside the sandbox, installs a seccomp filter
//! that hands it each `execve` and `execveat` ([`crate::linux::seccomp::listen`]),
//! starts the program as its child, and stays to watch: it reads which
//! file each call names and lets the call go on as it was
//! (`SECCOMP_USER_NOTIF_FLAG_CONTINUE`). It decides nothing; Landlock
//! decides what runs. What it sees it only reports:
//!
//! - with `[commands]` listing programs, each program the list refuses,
//!   by name, with its fix ([`Watch::Refusals`]);
//! - for `lnk sandbox learn`, every program started, to a file
//!   descriptor the run was given ([`Watch::Learn`]).
//!
//! With `[commands]` listing programs, it also makes every memory file
//! (`memfd_create`) one that can never be run: a program
//! copied into one is in no folder for Landlock to judge, and would run
//! through `/proc/self/fd`. The memory is still the program's to map
//! executable (a JIT); only running it as a program is closed.
//! When sealing support or this listener is unavailable, the base
//! seccomp filter refuses memory file creation instead.
//!
//! The path is read from the calling program's memory before the call
//! goes on, so a program can show it one file and run another (it
//! changes the path in between). That misleads the report, never what
//! runs. A program that keeps its memory from being read (it set itself
//! non-dumpable, as `ssh` does) isn't seen.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

pub use crate::programs::{Learned, allows, interpreter, learned};

/// The variable naming the file descriptor `lnk sandbox learn` reads the
/// programs started from: one line each, the path as the program gave
/// it, a tab, and the file it is.
pub const LEARN_FD: &str = "LNK_SANDBOX_LEARN_FD";

/// The most refusals said in one run: a program trying many can't flood
/// its own terminal or an agent's log with them.
const MOST_SAID: usize = 50;

/// What to do with each program started.
pub enum Watch {
    /// Say which of them `programs` (files, or folders of them) refuses.
    Refusals(Vec<PathBuf>),
    /// Write each, and the interpreters a script starts with, to this file.
    Learn(std::fs::File),
}

/// A program a call starts: the path as it was given (absolute), and the
/// file it is, every link resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Started {
    pub given: PathBuf,
    pub file: PathBuf,
}

/// How many threads take the execs the listener hands over, each one
/// whole, from the call to its report: programs starting at once wait on
/// none but their own.
const RECEIVERS: usize = 4;

/// What [`answer`] reports to, shared by the receiving threads of one
/// [`supervise`]: the watch and the refusals said so far, and the execs
/// taken and not yet reported.
struct Reports {
    said: std::sync::Mutex<(Watch, BTreeSet<PathBuf>)>,
    /// One is counted before it's let go on, so before its program can
    /// fail and the child exit: once the child has, [`Reports::settled`]
    /// waits for this to be none, and every refusal it made is said
    /// before the sandbox ends.
    in_flight: std::sync::Mutex<usize>,
    reported: std::sync::Condvar,
}

impl Reports {
    fn new(watch: Watch) -> Self {
        Reports {
            said: std::sync::Mutex::new((watch, BTreeSet::new())),
            in_flight: std::sync::Mutex::new(0),
            reported: std::sync::Condvar::new(),
        }
    }

    /// An exec taken: in flight until the guard is dropped.
    fn taken(&self) -> InFlight<'_> {
        *self.in_flight.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        InFlight(self)
    }

    /// Waits until every exec taken has been reported, a few seconds at
    /// most (a report never takes that long; a stuck one doesn't hold the
    /// sandbox open).
    fn settled(&self) {
        let count = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self
            .reported
            .wait_timeout_while(count, std::time::Duration::from_secs(5), |n| *n > 0);
    }
}

/// One exec in [`Reports::in_flight`], until it's dropped.
struct InFlight<'a>(&'a Reports);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut count = self.0.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        *count -= 1;
        self.0.reported.notify_all();
    }
}

/// Watches `child` until it exits, answering every exec its listener
/// (from [`crate::linux::seccomp::listen`]) hands over, on [`RECEIVERS`] threads;
/// returns the child's wait status. A stop reaches the child directly,
/// in this process's group.
pub fn supervise(listener: OwnedFd, child: libc::pid_t, watch: Watch) -> i32 {
    // Its descriptors, the listener and `learn`'s pipe, are its own: not
    // dumpable, no program in the sandbox opens them through
    // `/proc/<pid>/fd` or takes them with `pidfd_getfd`. (It still reads
    // the programs' memory: that's theirs to allow, and they're dumpable.)
    // SAFETY: a plain prctl on this process.
    unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
    // Its child is in its process group, which every stop reaches
    // whole: the terminal's, and on Linux a stop sent to the sandbox
    // ([`crate::linux::session`]). Each is left to reach the child
    // directly, never passed on a second time; this process outlives it
    // and ends as it did.
    for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT, libc::SIGQUIT] {
        // SAFETY: ignoring a signal installs no handler.
        unsafe { libc::signal(signal, libc::SIG_IGN) };
    }
    // Held until this process exits, with the child: the receivers wait
    // on it for good.
    let listener: &'static OwnedFd = Box::leak(Box::new(listener));
    let reports: &'static Reports = Box::leak(Box::new(Reports::new(watch)));
    let mut receiving = 0;
    for _ in 0..RECEIVERS {
        let spawned = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                loop {
                    answer(listener.as_raw_fd(), reports);
                }
            });
        receiving += usize::from(spawned.is_ok());
    }
    if receiving == 0 {
        return alone(listener.as_raw_fd(), child, reports);
    }
    loop {
        let mut status = 0;
        // SAFETY: waits for our own child; a signal passed on interrupts
        // it, and it waits again.
        let waited = unsafe { libc::waitpid(child, &mut status, 0) };
        if waited == child {
            reports.settled();
            return status;
        }
        if waited < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return status;
        }
    }
}

/// [`supervise`] on this one thread, none other started (out of
/// threads): it waits for an exec or the child's end, whichever comes.
fn alone(listener: i32, child: libc::pid_t, reports: &Reports) -> i32 {
    // An alarm only interrupts a wait for a call (below): no restart.
    extern "C" fn wake(_: libc::c_int) {}
    // SAFETY: a zeroed sigaction with a handler that does nothing.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = wake as *const () as libc::sighandler_t;
        libc::sigaction(libc::SIGALRM, &action, std::ptr::null_mut());
    }
    // SAFETY: pidfd_open takes a pid and flags, and returns a descriptor.
    let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, child, 0) } as i32;
    loop {
        let mut fds = [
            libc::pollfd {
                fd: listener,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: pidfd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // Without a pidfd (it failed), a tenth of a second at a time.
        let (count, wait) = match pidfd >= 0 {
            true => (2, -1),
            false => (1, 100),
        };
        // SAFETY: `fds` holds `count` pollfds.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), count, wait) };
        if ready > 0 && fds[0].revents & libc::POLLIN != 0 {
            // A call can be gone between the poll and here (its program
            // killed), and the wait would be for the next: a second at
            // most, then back to see whether the child is done.
            // SAFETY: alarms, around a wait for a call.
            unsafe { libc::alarm(1) };
            answer(listener, reports);
            unsafe { libc::alarm(0) };
        }
        let mut status = 0;
        // SAFETY: waits for our own child without blocking.
        if unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) } == child {
            return status;
        }
    }
}

/// Waits for one exec from the listener, sees what it starts, and lets
/// it go on.
#[expect(
    clippy::disallowed_methods,
    reason = "rests after an unexpected error rather than spin"
)]
fn answer(listener: i32, reports: &Reports) {
    // SAFETY: the kernel wants a zeroed struct to fill.
    let mut call: libc::seccomp_notif = unsafe { std::mem::zeroed() };
    // SAFETY: an ioctl filling `call`, waiting for the next.
    if unsafe { libc::ioctl(listener, libc::SECCOMP_IOCTL_NOTIF_RECV, &mut call) } != 0 {
        match std::io::Error::last_os_error().raw_os_error() {
            // Gone before it was read (killed), or interrupted.
            Some(libc::ENOENT | libc::EINTR) => {}
            // Anything else, not in a hurry.
            _ => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
        return;
    }
    // Counted before the call goes on: its program can't end the child
    // before its report is made.
    let _in_flight = reports.taken();
    if i64::from(call.data.nr) == libc::SYS_memfd_create {
        memfd(listener, &call);
        return;
    }
    // The path is read while the call waits, and the call goes on at
    // once: Landlock decides what runs, so finding the file, its
    // interpreters and whether the list allows them only names a refusal,
    // after. A program starting waits for no more than the read.
    let given = given(listener, &call);
    let mut go_on = libc::seccomp_notif_resp {
        id: call.id,
        val: 0,
        error: 0,
        flags: libc::SECCOMP_USER_NOTIF_FLAG_CONTINUE as u32,
    };
    // SAFETY: `go_on` is the struct this ioctl reads. If it fails, the
    // call is gone.
    unsafe { libc::ioctl(listener, libc::SECCOMP_IOCTL_NOTIF_SEND, &mut go_on) };
    let Some(started) = given.and_then(program) else {
        return;
    };
    let chain = interpreters(&started.file);
    let Ok(mut reports) = reports.said.lock() else {
        return;
    };
    let (watch, said) = &mut *reports;
    match watch {
        Watch::Refusals(programs) => {
            let refused = std::iter::once(&started)
                .chain(&chain)
                .find(|s| !allows(programs, &s.file));
            if let Some(refused) = refused
                && said.len() < MOST_SAID
                && said.insert(refused.file.clone())
            {
                // One write, so the program's own output never splits it.
                link_format::aside(link_format::Note(link_format::Sentence::adopt(&refusal(
                    refused, &started,
                ))));
            }
        }
        Watch::Learn(out) => {
            let mut lines = format!("{}\t{}\n", started.given.display(), started.file.display());
            for interpreter in &chain {
                lines.push_str(&format!(
                    "{}\t{}\n",
                    interpreter.given.display(),
                    interpreter.file.display()
                ));
            }
            let _ = out.write_all(lines.as_bytes());
        }
    }
}

/// The longest name a memory file takes, and its end (`MFD_NAME_MAX_LEN`).
const MEMFD_NAME: usize = 250;

/// A `memfd_create` the filter handed over, one that could make a file
/// that runs: made here as asked but never runnable (`MFD_NOEXEC_SEAL`,
/// which also refuses making it runnable later), and put in the calling
/// program as the call's answer, a descriptor as the call gives. An error
/// making it is the call's error: the program's own call never runs.
fn memfd(listener: i32, call: &libc::seccomp_notif) {
    let flags = call.data.args[1] as u32;
    let fail = |errno: i32| {
        let mut resp = libc::seccomp_notif_resp {
            id: call.id,
            val: 0,
            error: -errno,
            flags: 0,
        };
        // SAFETY: `resp` is the struct this ioctl reads. If it fails, the
        // call is gone.
        unsafe { libc::ioctl(listener, libc::SECCOMP_IOCTL_NOTIF_SEND, &mut resp) };
    };
    let name = match memfd_name(call.pid, call.data.args[0]) {
        Ok(name) => name,
        Err(errno) => return fail(errno),
    };
    let made = match sealed(&name, flags) {
        Ok(made) => made,
        Err(errno) => return fail(errno),
    };
    let mut add = libc::seccomp_notif_addfd {
        id: call.id,
        flags: libc::SECCOMP_ADDFD_FLAG_SEND as u32,
        srcfd: made.as_raw_fd() as u32,
        newfd: 0,
        newfd_flags: match flags & libc::MFD_CLOEXEC {
            0 => 0,
            _ => libc::O_CLOEXEC as u32,
        },
    };
    // SAFETY: `add` is the struct this ioctl reads. It answers the call
    // with the descriptor it puts in the program.
    if unsafe { libc::ioctl(listener, libc::SECCOMP_IOCTL_NOTIF_ADDFD, &mut add) } < 0 {
        // Its table of descriptors full, say; or the call is gone.
        fail(
            std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EIO),
        );
    }
}

/// The name a `memfd_create` call of the program at `pid` gives at `at`,
/// as the kernel would read it: `EFAULT` where its memory has none there
/// (a null or unmapped pointer). A program whose memory can't be read at
/// all (it made itself non-dumpable) gets its file named for what it is.
fn memfd_name(pid: u32, at: u64) -> Result<Vec<u8>, i32> {
    let Ok(memory) = std::fs::File::open(format!("/proc/{pid}/mem")) else {
        return Ok(b"memfd".into());
    };
    read_from(&memory, at, MEMFD_NAME).ok_or(libc::EFAULT)
}

/// A memory file named `name`, made with `flags` but never runnable:
/// `MFD_EXEC` dropped, `MFD_NOEXEC_SEAL` added. Ours closes on exec.
/// Both asked for at once is `EINVAL`, as the kernel answers.
fn sealed(name: &[u8], flags: u32) -> Result<OwnedFd, i32> {
    use crate::linux::seccomp::{MFD_EXEC, MFD_NOEXEC_SEAL};
    use std::os::fd::FromRawFd;
    if flags & MFD_EXEC != 0 && flags & MFD_NOEXEC_SEAL != 0 {
        return Err(libc::EINVAL);
    }
    // Read up to its end, so no NUL inside.
    let name = std::ffi::CString::new(name).map_err(|_| libc::EINVAL)?;
    // `MFD_NOEXEC_SEAL` also lets the program add seals of its own, as if
    // it had asked for `MFD_ALLOW_SEALING`: the kernel makes it so.
    let flags = (flags & !MFD_EXEC) | MFD_NOEXEC_SEAL | libc::MFD_CLOEXEC;
    // SAFETY: a NUL-terminated name and flags; it returns a descriptor.
    let fd = unsafe { libc::syscall(libc::SYS_memfd_create, name.as_ptr(), flags) } as i32;
    if fd < 0 {
        return Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EINVAL));
    }
    // SAFETY: the kernel just gave it, and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Whether this kernel makes a memory file that can never be run when
/// asked (`MFD_NOEXEC_SEAL`, Linux 6.3 and newer), as the watch needs.
pub fn sealable() -> bool {
    sealed(b"lnk", 0).is_ok()
}

/// What a refused program is told: which file, and the fix.
pub fn refusal(refused: &Started, started: &Started) -> String {
    let whose = match refused == started {
        true => String::new(),
        false => format!(" ({}'s interpreter)", started.given.display()),
    };
    format!(
        "lnk sandbox: refused {}{whose}: add it to [commands] allow",
        refused.given.display()
    )
}

/// The path the exec `call` names: read from the calling program's
/// memory, made absolute from its folder, and checked to still be that
/// call's (its process could have gone and its id been reused).
fn given(listener: i32, call: &libc::seccomp_notif) -> Option<PathBuf> {
    let pid = call.pid;
    let args = call.data.args;
    let (dirfd, at) = match i64::from(call.data.nr) {
        n if n == libc::SYS_execve => (libc::AT_FDCWD, args[0]),
        n if n == libc::SYS_execveat => (args[0] as i32, args[1]),
        _ => return None,
    };
    let path = read_string(pid, at)?;
    let given = match path.is_absolute() {
        true => path,
        false => {
            let base = match dirfd {
                libc::AT_FDCWD => std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?,
                fd => std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).ok()?,
            };
            base.join(path)
        }
    };
    // SAFETY: the id is the call's; the kernel answers whether it's live.
    let live = unsafe { libc::ioctl(listener, libc::SECCOMP_IOCTL_NOTIF_ID_VALID, &call.id) } == 0;
    live.then_some(given)
}

/// `given` as a program that's there: a regular file, every link
/// resolved.
fn program(given: PathBuf) -> Option<Started> {
    // Links resolved as the kernel resolves them, before each `..`; the
    // path without them only to show.
    let file = given.canonicalize().ok()?;
    let given = crate::lexical(&given);
    file.is_file().then_some(Started { given, file })
}

/// A path the program at `pid` holds at `at` in its memory, up to the
/// longest a path can be.
fn read_string(pid: u32, at: u64) -> Option<PathBuf> {
    let text = read_c(pid, at, libc::PATH_MAX as usize)?;
    // Longer than a path can be.
    if text.len() >= libc::PATH_MAX as usize {
        return None;
    }
    let text: std::ffi::OsString = std::os::unix::ffi::OsStringExt::from_vec(text);
    Some(PathBuf::from(text))
}

/// The text the program at `pid` holds at `at` in its memory, up to its
/// NUL, or its first `most` bytes when it goes on.
fn read_c(pid: u32, at: u64, most: usize) -> Option<Vec<u8>> {
    let memory = std::fs::File::open(format!("/proc/{pid}/mem")).ok()?;
    read_from(&memory, at, most)
}

/// The text at `at` in `memory` (a program's `/proc/<pid>/mem`), up to
/// its NUL, or its first `most` bytes when it goes on.
fn read_from(memory: &std::fs::File, at: u64, most: usize) -> Option<Vec<u8>> {
    use std::os::unix::fs::FileExt;
    let mut text = Vec::new();
    let mut chunk = [0u8; 256];
    while text.len() < most {
        let want = chunk.len().min(most - text.len());
        let n = memory
            .read_at(&mut chunk[..want], at + text.len() as u64)
            .ok()?;
        if n == 0 {
            return None;
        }
        if let Some(end) = chunk[..n].iter().position(|&b| b == 0) {
            text.extend_from_slice(&chunk[..end]);
            return Some(text);
        }
        text.extend_from_slice(&chunk[..n]);
    }
    Some(text)
}

/// The interpreters a script starts (its `#!` line, and that one's, as
/// the kernel follows them, four deep at most), each a program that runs
/// too.
pub fn interpreters(file: &Path) -> Vec<Started> {
    let mut chain = Vec::new();
    let mut at = file.to_path_buf();
    for _ in 0..4 {
        let Some(next) = shebang(&at).and_then(program) else {
            break;
        };
        at = next.file.clone();
        chain.push(next);
    }
    chain
}

/// The interpreter a `#!` line names, if `file` starts with one.
fn shebang(file: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    // Never waiting: a program may have put a FIFO where the file was
    // (read without blocking, and only a regular file), or a link.
    let mut opened = std::fs::File::options()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(file)
        .ok()?;
    if !opened.metadata().ok()?.is_file() {
        return None;
    }
    let mut head = [0u8; 256];
    let n = opened.read(&mut head).ok()?;
    let line = head[..n].strip_prefix(b"#!")?;
    let line = &line[..line.iter().position(|&b| b == b'\n').unwrap_or(line.len())];
    let line = String::from_utf8_lossy(line);
    let interpreter = line.split_whitespace().next()?;
    interpreter
        .starts_with('/')
        .then(|| PathBuf::from(interpreter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memory_file_made_here_never_runs() {
        use crate::linux::seccomp::{MFD_EXEC, MFD_NOEXEC_SEAL};
        if !sealable() {
            link_test_kit::gate::lacks(
                "sealed memory files",
                "this kernel can't seal a memory file",
                None,
            );
            return;
        }
        assert_eq!(
            sealed(b"x", MFD_EXEC | MFD_NOEXEC_SEAL).err(),
            Some(libc::EINVAL)
        );
        for flags in [0, MFD_EXEC, libc::MFD_ALLOW_SEALING] {
            let made = sealed(b"x", flags).unwrap();
            let meta = std::fs::File::from(made.try_clone().unwrap())
                .metadata()
                .unwrap();
            assert_eq!(
                std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111,
                0
            );
            // SAFETY: fchmod on our own descriptor.
            assert_ne!(unsafe { libc::fchmod(made.as_raw_fd(), 0o755) }, 0);
        }
    }

    #[test]
    fn a_memory_files_name_is_read_as_the_kernel_would() {
        let me = std::process::id();
        let name = c"lnk-name";
        let at = name.as_ptr() as u64;
        assert_eq!(memfd_name(me, at).unwrap(), b"lnk-name");
        // A null pointer: EFAULT, as the kernel answers.
        assert_eq!(memfd_name(me, 0), Err(libc::EFAULT));
        // Longer than a name can be: cut, for the kernel to refuse.
        let long = std::ffi::CString::new(vec![b'a'; 400]).unwrap();
        let read = memfd_name(me, long.as_ptr() as u64).unwrap();
        assert_eq!(read.len(), MEMFD_NAME);
        assert_eq!(sealed(&read, 0).err(), Some(libc::EINVAL));
    }

    #[test]
    fn learning_offers_what_isnt_listed_by_name_and_marks_interpreters() {
        let path = std::ffi::OsString::from("/usr/bin:/bin");
        let find = |name: &str| crate::resolve(&link_files::find_program(name, &path).unwrap());
        let (sh, env, ls) = (find("sh"), find("env"), find("ls"));
        let line = |given: &str, file: &Path| format!("{given}\t{}\n", file.display());
        let lines = [
            line("/usr/bin/sh", &sh),
            line("/usr/bin/sh", &sh),
            line("/usr/bin/env", &env),
            line("/usr/bin/ls", &ls),
            line("/opt/lnk/lnk-sandbox", Path::new("/opt/lnk/lnk-sandbox")),
            line("/work/out/tool", Path::new("/work/out/tool")),
            line("/tmp/x", Path::new("/tmp/x")),
            line(
                "/usr/lib/git-core/git-remote-https",
                Path::new("/usr/lib/git-core/git-remote-https"),
            ),
        ]
        .concat();
        let listed = [ls.clone()];
        let learned = learned(
            &lines,
            Some(&listed),
            &["/opt/lnk/lnk-sandbox".into()],
            &["/work/out".into()],
            &path,
        );
        assert_eq!(
            learned.add,
            [
                ("sh".to_string(), true),
                ("env".to_string(), false),
                ("/usr/lib/git-core/git-remote-https".to_string(), false),
            ]
        );
        assert_eq!(
            learned.writable,
            [PathBuf::from("/work/out/tool"), "/tmp/x".into()]
        );
        assert_eq!(learned.ran, 7);
        // "*": nothing to add.
        assert!(learned_any(&lines, &path).add.is_empty());
        assert!(interpreter(Path::new("/usr/bin/python3.12")));
        assert!(!interpreter(Path::new("/usr/bin/git")));
        for more in [
            "/bin/ash",
            "/usr/bin/php-cgi8.3",
            "/usr/bin/R",
            "/usr/bin/qjs",
        ] {
            assert!(interpreter(Path::new(more)), "{more}");
        }
    }

    fn learned_any(lines: &str, path: &std::ffi::OsStr) -> Learned {
        learned(lines, None, &[], &[], path)
    }

    #[test]
    fn a_scripts_interpreters_run_too() {
        let dir = std::env::temp_dir().join(format!("lnk-execs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sh =
            link_files::find_program("sh", &std::ffi::OsString::from("/usr/bin:/bin")).unwrap();
        let script = dir.join("run.sh");
        std::fs::write(&script, format!("#!{} -e\necho hi\n", sh.display())).unwrap();
        let inner = dir.join("inner");
        std::fs::write(&inner, format!("#!{}\n", script.display())).unwrap();
        let chain = interpreters(&inner);
        let files: Vec<_> = chain.iter().map(|s| s.file.clone()).collect();
        assert_eq!(
            files,
            [script.canonicalize().unwrap(), sh.canonicalize().unwrap()]
        );
        assert!(interpreters(&sh.canonicalize().unwrap()).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_fifo_or_a_link_in_a_files_place_holds_nothing_up() {
        let dir = std::env::temp_dir().join(format!("lnk-execs-fifo-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("real/sub")).unwrap();
        let fifo = dir.join("fifo");
        let path = std::ffi::CString::new(fifo.display().to_string()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        // Read at once, nothing found: never a wait for a writer.
        let started = std::time::Instant::now();
        assert!(interpreters(&fifo).is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        // A link, then `..`: where the kernel goes, through the link.
        std::fs::write(dir.join("real/tool"), "").unwrap();
        std::os::unix::fs::symlink(dir.join("real/sub"), dir.join("link")).unwrap();
        let found = program(dir.join("link/../tool")).unwrap();
        assert_eq!(found.file, dir.join("real/tool").canonicalize().unwrap());
        assert!(program(dir.join("link/../nothing")).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_link_to_a_listed_program_is_that_program() {
        let dir = std::env::temp_dir().join(format!("lnk-execs-link-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let listed = dir.join("bin/tool");
        std::fs::write(&listed, "").unwrap();
        let hard = dir.join("hard");
        std::fs::hard_link(&listed, &hard).unwrap();
        let other = dir.join("other");
        std::fs::write(&other, "").unwrap();
        let programs = [listed.clone()];
        assert!(allows(&programs, &listed));
        assert!(allows(&programs, &hard));
        assert!(!allows(&programs, &other));
        assert!(allows(&[dir.join("bin")], &listed));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The sandbox ends only once every exec taken has been reported:
    /// one still being looked at when the child exits holds it until its
    /// report is made.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn the_end_waits_for_every_exec_taken_to_be_reported() {
        let reports = Reports::new(Watch::Refusals(Vec::new()));
        let reported = std::sync::atomic::AtomicBool::new(false);
        let (taken, take) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let _in_flight = reports.taken();
                taken.send(()).unwrap();
                // Its program has gone on, failed, and the child exited,
                // while its report is still being made.
                std::thread::sleep(std::time::Duration::from_millis(300));
                reported.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            take.recv().unwrap();
            reports.settled();
            assert!(
                reported.load(std::sync::atomic::Ordering::SeqCst),
                "the sandbox ended before its report"
            );
        });
    }

    #[test]
    fn a_refusal_names_the_file_and_the_fix() {
        let started = Started {
            given: "/work/run.sh".into(),
            file: "/work/run.sh".into(),
        };
        let node = Started {
            given: "/usr/bin/node".into(),
            file: "/usr/lib/node/node".into(),
        };
        assert_eq!(
            refusal(&started, &started),
            "lnk sandbox: refused /work/run.sh: add it to [commands] allow"
        );
        assert_eq!(
            refusal(&node, &started),
            "lnk sandbox: refused /usr/bin/node (/work/run.sh's interpreter): add it to \
             [commands] allow"
        );
    }
}

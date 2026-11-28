//! A process safe to hold secrets, and its children given none but on
//! purpose:
//!
//! - [`guard`], first before secrets or threads: attempts platform process
//!   protection and, on Linux where permitted, memory locking. Inspect
//!   [`Guarded`] for what succeeded; protection is not unconditional.
//! - [`crate::Secret::from_env`] marks the variable it read as taken, and
//!   takes it out of the environment while the process has one thread
//!   (changing the environment with another thread running isn't safe).
//! - [`command`], every child: its environment the parent's but every
//!   variable taken and every one whose name says it's a secret's
//!   ([`names_a_secret`]).
//! - [`ours`], a child that's one of Link's own programs: the same, and
//!   every secret this one holds handed to it on a pipe, where its
//!   [`crate::Secret::from_env`] finds them (a key you set for `lnk`
//!   reaches the module that reads it). The pipe goes to that child alone:
//!   every child started here closes it, so it reaches no program the
//!   child starts in turn, but a program of its own not built on this
//!   package (a module from outside Link) gets it as Link's own do.
//! - [`ends_with_parent`], Linux parent-death signaling; elsewhere the
//!   caller must own child cleanup.
//! - [`stand_in_front`], a child this process waits for, its stop
//!   passed on to it, or on Linux to the group a process it started
//!   leads ([`pass_to_group_led_by`]); [`wait_in_front`] waits for it,
//!   standing aside before it's reaped.
//! - [`carried`], a program run for the one that started this one, as a
//!   wrapper runs it: the environment it was given for it, whole.
//! - [`give`], the one way to hand a child a secret in its environment,
//!   for a program that reads one nowhere else: each a reviewed
//!   place, with why.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::Secret;

#[cfg(unix)]
mod child;
#[cfg(unix)]
pub use child::OwnedChild;
#[cfg(unix)]
mod front;
#[cfg(target_os = "linux")]
pub use front::pass_to_group_led_by;
#[cfg(unix)]
pub use front::{
    Terminal, end_left, stand_aside, stand_in_front, stand_in_front_group, wait_in_front,
};
#[cfg(unix)]
mod watched;
#[cfg(unix)]
pub use watched::watched_by_parent;

/// Set (to anything) in a debug build, [`guard`] leaves the process as
/// it is, for a debugger or a test that reads its own memory. A release
/// build never reads it.
pub const DUMPABLE_ENV: &str = "LNK_SECRET_DUMPABLE";

/// What [`guard`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guarded {
    /// Whether Linux's `PR_SET_DUMPABLE` or macOS's `PT_DENY_ATTACH`
    /// succeeded. The core-file soft limit is also set to zero where
    /// possible. False on other systems, failure, or a debug bypass.
    pub undumpable: bool,
    /// Its memory, now and to come, kept out of swap: only on Linux, where
    /// the process may lock all of it, its limit on locked memory
    /// (`RLIMIT_MEMLOCK`) unlimited or `CAP_IPC_LOCK` held (a service run
    /// as root); never by a limit it raises, which its children would get.
    pub memory_locked: bool,
}

/// Attempts process protection ([`Guarded`] says what succeeded):
/// called first in a program's `main`, before it reads a secret or
/// starts a thread (a runtime's too), so every variable named like a
/// secret's leaves its environment, kept for [`crate::Secret::from_env`]. Its
/// children are their own: a program it runs is dumpable again once it
/// starts.
pub fn guard() -> Guarded {
    if crate::test_var(DUMPABLE_ENV).is_some_and(|v| !v.is_empty()) {
        return Guarded {
            undumpable: false,
            memory_locked: false,
        };
    }
    // What it was handed, read before it starts a child of its own, so
    // the pipe isn't theirs.
    handed();
    // Every variable named like a secret's, taken out of its environment
    // now, on its one thread: read later, from any thread, it's the same.
    if os::threads() == Some(1) {
        let named: Vec<String> = std::env::vars_os()
            .filter_map(|(name, _)| name.into_string().ok())
            .filter(|name| name != HANDED_ENV && names_a_secret(name))
            .collect();
        for name in named {
            take(&name);
        }
    }
    Guarded {
        undumpable: os::no_dumps(),
        memory_locked: os::lock_memory(),
    }
}

/// The variables [`Secret::from_env`] read, and what each held when it
/// was taken out of the environment: read again, it's still there.
static TAKEN: Mutex<BTreeMap<OsString, Option<Secret>>> = Mutex::new(BTreeMap::new());

fn taken() -> MutexGuard<'static, BTreeMap<OsString, Option<Secret>>> {
    TAKEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// `name`'s secret, read: marked taken, so no child is given it, and
/// taken out of the environment while this process has one thread.
pub(crate) fn take(name: &str) -> Option<Secret> {
    if let Some(given) = handed().get(name) {
        return Some(given.duplicate());
    }
    let mut taken = taken();
    let Some(value) = std::env::var_os(name) else {
        // Taken out already: what it held.
        return taken
            .get(OsStr::new(name))
            .and_then(|kept| kept.as_ref().map(Secret::duplicate));
    };
    let Ok(value) = value.into_string() else {
        // Not text: no secret read, but no child is given it either.
        taken.insert(name.into(), None);
        return None;
    };
    let value = Secret::new(value);
    let kept = if os::threads() == Some(1) {
        // SAFETY: this process has one thread, the one running here, so
        // nothing reads the environment as it changes; another starts
        // only from here.
        unsafe { std::env::remove_var(name) };
        Some(value.duplicate())
    } else {
        None
    };
    taken.insert(name.into(), kept);
    Some(value)
}

/// The environment a child is given: this process's, but every variable
/// [`Secret::from_env`] took and every one whose name says it's a
/// secret's.
pub fn environment() -> impl Iterator<Item = (OsString, OsString)> {
    let taken: Vec<OsString> = taken().keys().cloned().collect();
    std::env::vars_os().filter(move |(name, _)| {
        !taken.contains(name) && name != HANDED_ENV && !name.to_str().is_none_or(names_a_secret)
    })
}

/// The variables `wanted` names, with their values, as text: of
/// [`environment`], so never one taken or named like a secret's. How a
/// program reads the ones it means by a rule (a prefix, a list).
pub fn variables(wanted: impl Fn(&str) -> bool) -> impl Iterator<Item = (String, String)> {
    environment().filter_map(move |(name, value)| {
        let name = name.into_string().ok()?;
        wanted(&name).then_some((name, value.into_string().ok()?))
    })
}

/// A child running `program`, given [`environment`]: what every program
/// Link runs is started from.
#[expect(
    clippy::disallowed_methods,
    reason = "the one place a program Link runs is started from"
)]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.env_clear().envs(environment());
    no_pipe(&mut command);
    command
}

/// Keeps [`HANDED_FD`] held in this process, on `/dev/null` closed on
/// exec, whenever nothing handed is there: no descriptor of the
/// runtime's then lands on it, the pipe a child says a failed start on
/// among them, which [`no_pipe`] or [`hand`] in the child would close or
/// replace, a program that never ran taken for one started. Held for
/// good once taken; a pipe handed there is replaced by it once read
/// ([`read_handed`]), with no moment between. Another's file there is
/// left alone, and the child's own checks keep that case honest.
#[cfg(unix)]
fn hold_handed_fd() {
    // SAFETY: fcntl, open and close, on descriptors this process holds.
    unsafe {
        if libc::fcntl(HANDED_FD, libc::F_GETFD) >= 0 {
            return;
        }
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC);
        if null < 0 || null == HANDED_FD {
            // None to hold, or held already: it took the number itself.
            return;
        }
        // The lowest free from it: itself, unless taken meanwhile.
        let held = libc::fcntl(null, libc::F_DUPFD_CLOEXEC, HANDED_FD);
        if held >= 0 && held != HANDED_FD {
            libc::close(held);
        }
        libc::close(null);
    }
}

/// [`hold_handed_fd`] as the program loads, before `main` and any thread:
/// [`HANDED_FD`] is free then, or handed, so no file of the program's
/// own ever takes it, however many it opens. Taken only when needed
/// instead, it was often another's by then, and every pipe handed
/// refused (`EBUSY`).
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
#[used]
#[unsafe(link_section = ".init_array")]
static HOLD_AT_LOAD: extern "C" fn() = hold_at_load;

#[cfg(target_os = "macos")]
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static HOLD_AT_LOAD: extern "C" fn() = hold_at_load;

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "macos"
))]
extern "C" fn hold_at_load() {
    hold_handed_fd();
}

/// Whether descriptor `fd` is open and closed on exec, in the child
/// before it runs: the runtime's pipe that says a failed start is, and
/// a pipe this process was handed isn't.
#[cfg(unix)]
fn closed_on_exec(fd: libc::c_int) -> bool {
    // SAFETY: fcntl is async-signal-safe.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    flags >= 0 && flags & libc::FD_CLOEXEC != 0
}

/// `command`'s child closes the pipe this process may have been handed
/// on, unread ([`HANDED_FD`]), before it runs: never passed on. A
/// descriptor there closed on exec is left as it is: it's not the handed
/// pipe, and it may be the runtime's.
#[cfg(unix)]
fn no_pipe(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    hold_handed_fd();
    // SAFETY: in the child, before it runs the program: fcntl and close.
    unsafe {
        command.pre_exec(|| {
            if !closed_on_exec(HANDED_FD) {
                libc::close(HANDED_FD);
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn no_pipe(_: &mut Command) {}

/// `command`'s child is sent `signal` when this process ends, however
/// it ends (killed too), and ends at once if this one is gone before it
/// runs: on Linux (`PR_SET_PDEATHSIG`); elsewhere nothing, and the
/// child looks for its parent itself. The kernel sends it when the
/// *thread* that started the child ends, not the process: start it from
/// a thread that lives as long as the child is wanted.
pub fn ends_with_parent(command: &mut Command, signal: i32) -> &mut Command {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let parent = std::process::id() as libc::pid_t;
        // SAFETY: prctl, getppid and _exit are async-signal-safe.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, signal) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // Gone already, before the line above: so is the child.
                if libc::getppid() != parent {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = signal;
    command
}

/// A child that's one of Link's own programs (`lnk`, a module, a tool),
/// started like any ([`command`]), with every secret this process holds
/// handed to it on a pipe: what its environment had, what it read, what
/// it was handed itself. Its [`crate::Secret::from_env`] reads them there.
pub fn ours(program: impl AsRef<OsStr>) -> Command {
    ours_with(program, [])
}

/// [`ours`], with `more` handed to it too, each by its name.
pub fn ours_with<'a>(
    program: impl AsRef<OsStr>,
    more: impl IntoIterator<Item = (&'a str, &'a Secret)>,
) -> Command {
    let mut command = command(program);
    let mut held = held();
    for (name, secret) in more {
        held.insert(name.to_string(), secret.duplicate());
    }
    hand(&mut command, &held);
    command
}

/// `command`, handed `secrets` alone on a pipe, each by its name, as
/// [`ours`] hands all: for a program of Link's own given only some (a
/// sandbox's proxy, its run's secrets). After its environment is set:
/// clearing that would drop [`HANDED_ENV`].
pub fn hand_to<'a>(
    command: &mut Command,
    secrets: impl IntoIterator<Item = (&'a str, &'a Secret)>,
) {
    let secrets = secrets
        .into_iter()
        .map(|(name, secret)| (name.to_string(), secret.duplicate()))
        .collect();
    hand(command, &secrets);
}

/// Every secret this process holds by a variable's name: in its
/// environment, read from it, handed to it.
fn held() -> BTreeMap<String, Secret> {
    let mut held: BTreeMap<String, Secret> = handed()
        .iter()
        .map(|(name, secret)| (name.clone(), secret.duplicate()))
        .collect();
    for (name, kept) in taken().iter() {
        if let (Some(name), Some(kept)) = (name.to_str(), kept) {
            held.insert(name.to_string(), kept.duplicate());
        }
    }
    for (name, value) in std::env::vars_os() {
        if let (Some(name), Ok(value)) = (name.to_str(), value.into_string())
            && name != HANDED_ENV
            && names_a_secret(name)
        {
            held.entry(name.to_string())
                .or_insert_with(|| Secret::new(value));
        }
    }
    held
}

/// The variable that says a program was handed secrets, and on which
/// descriptor.
pub const HANDED_ENV: &str = "LNK_SECRETS_FD";

/// The descriptor a program's secrets are handed on: one a program
/// doesn't otherwise have open.
#[cfg(unix)]
const HANDED_FD: i32 = 60;

/// The most handed at once: what a pipe holds before it's read.
#[cfg(unix)]
const HANDED_MOST: usize = 16 * 1024;

/// What this process was handed ([`ours`]), read once.
fn handed() -> &'static BTreeMap<String, Secret> {
    static HANDED: OnceLock<BTreeMap<String, Secret>> = OnceLock::new();
    HANDED.get_or_init(read_handed)
}

/// `secrets` written into a pipe in the child, after it forks and before
/// it runs `program`: on [`HANDED_FD`], said by [`HANDED_ENV`]. Nothing
/// of it is open in this process, or in any other child.
#[cfg(unix)]
fn hand(command: &mut Command, secrets: &BTreeMap<String, Secret>) {
    use std::os::unix::process::CommandExt;
    if secrets.is_empty() {
        return;
    }
    // Its whole size first, so nothing grows and leaves a copy behind.
    let size = secrets
        .iter()
        .map(|(name, secret)| name.len() + secret.expose().len() + 2)
        .sum();
    let mut data = Secret::new(Vec::with_capacity(size));
    for (name, secret) in secrets {
        let pair = data.expose_mut();
        pair.extend_from_slice(name.as_bytes());
        pair.push(0);
        pair.extend_from_slice(secret.expose().as_bytes());
        pair.push(0);
    }
    command.env(HANDED_ENV, HANDED_FD.to_string());
    // SAFETY: in the child, before it runs the program: only pipe, fcntl,
    // fstat,
    // write, dup2 and close, on bytes made beforehand.
    unsafe {
        command.pre_exec(move || {
            let bytes = data.expose();
            if bytes.len() > HANDED_MOST {
                return Err(std::io::Error::from_raw_os_error(libc::E2BIG));
            }
            let mut fds = [0; 2];
            if libc::pipe(fds.as_mut_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let (read, write) = (fds[0], fds[1]);
            // Room for all of it (a pipe holds as little as 4 KiB where
            // its user has many), and never a wait: what doesn't fit fails
            // the start rather than holding it up for good.
            #[cfg(target_os = "linux")]
            libc::fcntl(write, libc::F_SETPIPE_SZ, bytes.len() as libc::c_int);
            if libc::fcntl(write, libc::F_SETFL, libc::O_NONBLOCK) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut written = 0;
            while written < bytes.len() {
                let n = libc::write(
                    write,
                    bytes[written..].as_ptr().cast(),
                    bytes.len() - written,
                );
                if n <= 0 {
                    return Err(std::io::Error::from_raw_os_error(libc::E2BIG));
                }
                written += n as usize;
            }
            libc::close(write);
            // Over the hold alone (`/dev/null`, a device), never another
            // descriptor there closed on exec, which may be the runtime's
            // way of saying the start failed: the start fails, saying
            // so, instead of seeming to start a program that never ran.
            let mut stat: libc::stat = std::mem::zeroed();
            if read != HANDED_FD
                && closed_on_exec(HANDED_FD)
                && (libc::fstat(HANDED_FD, &mut stat) != 0
                    || stat.st_mode & libc::S_IFMT != libc::S_IFCHR)
            {
                return Err(std::io::Error::from_raw_os_error(libc::EBUSY));
            }
            if read != HANDED_FD {
                if libc::dup2(read, HANDED_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::close(read);
            }
            Ok(())
        });
    }
}

/// Elsewhere, no pipe: in its environment.
#[cfg(not(unix))]
fn hand(command: &mut Command, secrets: &BTreeMap<String, Secret>) {
    for (name, secret) in secrets {
        command.env(name, secret.expose());
    }
}

/// What the program that started this one handed it, read from
/// [`HANDED_FD`] and closed; none if it handed nothing.
#[cfg(unix)]
fn read_handed() -> BTreeMap<String, Secret> {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    let mut handed = BTreeMap::new();
    let said = std::env::var(HANDED_ENV).ok();
    if said.as_deref() != Some(HANDED_FD.to_string().as_str()) {
        return handed;
    }
    // A pipe, or not what was handed.
    // SAFETY: a stat of a descriptor, into a buffer of its size.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    let pipe = unsafe { libc::fstat(HANDED_FD, &mut stat) } == 0
        && stat.st_mode & libc::S_IFMT == libc::S_IFIFO;
    if !pipe {
        return handed;
    }
    // SAFETY: the pipe handed to this process, read once and closed.
    let mut file = unsafe { std::fs::File::from_raw_fd(HANDED_FD) };
    // Room for the most there is, so nothing grows and leaves a copy.
    let mut data = Secret::new(Vec::with_capacity(HANDED_MOST + 64));
    if (&mut file)
        .take(HANDED_MOST as u64 + 1)
        .read_to_end(data.expose_mut())
        .is_err()
    {
        return handed;
    }
    // Replaced by the hold ([`hold_handed_fd`]), never just closed: the
    // number is never free for the runtime's pipe to land on.
    let pipe = std::os::fd::IntoRawFd::into_raw_fd(file);
    // SAFETY: open, dup2, fcntl and close, on descriptors this process
    // holds; `pipe` was taken out of its File above, so it's closed once.
    unsafe {
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC);
        if null >= 0 && libc::dup2(null, pipe) == pipe {
            libc::fcntl(pipe, libc::F_SETFD, libc::FD_CLOEXEC);
        } else {
            libc::close(pipe);
        }
        if null >= 0 {
            libc::close(null);
        }
    }
    if os::threads() == Some(1) {
        // SAFETY: one thread, this one: nothing reads the environment as
        // it changes.
        unsafe { std::env::remove_var(HANDED_ENV) };
    }
    let mut parts = data.expose().split(|b| *b == 0);
    while let (Some(name), Some(value)) = (parts.next(), parts.next()) {
        if let (Ok(name), Ok(value)) = (std::str::from_utf8(name), std::str::from_utf8(value))
            && !name.is_empty()
        {
            handed.insert(name.to_string(), Secret::new(value.to_string()));
        }
    }
    handed
}

#[cfg(not(unix))]
fn read_handed() -> BTreeMap<String, Secret> {
    BTreeMap::new()
}

/// A program this one runs for the one that started it, around it as a
/// wrapper is (a sandbox, its placeholders): with the environment it was
/// given for that program, whole, as it was before. Each call is a
/// reviewed place, with why.
#[expect(
    clippy::disallowed_methods,
    reason = "a wrapper's program, given its environment whole, each call reviewed"
)]
pub fn carried(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.env_remove(HANDED_ENV);
    no_pipe(&mut command);
    // What `guard` and `from_env` took out of this process's environment
    // was given for the program too: back in its.
    for (name, kept) in taken().iter() {
        if let Some(kept) = kept {
            command.env(name, kept.expose());
        }
    }
    command
}

/// `secret` in `command`'s environment, as `name`: for a program that
/// reads it nowhere else (a cloud's command line, a user's own). Each
/// call is a reviewed place, with why.
pub fn give<'a>(command: &'a mut Command, name: &str, secret: &Secret) -> &'a mut Command {
    command.env(name, secret.expose())
}

/// Whether a variable called `name` may hold a secret: a word of it
/// says so (`…_TOKEN`, `…_KEY`, `…_SECRET`, `…_PASSWORD`, `…_PASS`,
/// `…_AUTH`, `…_CREDENTIALS`, `…_PAT`, `…_BEARER`, `…_DSN`), or ends
/// as one does (`…_APIKEY`, `GHTOKEN`, `CLIENTSECRET`), or it's a
/// database's address (`…DATABASE_URL`); not when it says where one is
/// (`…_FILE`, `…_PATH`, `…_DIR`, `…_SOCK`, Google's credentials file,
/// git's config).
pub fn names_a_secret(name: &str) -> bool {
    const SAYS: &[&str] = &[
        "TOKEN",
        "KEY",
        "SECRET",
        "PASSWORD",
        "PASS",
        "AUTH",
        "CREDENTIAL",
        "CREDENTIALS",
        "PAT",
        "BEARER",
        "DSN",
    ];
    /// What a word holding a secret ends with, run together with what
    /// it's for (`APIKEY`).
    const ENDS: &[&str] = &["KEY", "TOKEN", "SECRET"];
    let name = name.to_ascii_uppercase();
    // Git's config, given one entry at a time (`GIT_CONFIG_KEY_0`): a
    // setting's name, not a key.
    let about = name == "GOOGLE_APPLICATION_CREDENTIALS"
        || name.starts_with("GIT_CONFIG_")
        || name
            .rsplit('_')
            .next()
            .is_some_and(|w| ["FILE", "PATH", "DIR", "SOCK"].contains(&w));
    !about
        && (name.ends_with("DATABASE_URL")
            || name
                .split('_')
                .any(|w| SAYS.contains(&w) || ENDS.iter().any(|end| w.ends_with(end))))
}

#[cfg(target_os = "linux")]
mod os {
    /// Whether it took: not dumpable, and no core file (its soft limit
    /// alone, so a program it runs may still set its own).
    pub fn no_dumps() -> bool {
        let mut core = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: plain calls on this process, with a limit that lives
        // through them.
        unsafe {
            let off = libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) == 0;
            if libc::getrlimit(libc::RLIMIT_CORE, &mut core) == 0 {
                core.rlim_cur = 0;
                libc::setrlimit(libc::RLIMIT_CORE, &core);
            }
            off
        }
    }

    /// Locked where the machine allows it whole: no limit on locked
    /// memory, or the capability to lock past it (`CAP_IPC_LOCK`, a root
    /// service's), never by raising a limit its children would inherit.
    pub fn lock_memory() -> bool {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: plain calls on this process, into a limit it owns.
        let unlimited = unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut limit) } == 0
            && limit.rlim_cur == libc::RLIM_INFINITY;
        // SAFETY: as above.
        (unlimited || can_lock())
            && unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE | libc::MCL_ONFAULT) }
                == 0
    }

    /// Whether this process holds `CAP_IPC_LOCK` (bit 14 of its effective
    /// capabilities, as `/proc/self/status` says them).
    fn can_lock() -> bool {
        const CAP_IPC_LOCK: u32 = 14;
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                let hex = status.lines().find_map(|l| l.strip_prefix("CapEff:"))?;
                u64::from_str_radix(hex.trim(), 16).ok()
            })
            .is_some_and(|caps| caps & (1 << CAP_IPC_LOCK) != 0)
    }

    pub fn threads() -> Option<usize> {
        Some(std::fs::read_dir("/proc/self/task").ok()?.count())
    }
}

#[cfg(target_os = "macos")]
mod os {
    pub fn no_dumps() -> bool {
        let mut core = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: plain calls on this process, with a limit that lives
        // through them.
        unsafe {
            if libc::getrlimit(libc::RLIMIT_CORE, &mut core) == 0 {
                core.rlim_cur = 0;
                libc::setrlimit(libc::RLIMIT_CORE, &core);
            }
            libc::ptrace(libc::PT_DENY_ATTACH, 0, std::ptr::null_mut(), 0) == 0
        }
    }

    pub fn lock_memory() -> bool {
        false
    }

    pub fn threads() -> Option<usize> {
        let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        // SAFETY: this process's task info, into a buffer of its size.
        let read = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDTASKINFO,
                0,
                (&raw mut info).cast(),
                size,
            )
        };
        (read == size).then_some(info.pti_threadnum as usize)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod os {
    pub fn no_dumps() -> bool {
        false
    }

    pub fn lock_memory() -> bool {
        false
    }

    pub fn threads() -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a test of descriptors' layout runs in a process of its own
    /// (`LAYOUT`): no other test's descriptors move them meanwhile.
    #[cfg(unix)]
    const LAYOUT: &str = "LNK_TEST_DESCRIPTOR_LAYOUT";

    /// Runs `scenario` alone, in this test program started again; true
    /// where this is that process, so the caller runs it and returns.
    #[cfg(unix)]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn alone(test: &str, scenario: &str) -> bool {
        if std::env::var(LAYOUT).as_deref() == Ok(scenario) {
            return true;
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(LAYOUT, scenario)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && said.contains("1 passed"),
            "{scenario}: {said}"
        );
        false
    }

    /// `/dev/null` opened until it's given descriptor `fd`, every one
    /// below it held open too.
    #[cfg(unix)]
    fn open_through(fd: libc::c_int) -> Vec<std::fs::File> {
        let mut opened = Vec::new();
        loop {
            let file = std::fs::File::open("/dev/null").unwrap();
            let at = std::os::fd::AsRawFd::as_raw_fd(&file);
            opened.push(file);
            if at >= fd {
                return opened;
            }
        }
    }

    /// Both ways a program is started, of one that isn't there.
    #[cfg(unix)]
    fn missing() -> [Command; 2] {
        [
            command("/nonexistent/lnk-program"),
            ours_with(
                "/nonexistent/lnk-program",
                [("A_SECRET", &Secret::new("x".to_string()))],
            ),
        ]
    }

    /// A program that isn't there fails to start, however this process's
    /// descriptors lie as it's started: never taken for started because
    /// the child closed or replaced, at the handed descriptor, the pipe
    /// it says so on.
    #[cfg(unix)]
    #[test]
    fn a_missing_program_fails_to_start_whatever_is_open() {
        let test = "process::tests::a_missing_program_fails_to_start_whatever_is_open";
        let scenarios = ["below", "through", "another-closed"];
        let Some(scenario) = scenarios.iter().find(|s| alone(test, s)) else {
            return;
        };
        let mut commands = missing();
        match *scenario {
            // Every descriptor below the handed one open but the one just
            // under it: a pipe takes that and the handed one's.
            "below" => {
                let mut opened = open_through(HANDED_FD - 1);
                opened.pop();
                for command in &mut commands {
                    assert!(command.spawn().is_err(), "a missing program started");
                }
            }
            // Every one through the one under it open, then that one
            // closed between the command made and started.
            "through" => {
                let mut opened = open_through(HANDED_FD - 1);
                let mut commands = missing();
                opened.pop();
                for command in &mut commands {
                    assert!(command.spawn().is_err(), "a missing program started");
                }
            }
            // Another's file at the handed descriptor as the command is
            // made, closed before it's started.
            _ => {
                let mut opened = open_through(HANDED_FD - 1);
                let other = std::fs::File::open("/dev/null").unwrap();
                // SAFETY: dup2 onto a descriptor number, the result checked.
                assert_eq!(
                    unsafe { libc::dup2(std::os::fd::AsRawFd::as_raw_fd(&other), HANDED_FD) },
                    HANDED_FD
                );
                let mut commands = missing();
                // SAFETY: the descriptor dup2 made, closed once.
                unsafe { libc::close(HANDED_FD) };
                opened.pop();
                for command in &mut commands {
                    assert!(command.spawn().is_err(), "a missing program started");
                }
            }
        }
    }

    /// What a program is handed still reaches it, at the handed
    /// descriptor, however this process's descriptors lie.
    #[cfg(unix)]
    #[test]
    fn a_secret_handed_reaches_the_program() {
        let test = "process::tests::a_secret_handed_reaches_the_program";
        if !alone(test, "handed") {
            return;
        }
        let _opened = open_through(HANDED_FD - 2);
        let output = ours_with(
            "/bin/sh",
            [("A_SECRET", &Secret::new("handed-over".to_string()))],
        )
        .args(["-c", "tr '\\0' '\\n' < /dev/fd/60"])
        .output()
        .unwrap();
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(said.contains("A_SECRET\nhanded-over"), "{said:?}");
    }

    /// What a program is handed reaches it however many files this
    /// process held before handing it, none of them a device: the handed
    /// descriptor was held for it as the program loaded, so none of them
    /// took it.
    #[cfg(unix)]
    #[test]
    fn a_secret_is_handed_however_many_files_are_open() {
        let test = "process::tests::a_secret_is_handed_however_many_files_are_open";
        if !alone(test, "busy") {
            return;
        }
        let exe = std::env::current_exe().unwrap();
        let _opened: Vec<_> = (0..HANDED_FD + 8)
            .map(|_| std::fs::File::open(&exe).unwrap())
            .collect();
        let output = ours_with(
            "/bin/sh",
            [("A_SECRET", &Secret::new("handed-over".to_string()))],
        )
        .args(["-c", "tr '\\0' '\\n' < /dev/fd/60"])
        .output()
        .unwrap();
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(said.contains("A_SECRET\nhanded-over"), "{said:?}");
    }

    /// A child that ends with its parent ends once the thread that
    /// started it does, the process still running.
    #[cfg(target_os = "linux")]
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_child_ends_with_the_thread_that_started_it() {
        let mut child = std::thread::spawn(|| {
            let mut sleep = Command::new("sleep");
            sleep.arg("30");
            ends_with_parent(&mut sleep, libc::SIGTERM);
            sleep.spawn().unwrap()
        })
        .join()
        .unwrap();
        let start = std::time::Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(libc::SIGTERM));
    }

    #[test]
    fn names_say_whether_they_hold_a_secret() {
        for name in [
            "LNK_TOKEN",
            "ANTHROPIC_API_KEY",
            "LNK_HOOK_AUTH",
            "CLOUD_CREDENTIALS",
            "DATABASE_URL",
            "lnk_secret",
            // A word ending as one does.
            "OPENAI_APIKEY",
            "GHTOKEN",
            "AZURE_CLIENTSECRET",
            // Words of their own.
            "GH_PAT",
            "HOOK_BEARER",
            "SENTRY_DSN",
        ] {
            assert!(names_a_secret(name), "{name}");
        }
        for name in [
            "HOME",
            "PATH",
            "SSH_AUTH_SOCK",
            "LNK_GCS_TOKEN_FILE",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "TOKENIZERS_PARALLELISM",
            "LNK_AGENT",
            "GIT_CONFIG_KEY_0",
            "MAX_TOKENS",
            "PATCH_LEVEL",
            // Where one is, ending as one does.
            "LNK_APIKEY_FILE",
            "SENTRY_DSN_PATH",
        ] {
            assert!(!names_a_secret(name), "{name}");
        }
    }

    /// A test runs on threads: the variable stays, but no child is
    /// given it, nor any other named for a secret.
    #[test]
    fn a_child_is_given_no_secret() {
        // SAFETY: these tests alone set or read these variables.
        unsafe {
            std::env::set_var("LNK_PROCESS_TEST_TOKEN", "hunter2");
            std::env::set_var("LNK_PROCESS_TEST_TAKEN", "taken-2");
            std::env::set_var("LNK_PROCESS_TEST_PLAIN", "plain");
        }
        let read = Secret::from_env("LNK_PROCESS_TEST_TAKEN").unwrap();
        assert_eq!(read.expose(), "taken-2");
        let out = command("env").output().unwrap();
        let env = String::from_utf8(out.stdout).unwrap();
        assert!(env.contains("LNK_PROCESS_TEST_PLAIN=plain"), "{env}");
        assert!(
            !env.contains("hunter2") && !env.contains("taken-2"),
            "{env}"
        );
        // Given on purpose, it's there.
        let mut given = command("env");
        give(&mut given, "LNK_PROCESS_TEST_GIVEN", &read);
        let env = String::from_utf8(given.output().unwrap().stdout).unwrap();
        assert!(env.contains("LNK_PROCESS_TEST_GIVEN=taken-2"), "{env}");
    }
}

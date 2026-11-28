//! What kind of test program this is, declared at its root.
//!
//! Every program Cargo builds for `cargo test` says its kind with
//! [`program!`](crate::program!), at its root:
//!
//! - **`program!(unit)`:** its tests run as libtest runs them, side by
//!   side as threads of one process. They share that process: its
//!   memory, its statics, its descriptors and its environment. A unit
//!   test starts no program and opens no socket, so it never meets
//!   another test through them.
//! - **`program!(system)`:** each test runs in a process of its own, the
//!   test program started again for that one test ([`run`]). Its home
//!   and runtime folder are its own and new. Everything it
//!   starts is ended when it ends, and a test that hangs is ended at its
//!   deadline, naming itself. A system test starts programs, servers
//!   and sandboxes freely: no other test shares its process.
//!
//! A system program started for one test by its whole name (`--exact`
//! and one name), as a person reruns a failure or a test reruns its own
//! program as a fixture, is that test alone in its process already, and
//! libtest runs it as usual.
//!
//! Either kind gives the program a home of its own ([`crate::home`]) and
//! keeps the descriptors it inherited, such as Cargo's build pipes, out of
//! every program a test starts.

#[cfg(unix)]
pub mod run;

/// The two kinds of test program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Tests side by side in one process, starting no program.
    Unit,
    /// Each test in a process of its own.
    System,
}

/// Declares the test program's kind at its root, `unit` or `system`
/// ([`Kind`]). A crate's unit tests hold it at the crate's root, under
/// `#[cfg(test)]`, and each integration test program at its own root:
///
/// ```ignore
/// #[cfg(test)]
/// link_test_kit::program!(unit);
/// ```
#[macro_export]
macro_rules! program {
    (unit) => {
        $crate::program!(@start $crate::program::Kind::Unit);
    };
    (system) => {
        $crate::program!(@start $crate::program::Kind::System);
    };
    (@start $kind:expr) => {
        // Run by the system's loader as the program starts, as a C
        // program's constructor is: the one moment no thread can read
        // the environment while it changes, and before libtest's `main`.
        #[used]
        #[cfg_attr(
            any(target_os = "linux", target_os = "android", target_os = "freebsd"),
            unsafe(link_section = ".init_array")
        )]
        #[cfg_attr(
            target_vendor = "apple",
            unsafe(link_section = "__DATA,__mod_init_func")
        )]
        #[cfg_attr(windows, unsafe(link_section = ".CRT$XCU"))]
        static LINK_TEST_KIT_PROGRAM: extern "C" fn() = {
            extern "C" fn start() {
                $crate::program::start($kind);
            }
            start
        };
    };
}

/// The variable naming the one test a system program's process runs,
/// set by [`run`] on each test's process: a process holding it is libtest
/// as usual, running that test.
pub const ONE: &str = "LNK_TEST_ONE";

/// The variable asking a test program its kind: set, the program prints
/// `unit` or `system` and exits, as `cargo test-copies` asks.
pub const KIND: &str = "LNK_TEST_KIND";

/// The variable asking a system program to run its one selected test as
/// this many copies at once, through its runner, as `cargo test-copies`
/// does: the copies run as wide as the suite's tests, each in a process
/// of its own.
pub const COPIES: &str = "LNK_TEST_COPIES";

/// Gives the program its home and keeps its inherited descriptors from
/// its children; a system program not started for one test then runs
/// each test in a process of its own and exits. Called once, by
/// [`program!`](crate::program!), before `main`.
pub fn start(kind: Kind) {
    if std::env::var_os(KIND).is_some() {
        println!(
            "{}",
            match kind {
                Kind::Unit => "unit",
                Kind::System => "system",
            }
        );
        std::process::exit(0);
    }
    crate::home::own();
    keep_inherited();
    #[cfg(unix)]
    if kind == Kind::System
        && std::env::var_os(ONE).is_none()
        && (std::env::var_os(COPIES).is_some() || !run::one_named())
    {
        std::process::exit(run::run());
    }
    #[cfg(not(unix))]
    if kind == Kind::System {
        compile_error!("link-test-kit: no Windows implementation of program!(system)");
    }
}

/// How [`run`] runs each test: [`program.toml`](program.toml), read once.
#[derive(serde::Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The longest a test's process runs, in seconds, before scaling.
    pub deadline: u64,
    /// When a test still running is named, in seconds.
    pub slow: u64,
    /// How long a finished test's output is read past its end, in seconds.
    pub drain: u64,
    /// How many system tests run at once for each CPU.
    pub per_cpu: usize,
    /// Below this many CPUs, waits scale by it over the machine's CPUs.
    pub scale_cpus: usize,
}

/// The machine's CPUs, as the system gives them to this process.
pub fn cpus() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// [`Settings`], read once.
pub fn settings() -> &'static Settings {
    static SETTINGS: std::sync::OnceLock<Settings> = std::sync::OnceLock::new();
    SETTINGS.get_or_init(|| {
        toml::from_str(include_str!("program.toml")).expect("the test kit's program.toml reads")
    })
}

/// Marks every descriptor past the standard three close-on-exec. Cargo
/// hands a test program its jobserver's pipe, and a program a test starts
/// would otherwise hold it, and any other descriptor the run inherited,
/// for as long as it lives. A descriptor the test opens itself keeps the
/// flag it was given.
fn keep_inherited() {
    #[cfg(target_os = "linux")]
    let open: Vec<i32> = std::fs::read_dir("/proc/self/fd")
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    #[cfg(all(unix, not(target_os = "linux")))]
    // SAFETY: getdtablesize reads this process's descriptor limit.
    let open: Vec<i32> = (0..unsafe { libc::getdtablesize() }.min(4096)).collect();
    #[cfg(unix)]
    for fd in open.into_iter().filter(|fd| *fd > 2) {
        // SAFETY: fcntl reads and sets one descriptor's flags; a closed
        // one (the listing's own) answers an error, which changes nothing.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn the_settings_read() {
        let settings = super::settings();
        assert!(settings.slow < settings.deadline, "{settings:?}");
    }

    /// A program a test starts ends on the Ctrl-C it's sent, though this
    /// program was started with Ctrl-C ignored, as a shell without job
    /// control starts a job in the background: the child's environment
    /// gives it back its default ([`crate::env::command`]).
    #[test]
    fn an_ignored_ctrl_c_reaches_no_child() {
        const NAME: &str = "program::tests::an_ignored_ctrl_c_reaches_no_child";
        let home = crate::TempDir::new("ignored-ctrl-c");
        if std::env::var_os("LNK_TEST_IGNORED").is_some() {
            let child =
                crate::ChildGroup::spawn(crate::env::command("/bin/sleep", &home).arg("30"))
                    .unwrap();
            child.interrupt();
            let status = crate::wait::eventually(
                "the interrupted sleep to end",
                std::time::Duration::from_secs(5),
                || child.try_status().unwrap(),
            );
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(status.signal(), Some(libc::SIGINT));
            return;
        }
        let mut command = crate::env::command(std::env::current_exe().unwrap(), &home);
        command
            .args(["--exact", NAME, "--nocapture"])
            .env("LNK_TEST_IGNORED", "1");
        // SAFETY: only signal between fork and exec, as a shell's
        // background job is started.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(&mut command, || {
                libc::signal(libc::SIGINT, libc::SIG_IGN);
                Ok(())
            });
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A descriptor this program inherited without close-on-exec reaches
    /// no program a test starts: the kit marked it before `main`.
    #[test]
    fn an_inherited_descriptor_reaches_no_child() {
        const NAME: &str = "program::tests::an_inherited_descriptor_reaches_no_child";
        let home = crate::TempDir::new("inherited");
        if let Ok(fd) = std::env::var("LNK_TEST_INHERITED") {
            let number: i32 = fd.parse().unwrap();
            // SAFETY: reads one descriptor's flags.
            let flags = unsafe { libc::fcntl(number, libc::F_GETFD) };
            assert!(flags >= 0, "descriptor {fd} wasn't inherited");
            assert_ne!(flags & libc::FD_CLOEXEC, 0, "descriptor {fd} wasn't marked");
            let probe = home.path().join("probe");
            crate::write_program(
                &probe,
                format!("#!/bin/sh\nif [ -e /dev/fd/{fd} ]; then echo held; else echo free; fi\n"),
            )
            .unwrap();
            let output = crate::env::command(&probe, &home).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "free");
            return;
        }
        let (reader, writer) = std::io::pipe().unwrap();
        let fd = {
            use std::os::fd::AsRawFd;
            writer.as_raw_fd()
        };
        // SAFETY: clears close-on-exec on a descriptor this test owns, so
        // the program started below inherits it, as Cargo's pipe is.
        unsafe { libc::fcntl(fd, libc::F_SETFD, 0) };
        let output = crate::env::command(std::env::current_exe().unwrap(), &home)
            .args(["--exact", NAME, "--nocapture"])
            .env("LNK_TEST_INHERITED", fd.to_string())
            .output()
            .unwrap();
        drop((reader, writer));
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

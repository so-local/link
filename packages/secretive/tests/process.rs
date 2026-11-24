//! `process`, on a process of its own and one thread, as a program's
//! `main` runs it: no harness, so no other thread.

link_test_kit::program!(system);

use link_secretive::Secret;
use link_secretive::process::{self, DUMPABLE_ENV};
use link_test_kit::libtest::{Counts, Selection};

#[derive(clap::Parser)]
struct Cli {
    #[command(subcommand)]
    child: Option<Child>,
    #[command(flatten)]
    selection: Selection,
}
#[derive(clap::Subcommand)]
enum Child {
    Handed,
    LeftDumpable,
    InFront {
        #[arg(value_parser = ["passed", "reaches"], default_value = "reaches")]
        terminal: String,
    },
}

fn main() {
    let given: Cli = link_spec::entry::plain();
    if matches!(given.child, Some(Child::Handed)) {
        // One of Link's own programs, started by `ours`: its secrets on
        // the pipe, none in its environment.
        process::guard();
        // Read and closed as it started, and the variable that said so gone.
        assert!(std::env::var_os(process::HANDED_ENV).is_none());
        assert!(std::env::var_os("LNK_PROCESS_HANDED_TOKEN").is_none());
        assert!(std::env::var_os("LNK_PROCESS_MORE_KEY").is_none());
        let token = Secret::from_env("LNK_PROCESS_HANDED_TOKEN").unwrap();
        assert_eq!(token.expose(), "handed-1");
        let more = Secret::from_env("LNK_PROCESS_MORE_KEY").unwrap();
        assert_eq!(more.expose(), "more-2");
        // A program it runs that isn't Link's sees no pipe and no secret.
        #[cfg(target_os = "linux")]
        {
            let out = process::command("sh")
                .args(["-c", "ls /proc/self/fd; env"])
                .output()
                .unwrap()
                .stdout;
            let out = String::from_utf8(out).unwrap();
            assert!(!out.lines().any(|l| l == "60"), "{out}");
            assert!(
                !out.contains("handed-1") && !out.contains("more-2"),
                "{out}"
            );
            assert!(!out.contains(process::HANDED_ENV), "{out}");
            // Nor one it runs as a wrapper does, its environment whole.
            let out = process::carried("sh")
                .args(["-c", "ls /proc/self/fd; env"])
                .output()
                .unwrap()
                .stdout;
            let out = String::from_utf8(out).unwrap();
            assert!(!out.lines().any(|l| l == "60"), "{out}");
            assert!(!out.contains(process::HANDED_ENV), "{out}");
        }
        return;
    }
    if matches!(given.child, Some(Child::LeftDumpable)) {
        // Run again with `DUMPABLE_ENV`: a debug build leaves it as it is.
        let guarded = process::guard();
        assert_eq!(guarded.undumpable, !cfg!(debug_assertions));
        #[cfg(target_os = "linux")]
        assert_eq!(dumpable(), i32::from(!guarded.undumpable));
        return;
    }
    #[cfg(unix)]
    if let Some(Child::InFront { terminal }) = given.child {
        // Stands in front of a `sleep`, the terminal's signals as asked,
        // and ends as it did.
        let terminal = if terminal == "passed" {
            process::Terminal::Passed
        } else {
            process::Terminal::Reaches
        };
        // A run started under nohup, or in the background, ignores these,
        // and a child inherits what's ignored: as from a terminal, the
        // child starts with each signal's default.
        for signal in [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM] {
            // SAFETY: one thread, before the child starts.
            unsafe { libc::signal(signal, libc::SIG_DFL) };
        }
        let mut child = process::command("sleep").arg("60").spawn().unwrap();
        process::stand_in_front(child.id(), terminal);
        println!("{}", child.id());
        let status = child.wait().unwrap();
        let signal = std::os::unix::process::ExitStatusExt::signal(&status).unwrap_or(0);
        std::process::exit(100 + signal);
    }
    let selection = given.selection;
    let tests: &[(&str, fn())] = &[
        (
            "a_variable_read_on_one_thread_leaves_the_environment",
            a_variable_read_on_one_thread_leaves_the_environment,
        ),
        (
            "a_guarded_process_dumps_nothing",
            a_guarded_process_dumps_nothing,
        ),
        (
            "debug_builds_may_be_left_dumpable",
            debug_builds_may_be_left_dumpable,
        ),
        (
            "our_programs_are_handed_secrets_on_a_pipe",
            our_programs_are_handed_secrets_on_a_pipe,
        ),
        #[cfg(unix)]
        (
            "a_stop_is_passed_to_the_child_in_front",
            a_stop_is_passed_to_the_child_in_front,
        ),
    ];
    let selected: Vec<_> = tests
        .iter()
        .filter(|(name, _)| selection.matches(name))
        .collect();
    if selection.list {
        for (name, _) in &selected {
            println!("{name}: test");
        }
        println!("\n{} tests, 0 benchmarks", selected.len());
        return;
    }
    let started = std::time::Instant::now();
    let mut counts = Counts {
        filtered: tests.len() - selected.len(),
        ..Counts::default()
    };
    println!("\nrunning {} tests", selected.len());
    for (index, (name, test)) in selected.iter().enumerate() {
        match std::panic::catch_unwind(test) {
            Ok(()) => {
                counts.passed += 1;
                println!("test {name} ... ok");
            }
            Err(panic) => {
                counts.failed += 1;
                println!("test {name} ... FAILED");
                for (name, _) in &selected[index + 1..] {
                    counts.ignored += 1;
                    println!("test {name} ... ignored, not run after earlier failure");
                }
                println!("\n{}", counts.summary(started.elapsed()));
                std::panic::resume_unwind(panic);
            }
        }
    }
    println!("\n{}", counts.summary(started.elapsed()));
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_variable_read_on_one_thread_leaves_the_environment() {
    // SAFETY: one thread, this one.
    unsafe { std::env::set_var("LNK_PROCESS_MAIN_TOKEN", "hunter2\n") };
    let read = Secret::from_env("LNK_PROCESS_MAIN_TOKEN").unwrap();
    assert_eq!(read.expose(), "hunter2");
    assert!(std::env::var_os("LNK_PROCESS_MAIN_TOKEN").is_none());
    // Read again, it's the same.
    let again = Secret::from_env("LNK_PROCESS_MAIN_TOKEN").unwrap();
    assert_eq!(again.expose(), "hunter2");
    // A child of any kind sees none of it.
    let env = std::process::Command::new("env").output().unwrap().stdout;
    assert!(!String::from_utf8(env).unwrap().contains("hunter2"));
}

fn a_guarded_process_dumps_nothing() {
    // SAFETY: one thread, this one.
    unsafe { std::env::set_var("LNK_PROCESS_LATER_SECRET", "later-3") };
    let guarded = process::guard();
    // Every variable named like a secret's is out of its environment, and
    // read later, from another thread, it's the same.
    assert!(std::env::var_os("LNK_PROCESS_LATER_SECRET").is_none());
    let later = std::thread::spawn(|| Secret::from_env("LNK_PROCESS_LATER_SECRET"))
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(later.expose(), "later-3");
    assert!(guarded.undumpable);
    #[cfg(target_os = "linux")]
    {
        assert_eq!(dumpable(), 0);
        let mut core = libc::rlimit {
            rlim_cur: 1,
            rlim_max: 1,
        };
        // SAFETY: a plain call, into a limit this owns.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut core) }, 0);
        // Its soft limit: a program it runs may still set its own.
        assert_eq!(core.rlim_cur, 0);
    }
    // Its memory is locked only where the machine allows it whole.
    #[cfg(target_os = "linux")]
    {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: as above.
        unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut limit) };
        let unlimited = limit.rlim_cur == libc::RLIM_INFINITY;
        // Or past its limit, holding CAP_IPC_LOCK (bit 14), as root does.
        let can_lock = std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|l| l.strip_prefix("CapEff:"))
            .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok())
            .is_some_and(|caps| caps & (1 << 14) != 0);
        assert_eq!(guarded.memory_locked, unlimited || can_lock);
        // Its limit is as it was: no program it runs gets more.
        let mut after = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: as above.
        unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut after) };
        assert_eq!(
            (after.rlim_cur, after.rlim_max),
            (limit.rlim_cur, limit.rlim_max)
        );
    }
}

fn our_programs_are_handed_secrets_on_a_pipe() {
    // SAFETY: one thread, this one.
    unsafe { std::env::set_var("LNK_PROCESS_HANDED_TOKEN", "handed-1") };
    let more = Secret::new("more-2".to_string());
    let status = process::ours_with(
        std::env::current_exe().unwrap(),
        [("LNK_PROCESS_MORE_KEY", &more)],
    )
    .arg("handed")
    .status()
    .unwrap();
    assert!(status.success());
    // More than a pipe holds before it's read fails the start, at once,
    // rather than holding it up for good.
    let most = Secret::new("x".repeat(64 * 1024));
    let started = std::time::Instant::now();
    let refused = process::ours_with("true", [("LNK_PROCESS_MOST_KEY", &most)]).status();
    assert!(refused.is_err(), "{refused:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn debug_builds_may_be_left_dumpable() {
    let me = std::env::current_exe().unwrap();
    let status = std::process::Command::new(me)
        .arg("left-dumpable")
        .env(DUMPABLE_ENV, "1")
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(target_os = "linux")]
fn dumpable() -> i32 {
    // SAFETY: a plain call on this process.
    unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) }
}

/// A stop sent to the process in front reaches its child, and the
/// terminal's own only when asked: otherwise the one in front ignores it
/// and outlives it.
#[cfg(unix)]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_stop_is_passed_to_the_child_in_front() {
    use std::io::BufRead;
    for (terminal, stop, ends_by) in [
        ("reaches", libc::SIGTERM, libc::SIGTERM),
        ("reaches", libc::SIGHUP, libc::SIGHUP),
        ("passed", libc::SIGINT, libc::SIGINT),
        ("passed", libc::SIGQUIT, libc::SIGQUIT),
        // Ignored in front: the SIGTERM after it is what ends the child.
        ("reaches", libc::SIGINT, libc::SIGTERM),
    ] {
        let mut front = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["in-front", terminal])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(front.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let child: libc::pid_t = line.trim().parse().unwrap();
        let front_pid = front.id() as libc::pid_t;
        // SAFETY: signals to the process this test started.
        unsafe { libc::kill(front_pid, stop) };
        if ends_by != stop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            assert!(
                front.try_wait().unwrap().is_none(),
                "{terminal}: ended by {stop}"
            );
            unsafe { libc::kill(front_pid, ends_by) };
        }
        let status = front.wait().unwrap();
        assert_eq!(
            status.code(),
            Some(100 + ends_by),
            "{terminal} {stop}: {status:?}"
        );
        // SAFETY: a null signal, only asking whether it's there.
        assert_ne!(
            unsafe { libc::kill(child, 0) },
            0,
            "{terminal} {stop}: child left"
        );
    }
}

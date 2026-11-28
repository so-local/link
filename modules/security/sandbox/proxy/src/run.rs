//! The program a proxy runs: its sandbox, with the limits it had.

use anyhow::Context;
#[cfg(target_os = "linux")]
use anyhow::ensure;
#[cfg(target_os = "linux")]
use tokio::net::TcpListener;

#[cfg(target_os = "linux")]
use crate::filter::Filter;

/// This process's limit of open files before [`raise_open_files`], which
/// the program it runs gets back.
static OPEN_FILES: std::sync::OnceLock<libc::rlimit> = std::sync::OnceLock::new();

/// Raises this process's limit of open files to its hard limit (macOS:
/// at most 10240, the most it takes), since each proxied connection
/// holds two or three: a Mac's default of 256 is a few dozen downloads
/// at once.
pub fn raise_open_files() {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a valid rlimit for the call to fill.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return;
    }
    let before = limit;
    let most = match cfg!(target_os = "macos") {
        true => limit.rlim_max.min(10240),
        false => limit.rlim_max,
    };
    if most <= limit.rlim_cur {
        return;
    }
    limit.rlim_cur = most;
    // SAFETY: `limit` is a valid rlimit.
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) } == 0 {
        let _ = OPEN_FILES.set(before);
    }
}

/// Runs `argv`, the program this proxy is in front of, and returns its
/// exit code. A stop sent here (SIGTERM, SIGHUP) is passed on to it, the
/// terminal's own reach it directly, and it's sent SIGTERM should this
/// process end first: with no sandbox around it, nothing else would
/// stop it with its proxy.
pub async fn run(argv: &[String]) -> anyhow::Result<i32> {
    run_at(argv, &Default::default()).await
}

/// A loopback proxy's program, with its bound port filled in the
/// environment its caller supplied for it and its generated variables.
pub async fn run_at(
    argv: &[String],
    env: &std::collections::BTreeMap<String, String>,
) -> anyhow::Result<i32> {
    let mut command = command(argv)?;
    command.envs(env);
    link_secretive::process::ends_with_parent(command.as_std_mut(), libc::SIGTERM);
    let mut child = command
        .spawn()
        .with_context(|| format!("running {}", argv[0]))?;
    let _group = Group::of(child.id().context("the program has no process ID")?);
    let status = child
        .wait()
        .await
        .with_context(|| format!("running {}", argv[0]))?;
    Ok(status.code().unwrap_or(1))
}

/// Every run has a group of its own. Its remaining children are killed
/// before the proxy releases its port, even on an error or cancellation.
/// A terminal run is put in the foreground and the terminal restored
/// afterwards; a child that reached the terminal first is continued.
struct Group {
    id: libc::pid_t,
    terminal: Option<libc::pid_t>,
}

impl Group {
    fn of(id: u32) -> Self {
        link_secretive::process::stand_in_front_group(
            id,
            link_secretive::process::Terminal::Passed,
        );
        let id = id as libc::pid_t;
        // SAFETY: these calls only inspect process/terminal identity.
        let ours = unsafe { libc::getpgrp() };
        let foreground = unsafe { libc::tcgetpgrp(libc::STDIN_FILENO) };
        let terminal = (foreground == ours && foreground > 1).then_some(ours);
        if terminal.is_some() {
            foreground_group(id);
            // SAFETY: id is a group created for this child, never 0 or 1.
            unsafe { libc::kill(-id, libc::SIGCONT) };
        }
        Self { id, terminal }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if self.id > 1 {
            // SAFETY: only this run's group. In particular, never group -1.
            unsafe { libc::kill(-self.id, libc::SIGKILL) };
            // Its leader reaped, where it isn't yet: only once it has ended
            // is what it left running this process's child, to end below.
            // SAFETY: waits for this run's own child, the group's leader;
            // one already reaped answers at once.
            unsafe { libc::waitpid(self.id, std::ptr::null_mut(), 0) };
        }
        // And what its program left outside its group, handed here.
        link_secretive::process::end_left(link_sandbox_contract::constants::BUBBLEWRAP);
        if let Some(ours) = self.terminal {
            foreground_group(ours);
        }
    }
}

fn foreground_group(group: libc::pid_t) {
    // Block SIGTTOU on this thread while changing the terminal's owner:
    // restoring it is otherwise a background terminal operation.
    // SAFETY: every signal set is initialized before use and restored.
    unsafe {
        let mut blocked: libc::sigset_t = std::mem::zeroed();
        let mut before: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut blocked);
        libc::sigaddset(&mut blocked, libc::SIGTTOU);
        if libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut before) == 0 {
            libc::tcsetpgrp(libc::STDIN_FILENO, group);
            libc::pthread_sigmask(libc::SIG_SETMASK, &before, std::ptr::null_mut());
        }
    }
}

/// `argv`, bubblewrap's command at `bwrap`, run with a resolver in the sandbox's
/// network: bubblewrap waits (`--block-fd`) until this proxy listens on
/// its `127.0.0.1:53` ([`crate::dns`]), answering by `filter`. Without it
/// (a kernel or a machine that won't), the sandbox runs with no DNS, as
/// it would without `tcp`, and this says why.
#[cfg(target_os = "linux")]
pub async fn run_with_dns(argv: &[String], bwrap: usize, filter: Filter) -> anyhow::Result<i32> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let pipe = || -> anyhow::Result<(OwnedFd, OwnedFd)> {
        let mut fds = [0; 2];
        // SAFETY: pipe2 fills two descriptors, which we own from here.
        ensure!(
            unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0,
            "a pipe"
        );
        Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
    };
    let (info_read, info_write) = pipe()?;
    let (block_read, block_write) = pipe()?;
    let (info_fd, block_fd) = (info_write.as_raw_fd(), block_read.as_raw_fd());
    let mut argv = argv.to_vec();
    argv.splice(
        bwrap + 1..bwrap + 1,
        [
            "--info-fd".to_string(),
            info_fd.to_string(),
            "--block-fd".to_string(),
            block_fd.to_string(),
        ],
    );
    let mut command = command(&argv)?;
    // SAFETY: fcntl is async-signal-safe: bubblewrap's ends, open across
    // its exec.
    unsafe {
        command.pre_exec(move || {
            libc::fcntl(info_fd, libc::F_SETFD, 0);
            libc::fcntl(block_fd, libc::F_SETFD, 0);
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("running {}", argv[0]))?;
    let _group = Group::of(child.id().context("the program has no process ID")?);
    drop((info_write, block_read));
    let sockets = tokio::task::spawn_blocking(move || {
        let sockets = crate::dns::sockets(std::fs::File::from(info_read));
        // Whatever came of it, the sandbox goes on.
        let _ = std::io::Write::write_all(&mut std::fs::File::from(block_write), b"x");
        sockets
    })
    .await?;
    let sockets = sockets.and_then(|(udp, tcp)| {
        Ok((
            tokio::net::UdpSocket::from_std(udp)?,
            TcpListener::from_std(tcp)?,
        ))
    });
    match sockets {
        Ok((udp, tcp)) => {
            let servers = crate::dns::nameservers();
            tokio::spawn(crate::dns::serve_udp(udp, filter, servers.clone()));
            tokio::spawn(crate::dns::serve_tcp(tcp, filter, servers));
        }
        Err(err) => link_format::aside(
            link_format::Report::warning(link_format::sentence!("There is no DNS in the sandbox."))
                .why(link_format::Prose::from(link_format::Sentence::adopt(
                    &format!("{err:#}"),
                ))),
        ),
    }
    let status = child
        .wait()
        .await
        .with_context(|| format!("running {}", argv[0]))?;
    Ok(status.code().unwrap_or(1))
}

/// Whether a program started now would own this process's terminal: its
/// input is one, and this process's group is the terminal's foreground,
/// as an interactive run is.
pub fn owns_terminal() -> bool {
    // SAFETY: these calls only inspect process and terminal identity.
    unsafe {
        libc::isatty(libc::STDIN_FILENO) == 1
            && libc::tcgetpgrp(libc::STDIN_FILENO) == libc::getpgrp()
    }
}

/// On macOS, starts `command` in a session of its own (`setsid`), with
/// no controlling terminal, where it doesn't own this process's terminal
/// ([`owns_terminal`]): it then can't type into the caller's shell
/// (`TIOCSTI` on `/dev/tty`), which Seatbelt doesn't stop. Linux's
/// bubblewrap does the same (`--new-session`); an interactive run, in
/// the terminal's foreground, keeps it. Whether it does: a session of
/// its own is a group of its own too.
pub fn own_session(command: &mut std::process::Command) -> bool {
    if !cfg!(target_os = "macos") || owns_terminal() {
        return false;
    }
    new_session(command);
    true
}

/// `command` started as the leader of a session of its own.
fn new_session(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid is async-signal-safe; its error is the start's.
    unsafe {
        command.pre_exec(|| match libc::setsid() {
            -1 => Err(std::io::Error::last_os_error()),
            _ => Ok(()),
        });
    }
}

/// `argv` as a command, its limit of open files as this process's was.
fn command(argv: &[String]) -> anyhow::Result<tokio::process::Command> {
    let (program, args) = argv.split_first().context("nothing to run")?;
    let mut command = tokio::process::Command::from(link_secretive::process::carried(program));
    // The program's limit is its own, as it was: some still use
    // `select`, which takes no descriptor past 1023.
    if let Some(&limit) = OPEN_FILES.get() {
        // SAFETY: setrlimit is async-signal-safe, and `limit` is a copy.
        unsafe {
            command.pre_exec(move || {
                libc::setrlimit(libc::RLIMIT_NOFILE, &limit);
                Ok(())
            });
        }
    }
    // The listener its proxy was handed is the proxy's alone: its
    // program never learns of it ([`crate::listen::handed`]).
    command.env_remove(link_sandbox_client::HELD_LISTENER_ENV);
    command.args(args).kill_on_drop(true);
    // A group of its own, in a session of its own where it doesn't own
    // the terminal (macOS: [`own_session`]).
    if !own_session(command.as_std_mut()) {
        command.process_group(0);
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    /// A run started in a session of its own leads it, with no
    /// controlling terminal to type into: its session's ID is its own.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_run_in_a_session_of_its_own_never_types_into_its_terminal() {
        let home = link_test_kit::TempDir::new("session");
        let mut command = link_test_kit::env::command("/bin/sh", &home);
        command.args(["-c", "exec cat /proc/self/stat"]);
        super::new_session(&mut command);
        let output = command.output().unwrap();
        assert!(output.status.success());
        let stat = String::from_utf8(output.stdout).unwrap();
        // `pid (name) state ppid pgrp session ...`, after the name.
        let pid = stat.split_whitespace().next().unwrap();
        let fields: Vec<&str> = stat[stat.rfind(')').unwrap() + 2..]
            .split_whitespace()
            .collect();
        assert_eq!(fields[3], pid, "a session of its own: {stat}");
        assert_eq!(fields[2], pid, "a group of its own: {stat}");
    }
}

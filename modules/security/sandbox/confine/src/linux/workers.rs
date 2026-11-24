//! Ordinary-box workers: a fresh host identity and idmapped state mounts.
//! The controller's inode ownership is never changed. Only the worker's
//! private mount namespace presents those inodes under its temporary identity.

mod account;
mod identities;
mod views;

use std::collections::BTreeMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use link_sandbox_contract::box_workers::{self, Controller};
use serde::{Deserialize, Serialize};

use super::session::{exited, pidfd};
use account::{Account, reap_in, records, root};
use views::{Views, directories, empty_home, mapped, mapping, source};

fn journal() -> &'static Path {
    static PATH: std::sync::LazyLock<PathBuf> =
        std::sync::LazyLock::new(|| link_files::layout::System::WorkerJournal.path());
    PATH.as_path()
}
const RUNTIME: &str = "/run/lnk-workers";

/// `workers.toml`, as read.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept {
    dropped_prefixes: Vec<String>,
}

/// `workers.toml`, read once.
fn kept() -> &'static Kept {
    static READ: std::sync::OnceLock<Kept> = std::sync::OnceLock::new();
    READ.get_or_init(|| toml::from_str(include_str!("workers.toml")).expect("workers.toml reads"))
}

/// Of `names` (this process's variables), what [`Kept::dropped_prefixes`]
/// names: what sudo told the launcher, never passed to the program it
/// starts.
fn dropped(names: impl Iterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    names
        .filter(|name| {
            let text = name.to_string_lossy();
            kept()
                .dropped_prefixes
                .iter()
                .any(|prefix| text.starts_with(prefix.as_str()))
        })
        .collect()
}

/// Provision durable identities before enabling ordinary-box work.
pub fn provision() -> anyhow::Result<()> {
    identities::provision()
}

/// Reap interrupted launchers under the registry lock.
pub fn reap() -> anyhow::Result<()> {
    account::reap()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    controller: Controller,
    roots: Vec<PathBuf>,
    empty_home: Option<PathBuf>,
    program: PathBuf,
}

/// Wrap only ordinary boxes. Packed machines and computers use their existing
/// identities. The plan is outside every worker's mounts, in Link's settings.
pub fn wrapping(
    rules: &crate::Rules,
    kept: &Path,
    argv: Vec<String>,
) -> anyhow::Result<Vec<String>> {
    let Some(controller) = box_workers::controller()? else {
        return Ok(argv);
    };
    ensure!(
        super::FILTERED,
        "ordinary-box workers require seccomp on x86_64 or aarch64"
    );
    // SAFETY: geteuid has no failure.
    ensure!(
        unsafe { libc::geteuid() } == controller.uid,
        "only the box controller launches work"
    );
    let mut roots = rules.common.write.clone();
    roots.extend(rules.common.read.iter().chain(&rules.linux.own).cloned());
    roots.extend(
        rules
            .linux
            .etc_hosts
            .iter()
            .chain(&rules.linux.resolv_conf)
            .cloned(),
    );
    if rules.common.read_home {
        roots.push(rules.common.home.clone());
    }
    let plan = kept.with_extension("worker.json");
    link_files::write_private(
        &plan,
        &serde_json::to_vec(&Plan {
            controller,
            roots,
            empty_home: (!rules.common.read_home).then(|| rules.common.home.clone()),
            program: std::env::current_exe()?.canonicalize()?,
        })?,
    )?;
    let mut outer = vec![box_workers::program().display().to_string()];
    outer.extend([
        "worker".into(),
        "--plan".into(),
        plan.display().to_string(),
        "--".into(),
    ]);
    outer.extend(argv);
    Ok(outer)
}

/// Launch a controller's prepared command. Root is used for namespace setup
/// alone; the first program inside drops every saved ID and privilege.
pub fn run(plan: &Path, parents: &[i32], argv: &[String]) -> anyhow::Result<i32> {
    ensure!(
        super::FILTERED,
        "ordinary-box workers require seccomp on x86_64 or aarch64"
    );
    let controller = box_workers::controller()?.context("this is not an ordinary box")?;
    // First invoked as the controller: acquire its actual parent's lifetime
    // before sudo. Exec of sudo clears PDEATHSIG, but cannot change a pidfd.
    if parents.is_empty() {
        ensure!(
            unsafe { libc::geteuid() } == controller.uid,
            "only the controller launches workers"
        );
        let parent_pid = unsafe { libc::getppid() } as u32;
        let parent = pidfd(parent_pid).context("pinning the controller's lifetime")?;
        let front = pidfd(std::process::id()).context("pinning the launcher's lifetime")?;
        ensure!(
            unsafe { libc::getppid() } as u32 == parent_pid && !exited(&parent, 0)?,
            "the controller ended before worker setup"
        );
        let privileged = box_workers::privileged(box_workers::program());
        let mut command = link_secretive::process::carried(privileged.get_program());
        command
            .args(privileged.get_args())
            .args(["worker", "--plan"])
            .arg(plan)
            .args([
                "--parent-fd",
                &parent.as_raw_fd().to_string(),
                "--parent-fd",
                &front.as_raw_fd().to_string(),
                "--",
            ])
            .args(argv);
        let fds = [parent.as_raw_fd(), front.as_raw_fd()];
        // SAFETY: fcntl is async-signal-safe; only this pinned lifetime is inherited.
        unsafe {
            command.pre_exec(move || {
                for fd in fds {
                    if libc::fcntl(fd, libc::F_SETFD, 0) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let mut child = command.spawn()?;
        link_secretive::process::stand_in_front(
            child.id(),
            link_secretive::process::Terminal::Reaches,
        );
        return Ok(child.wait()?.code().unwrap_or(1));
    }
    root()?;
    ensure!(
        parents.len() == 2 && parents[0] != parents[1],
        "both controller lifetime descriptors are required"
    );
    let mut lifetimes = Vec::new();
    for &parent in parents {
        ensure!(
            parent > 2
                && std::fs::read_link(format!("/proc/self/fd/{parent}"))?
                    == Path::new("anon_inode:[pidfd]"),
            "a pinned controller lifetime is required"
        );
        // SAFETY: the caller explicitly handed over this validated pidfd. Own it
        // and restore CLOEXEC before spawning any privileged or workload child.
        let parent = unsafe { OwnedFd::from_raw_fd(parent) };
        ensure!(
            unsafe { libc::fcntl(parent.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
            "closing the lifetime descriptor in children"
        );
        ensure!(!exited(&parent, 0)?, "the controller already ended");
        lifetimes.push(parent);
    }
    let trust = link_files::fs::Trust {
        owners: &[0, controller.uid],
        deny_mode: 0o022,
        ancestors: true,
        no_link: true,
    };
    let read = link_files::fs::read_trusted(plan, trust, 1024 * 1024, "the worker plan")?;
    let plan: Plan = serde_json::from_str(&read.text)?;
    ensure!(
        plan.controller == controller,
        "the controller changed; prepare this worker again"
    );
    let records = records()?;
    let mut account = {
        let _lock = records.lock(libc::LOCK_EX)?;
        reap_in(&records)?;
        Account::create(&controller)?
    };
    // SAFETY: a single-threaded launcher; mounts must never propagate outside it.
    unsafe {
        ensure!(
            libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) == 0,
            "adopting worker descendants"
        );
        ensure!(
            libc::unshare(libc::CLONE_NEWNS) == 0,
            "the worker needs a private mount namespace"
        );
        ensure!(
            libc::mount(
                std::ptr::null(),
                c"/".as_ptr(),
                std::ptr::null(),
                libc::MS_REC | libc::MS_PRIVATE,
                std::ptr::null()
            ) == 0,
            "making the worker's mounts private"
        );
    }
    let runtime = Path::new(RUNTIME).join(&account.name);
    link_files::create_private_dir(&runtime)?;
    let mut views = Views {
        dir: runtime.clone(),
        paths: Vec::new(),
    };
    let mapping = mapping(&controller, &account)?;
    let home = runtime.join("home");
    if plan.empty_home.is_some() {
        views.paths.push(home.clone());
        empty_home(&home, &account)?;
    }
    let mut argv = argv.to_vec();
    let bwrap = super::mounts::command_at(&argv);
    ensure!(
        argv.get(bwrap)
            .is_some_and(|x| x == "/usr/bin/bwrap" || x == "/bin/bwrap"),
        "a worker runs bubblewrap"
    );
    let mut end = (bwrap + 1..argv.len())
        .find(|&i| argv[i] == "--")
        .context("bubblewrap has no command")?;
    let mut mapped_paths = BTreeMap::<(PathBuf, bool), PathBuf>::new();
    let mut i = bwrap + 1;
    while i < end {
        if matches!(
            argv[i].as_str(),
            "--bind" | "--bind-try" | "--ro-bind" | "--ro-bind-try"
        ) {
            let path = PathBuf::from(&argv[i + 1]);
            if plan.roots.iter().any(|root| path.starts_with(root)) {
                let program = path == plan.program && argv[i].starts_with("--ro-bind");
                let cache = (path.clone(), program);
                let next = runtime.join(format!("view-{}", mapped_paths.len()));
                let target = if let Some(target) = mapped_paths.get(&cache) {
                    target.clone()
                } else {
                    // Helpers inside run the installed root-owned binary,
                    // even when the controller's original executable is private
                    // or lives on a filesystem without idmapped mounts.
                    let source = match source(if program {
                        box_workers::program()
                    } else {
                        &path
                    }) {
                        Ok(source) => source,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::NotFound
                                && argv[i].ends_with("-try") =>
                        {
                            // Freeze absence as well as presence. A sibling
                            // worker cannot make bwrap discover a raw bind later.
                            argv.drain(i..i + 3);
                            end -= 3;
                            continue;
                        }
                        Err(e) => {
                            return Err(e).with_context(|| {
                                format!("opening worker state {} without links", path.display())
                            });
                        }
                    };
                    views.paths.push(next.clone());
                    mapped(source, &next, if program { None } else { Some(&mapping) })
                        .with_context(|| format!("mapping worker state {}", path.display()))?;
                    mapped_paths.insert(cache, next.clone());
                    next
                };
                argv[i + 1] = target.display().to_string();
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    // Existing rootless bubblewrap options are replaced only before its command.
    let normalized = directories(
        &argv[..end],
        plan.empty_home
            .as_deref()
            .map(|original| (original, home.as_path())),
    );
    let mut head: Vec<_> = normalized
        .iter()
        .filter(|a| !matches!(a.as_str(), "--unshare-user" | "--disable-userns"))
        .cloned()
        .collect();
    let program = box_workers::program()
        .to_str()
        .expect("the installed worker path is text");
    head.extend(
        [
            "--cap-drop",
            "ALL",
            "--cap-add",
            "CAP_SETUID",
            "--cap-add",
            "CAP_SETGID",
            "--ro-bind",
            program,
            program,
            "--chdir",
            "/",
            "--",
        ]
        .map(str::to_string),
    );
    let [_, verb, rest @ ..] = &argv[end + 1..] else {
        bail!("no worker entry point")
    };
    ensure!(
        verb == "confine",
        "the worker starts at its privilege boundary"
    );
    head.extend([
        program.to_string(),
        "confine".into(),
        "--worker-user".into(),
        account.uid.to_string(),
    ]);
    for (_, gid) in &account.groups {
        head.extend(["--worker-group".into(), gid.to_string()]);
    }
    let cwd = std::env::current_dir()?;
    let cwd = if plan.roots.iter().any(|root| cwd.starts_with(root)) {
        cwd
    } else {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("the worker's home")?
    };
    head.extend(["--worker-dir".into(), cwd.display().to_string()]);
    head.extend(rest.iter().cloned());
    ensure!(
        !lifetimes
            .iter()
            .map(|fd| exited(fd, 0))
            .collect::<Result<Vec<_>, _>>()?
            .contains(&true),
        "the controller ended during worker setup"
    );
    let asked = super::session::ask(&mut head)?;
    let pins = super::pins::Pins::of(&mut head)?;
    let mut command = link_secretive::process::carried(&head[0]);
    command.args(&head[1..]);
    for name in dropped(std::env::vars_os().map(|(name, _)| name)) {
        command.env_remove(name);
    }
    pins.pass_to(&mut command);
    asked.pass_to(&mut command);
    // Cover the interval before bwrap installs its own parent-death guard.
    link_secretive::process::ends_with_parent(&mut command, libc::SIGKILL);
    let mut child = command.spawn()?;
    let child_lifetime = pidfd(child.id())?;
    link_secretive::process::stand_in_front(child.id(), link_secretive::process::Terminal::Reaches);
    asked.started(child.id());
    loop {
        if exited(&child_lifetime, 0)? {
            break;
        }
        if exited(&lifetimes[0], 50)? || exited(&lifetimes[1], 50)? {
            // Our child cannot be reused before wait. Its namespace init
            // dies too; cleanup below also checks every worker UID field.
            let _ = child.kill();
            break;
        }
    }
    // Stop numeric signal forwarding before reaping the child, while its
    // PID is still reserved. Cleanup cannot signal an unrelated reused PID.
    link_secretive::process::stand_aside();
    let status = child.wait()?;
    views.clean()?;
    account.clean()?;
    account.cleanup = false;
    Ok(status.code().unwrap_or(1))
}

/// First inside, before the seccomp watcher or any worker-supplied program.
pub fn enter(uid: u32, groups: &[u32]) -> anyhow::Result<()> {
    root()?;
    ensure!(
        uid != 0 && !groups.is_empty() && !groups.contains(&0),
        "a worker has no root identity"
    );
    // SAFETY: drop supplementary, real, effective and saved IDs, then forbid
    // exec from gaining privileges. No worker code has run before this point.
    unsafe {
        ensure!(
            libc::setgroups(groups.len(), groups.as_ptr()) == 0,
            "setting worker groups"
        );
        ensure!(
            libc::setresgid(groups[0], groups[0], groups[0]) == 0,
            "dropping the worker's group privilege"
        );
        ensure!(
            libc::setresuid(uid, uid, uid) == 0,
            "dropping the worker's user privilege"
        );
        ensure!(
            libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0,
            "refusing worker privilege changes"
        );
        #[repr(C)]
        struct Header {
            version: u32,
            pid: i32,
        }
        #[repr(C)]
        struct Caps {
            effective: u32,
            permitted: u32,
            inheritable: u32,
        }
        let header = Header {
            version: 0x2008_0522,
            pid: 0,
        };
        let caps = [
            Caps {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            },
            Caps {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            },
        ];
        ensure!(
            libc::syscall(libc::SYS_capset, &header, caps.as_ptr()) == 0,
            "clearing every worker capability"
        );
    }
    link_secretive::process::guard();
    Ok(())
}

#[cfg(test)]
mod tests {
    /// What sudo adds is taken out of the program's environment, and
    /// nothing else is.
    #[test]
    fn sudos_variables_never_reach_the_program() {
        let names = [
            "SUDO_USER",
            "SUDO_UID",
            "SUDO_GID",
            "SUDO_COMMAND",
            "PATH",
            "XSUDO_X",
        ];
        let dropped = super::dropped(names.iter().map(std::ffi::OsString::from));
        assert_eq!(
            dropped,
            ["SUDO_USER", "SUDO_UID", "SUDO_GID", "SUDO_COMMAND"]
        );
    }

    /// A worker's first program, run as root, ends as the worker's
    /// account before anything of the worker's runs: every user and group
    /// ID the worker's, no capability, and no way back.
    #[test]
    fn a_worker_ends_as_its_own_account_never_root() {
        if !link_test_kit::gate::root() {
            return;
        }
        const WORKER: u32 = 65534;
        // In a child, so this test's own process keeps root.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            let code = match super::enter(WORKER, &[WORKER]) {
                Err(_) => 10,
                Ok(()) => {
                    let (mut r, mut e, mut s) = (0, 0, 0);
                    let (mut gr, mut ge, mut gs) = (0, 0, 0);
                    unsafe {
                        libc::getresuid(&mut r, &mut e, &mut s);
                        libc::getresgid(&mut gr, &mut ge, &mut gs);
                    }
                    // Read without allocating: a forked child of a
                    // threaded process may find the allocator's lock held
                    // by a thread that no longer exists in it.
                    let mut buffer = [0u8; 8192];
                    let read = unsafe {
                        let fd = libc::open(c"/proc/self/status".as_ptr(), libc::O_RDONLY);
                        let n = libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len());
                        libc::close(fd);
                        n.max(0) as usize
                    };
                    let status = std::str::from_utf8(&buffer[..read]).unwrap_or("");
                    let caps_none = ["CapEff:", "CapPrm:"].iter().all(|field| {
                        status
                            .lines()
                            .find(|l| l.starts_with(field))
                            .is_some_and(|l| {
                                l.split_whitespace().nth(1) == Some("0000000000000000")
                            })
                    });
                    let back = unsafe { libc::setuid(0) } == 0;
                    match (
                        [r, e, s, gr, ge, gs].iter().all(|id| *id == WORKER),
                        caps_none,
                        back,
                    ) {
                        (true, true, false) => 0,
                        (false, _, _) => 1,
                        (_, false, _) => 2,
                        (_, _, true) => 3,
                    }
                }
            };
            unsafe { libc::_exit(code) };
        }
        let mut status = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        assert!(libc::WIFEXITED(status));
        assert_eq!(
            libc::WEXITSTATUS(status),
            0,
            "10: enter failed; 1: an ID stayed root's; 2: a capability stayed; 3: root came back"
        );
    }
}

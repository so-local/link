//! `lnk sandbox check`: the sandbox tried from inside.

use std::path::{Path, PathBuf};

use anyhow::bail;
use link_format::{
    Address, Count, Done, Fields, Foreign, Out, Report, Table, Value, WithinExt, cmd, prose,
    sentence, states,
};
use link_sandbox::check::{self, Verdict};
use link_sandbox::cli::{self};
use link_sandbox_client::Net;
use link_sandbox_client::Wrap;

use super::*;

pub(crate) fn check_command(
    out: &mut Out,
    cli::SandboxCheckArgs {
        spec,
        upstream,
        ready: only_ready,
        yes,
        policy,
        reaches,
        refuses,
        runs,
        refuses_program,
        dot_files_open,
        dot_files_closed,
        reads,
        refuses_read,
    }: cli::SandboxCheckArgs,
    fields: link_spec::FieldFlags,
) -> Result<Done, Report> {
    // The confinement to check comes from its caller or its spec, never both.
    if policy && spec.is_some() {
        return Err(anyhow::anyhow!("--policy can't be given with --spec").into());
    }
    if only_ready {
        if policy {
            return Err(anyhow::anyhow!("--ready can't be given with --policy").into());
        }
        // Asked afresh: what was kept of this machine is proved again.
        ready_again(out, !yes)?;
        return Ok(Done::success(sentence!("The sandbox can run here."))
            .next(cmd!("lnk sandbox check"), "try it from inside"));
    }
    let opening = link_sandbox::opening::Opening::of(spec, fields, upstream, yes);
    let (policy, own) = match policy {
        true => {
            let sandbox: Wrap = serde_json::from_reader(std::io::stdin().lock())
                .within("reading the confinement on stdin")?;
            (link_sandbox::spec::Resolved::new(door(sandbox)?)?, false)
        }
        false => (opening.opened("check")?.sandbox, true),
    };
    let target = |t: &String| check::target(t);
    let path = link_files::absolute_path(&std::env::var_os("PATH").unwrap_or_default());
    let program = |p: &String| -> anyhow::Result<PathBuf> {
        let found = match p.contains('/') {
            true => Some(PathBuf::from(p)),
            false => link_files::find_program(p, &path),
        };
        found
            .map(|f| link_sandbox_confine::resolve(&f))
            .within(format!("{p}: not found"))
    };
    let expected = check::Expected {
        reaches: reaches.iter().map(target).collect::<anyhow::Result<_>>()?,
        refuses: refuses.iter().map(target).collect::<anyhow::Result<_>>()?,
        runs: runs.iter().map(program).collect::<anyhow::Result<_>>()?,
        refused_programs: refuses_program
            .iter()
            .map(program)
            .collect::<anyhow::Result<_>>()?,
        dot_files_open: dot_files_open
            .iter()
            .map(|d| Ok(link_sandbox_confine::resolve(&std::path::absolute(d)?)))
            .collect::<anyhow::Result<_>>()?,
        dot_files_closed: dot_files_closed
            .iter()
            .map(|d| Ok(link_sandbox_confine::resolve(&std::path::absolute(d)?)))
            .collect::<anyhow::Result<_>>()?,
        reads: reads
            .iter()
            .map(std::path::absolute)
            .collect::<std::io::Result<_>>()
            .within("finding the folders to read")?,
        unread: refuses_read
            .iter()
            .map(std::path::absolute)
            .collect::<std::io::Result<_>>()
            .within("finding the folders not to read")?,
    };
    // Another command asking has made sure already, in its own words.
    ready(out, own)?;
    let leaks = check(out, &policy, own, &expected);
    if own {
        clean_up(&policy);
    }
    match leaks? {
        0 => Ok(
            Done::success(sentence!("Nothing got through the sandbox.")).next(
                cmd!(
                    "lnk sandbox run --commands.toolchains {} --files.write . -- bash",
                    "shell"
                ),
                "run a program in one",
            ),
        ),
        leaks => Err(Report::error(sentence!(
            "{} got through the sandbox that shouldn't have.",
            Count::new(leaks as u64, "thing", "things")
        ))
        .next(
            cmd!("lnk sandbox permission list"),
            "see what a spec can open",
        )),
    }
}

pub(crate) fn try_exec_command(
    cli::SandboxTryExecArgs {
        how,
        payload,
        write,
        loader,
        lib,
    }: cli::SandboxTryExecArgs,
) -> Result<Done, Report> {
    Ok(Done::passed(try_exec(
        &how,
        &payload,
        write.as_deref(),
        loader.as_deref(),
        lib.as_deref(),
    )?))
}

/// `sandbox try-exec`: tries one way around `[commands]` to run `payload`
/// (a program that exits 0) from inside the sandbox, for `check`. Returns
/// the exit code to use: 0 if the program ran (the way got through), 1 if
/// the sandbox stopped it. Each runs in a child, so a blocked `exec`
/// leaves this process to report it.
#[cfg(target_os = "linux")]
pub(crate) fn try_exec(
    how: &str,
    payload: &Path,
    write: Option<&Path>,
    loader: Option<&Path>,
    lib: Option<&Path>,
) -> anyhow::Result<i32> {
    // A copy in a folder the sandbox may write, removed once tried, so
    // `check` leaves nothing in your folders.
    struct Copy(PathBuf);
    impl Drop for Copy {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    // A copy of `payload` there, for the ways that run a file
    // (`/proc/self/fd` and the loader).
    let written = |ext: &str| -> anyhow::Result<Copy> {
        let dir = write.within("this way needs a folder the sandbox may write")?;
        let at = Copy(dir.join(format!("lnk-try-{}{ext}", std::process::id())));
        std::fs::copy(payload, &at.0)?;
        let mut perms = std::fs::metadata(&at.0)?.permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&at.0, perms)?;
        Ok(at)
    };
    // Runs `child` in a fork; this process's exit code is 0 when the child
    // ran `payload` (which exits 0), 1 otherwise.
    let forked = |child: &dyn Fn()| -> anyhow::Result<i32> {
        // SAFETY: the child only execs or _exits; no allocation after fork.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            child();
            unsafe { libc::_exit(1) };
        }
        let mut status = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        let ran = libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0;
        Ok(if ran { 0 } else { 1 })
    };
    match how {
        // The loader runs any file it can read; it must stay runnable for
        // every dynamically linked program, so this can't be closed.
        "ldso" => {
            let loader = loader.within("the ldso way needs --loader")?;
            let file = written("")?;
            // Reached if the loader ran it.
            Ok(
                match link_secretive::process::command(loader)
                    .arg(&file.0)
                    .status()
                {
                    Ok(s) if s.success() => 0,
                    _ => 1,
                },
            )
        }
        // A library the sandbox wrote, loaded into a listed program. The
        // loader's injection variables are cleared at the sandbox's start,
        // but a program can set one for a child, as here.
        "preload" => {
            let lib = lib.within("the preload way needs --lib")?;
            let dir = write.within("the preload way needs --files.write")?;
            let copy = Copy(dir.join(format!("lnk-try-{}.so", std::process::id())));
            std::fs::copy(lib, &copy.0)?;
            let out = link_secretive::process::command(payload)
                .env("LD_PRELOAD", &copy.0)
                .output()?;
            let err = String::from_utf8_lossy(&out.stderr);
            // Reached if the program ran and the loader didn't reject the
            // writable library.
            let rejected = err.contains("cannot be preloaded") || err.contains("cannot open");
            Ok(if out.status.success() && !rejected {
                0
            } else {
                1
            })
        }
        // A program copied into a memory file (`memfd_create`), run with no
        // path: by `execveat` (closed by the seccomp filter) or through
        // `/proc/self/fd` (closed by non-executable memory).
        "memfd" | "memfd-proc" => {
            let bytes = std::fs::read(payload)?;
            let proc = how == "memfd-proc";
            forked(&|| unsafe {
                let fd = libc::syscall(libc::SYS_memfd_create, c"lnk".as_ptr(), 0u32) as i32;
                if fd < 0 || libc::write(fd, bytes.as_ptr().cast(), bytes.len()) < 0 {
                    return;
                }
                let argv = [c"x".as_ptr(), std::ptr::null()];
                let envp = [std::ptr::null()];
                if proc {
                    let path = std::ffi::CString::new(format!("/proc/self/fd/{fd}")).unwrap();
                    libc::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr());
                } else {
                    libc::syscall(
                        libc::SYS_execveat,
                        fd,
                        c"".as_ptr(),
                        argv.as_ptr(),
                        envp.as_ptr(),
                        0x1000, // AT_EMPTY_PATH
                    );
                }
            })
        }
        // A program copied into a memory file of huge pages, whose seal
        // hugetlbfs doesn't keep: made executable again and run through
        // `/proc/self/fd` (closed by refusing such memory files). The
        // loader may fail to map it once exec let it in, so it got
        // through unless exec itself refused it.
        "memfd-huge" => {
            let bytes = std::fs::read(payload)?;
            const MFD_HUGETLB: u32 = 0x4;
            const SIZE: usize = 2 << 20;
            if bytes.len() > SIZE {
                bail!("{} is larger than a huge page", payload.display());
            }
            // SAFETY: the child only makes syscalls and _exits.
            let pid = unsafe { libc::fork() };
            if pid == 0 {
                unsafe {
                    let fd =
                        libc::syscall(libc::SYS_memfd_create, c"lnk".as_ptr(), MFD_HUGETLB) as i32;
                    if fd >= 0 && libc::ftruncate(fd, SIZE as libc::off_t) == 0 {
                        let at = libc::mmap(
                            std::ptr::null_mut(),
                            SIZE,
                            libc::PROT_READ | libc::PROT_WRITE,
                            libc::MAP_SHARED,
                            fd,
                            0,
                        );
                        if at != libc::MAP_FAILED {
                            std::ptr::copy_nonoverlapping(bytes.as_ptr(), at.cast(), bytes.len());
                            libc::munmap(at, SIZE);
                            libc::fchmod(fd, 0o755);
                            let path =
                                std::ffi::CString::new(format!("/proc/self/fd/{fd}")).unwrap();
                            let argv = [c"x".as_ptr(), std::ptr::null()];
                            let envp = [std::ptr::null()];
                            libc::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr());
                        }
                    }
                    libc::_exit(1);
                }
            }
            let mut status = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };
            let refused = libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 1;
            Ok(if refused { 1 } else { 0 })
        }
        // A file the sandbox wrote, run through its `/proc/self/fd` entry
        // (Landlock judges the real file: closed).
        "written-proc" => {
            let file = written("")?;
            forked(&|| unsafe {
                let path = std::ffi::CString::new(file.0.display().to_string()).unwrap();
                let fd = libc::open(path.as_ptr(), libc::O_RDONLY);
                if fd < 0 {
                    return;
                }
                let proc = std::ffi::CString::new(format!("/proc/self/fd/{fd}")).unwrap();
                let argv = [c"x".as_ptr(), std::ptr::null()];
                let envp = [std::ptr::null()];
                libc::execve(proc.as_ptr(), argv.as_ptr(), envp.as_ptr());
            })
        }
        other => bail!("unknown way around [commands]: {other}"),
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn try_exec(
    _how: &str,
    _payload: &Path,
    _write: Option<&Path>,
    _loader: Option<&Path>,
    _lib: Option<&Path>,
) -> anyhow::Result<i32> {
    bail!("the [commands] probes run on Linux only")
}

states! {
    /// How one probe came out.
    enum Probed {
        Blocked("blocked") => Good,
        Allowed("allowed") => Good,
        Failed("failed") => Warn,
        Missed("missed") => Bad,
        Open("open") => Warn,
        Leaked("leaked") => Bad,
    }
}

/// `lnk sandbox check`: the probes' verdicts, and whether nothing leaked.
/// `own`: checked from the flags (not for another command), so it says
/// what it tried and what was allowed. How many got through.
pub(crate) fn check(
    out: &mut Out,
    policy: &link_sandbox::spec::Resolved,
    own: bool,
    expected: &check::Expected,
) -> Result<usize, Report> {
    if own {
        out.block(link_format::Note(sentence!(
            "Trying, from inside the sandbox, what it shouldn't allow."
        )));
    }
    let mut leaks = 0;
    let mut table = Table::new(["RESULT", "WHAT", "NOTE"], sentence!("Nothing was tried."));
    for (probe, verdict) in check::run(policy, expected)? {
        let (state, note) = match verdict {
            Verdict::Blocked => (Probed::Blocked, String::new()),
            Verdict::Allowed => (Probed::Allowed, String::new()),
            Verdict::AllowedFailed => (Probed::Failed, "allowed, but it didn't work here".into()),
            Verdict::Missed => {
                leaks += 1;
                (Probed::Missed, "it should have been allowed".into())
            }
            Verdict::Gap(why) => (Probed::Open, format!("known: {why}")),
            Verdict::Leak => {
                leaks += 1;
                (
                    Probed::Leaked,
                    "the sandbox should have blocked this".into(),
                )
            }
        };
        table.row([
            Value::from(state),
            Value::from(probe.what.to_string()),
            Value::from(note),
        ]);
    }
    out.block(table);
    if own {
        // In a spec's names.
        let granted: Vec<Value> = [
            (
                policy.read_home,
                "your whole home, to read (--files.read ~)",
            ),
            (
                link_sandbox::proxy_of(policy).is_some_and(|p| p.pages().is_some()),
                "pages in your browser, asking you (--browser.hosts)",
            ),
            (
                matches!(policy.net, Net::Host { .. }),
                "this machine's network as it is, with no proxy (--network.mode host)",
            ),
            (
                policy.developer_tools,
                "Xcode's tools, to run (--commands.developer_tools)",
            ),
        ]
        .into_iter()
        .filter(|(granted, _)| *granted)
        .map(|(_, what)| Value::from(what))
        .collect();
        let mut fields = Fields::new().row(
            "Allowed",
            match granted.is_empty() {
                true => Value::from("nothing beyond its folders"),
                false => Value::List(granted),
            },
        );
        if let Some(proxy) = link_sandbox::proxy_of(policy) {
            fields = fields.row(
                "Network",
                format!(
                    "only through Link's proxy ({}), never this machine or its networks",
                    proxy.upstream
                ),
            );
            if !proxy.hosts.is_empty() {
                fields = fields.row(
                    "Hosts",
                    Value::List(proxy.hosts.iter().map(Value::from).collect()),
                );
            }
            out.block(fields);
            for (endpoint, domain) in check::shared_wildcards(&proxy.tcp) {
                out.block(
                    Report::warning(sentence!(
                        "{} reaches every customer's servers on {}, not only yours.",
                        Address::new(endpoint),
                        Foreign::new(&domain)
                    ))
                    .why(prose!(
                        "Name your own under it: *.<yours>.{}.",
                        Foreign::new(&domain)
                    )),
                );
            }
        } else {
            out.block(fields);
        }
    }
    Ok(leaks)
}

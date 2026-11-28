//! Inside the sandbox, as its first program (`sandbox confine`): what
//! it was handed closed, the seccomp filter and the exec watch on this
//! process where they're built ([`super::FILTERED`]), then the program
//! in its place.

use std::convert::Infallible;
use std::path::PathBuf;

use anyhow::{Context, ensure};

/// Every descriptor it was handed past stderr closed, the seccomp filter
/// on this process where it's built, the file grants held again by
/// Landlock where the kernel has it (`wall`), then `argv` in its place: it
/// returns only what stopped it.
pub fn enter(
    wall: Option<&crate::Wall>,
    only: Option<&[PathBuf]>,
    argv: &[String],
) -> anyhow::Result<Infallible> {
    use std::os::unix::process::CommandExt;
    close_inherited()?;
    // `wrap` refuses a `[commands]` list here: no filter to hold it.
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    ensure!(
        only.is_none(),
        "no seccomp filter here for [commands]: {argv:?} not run"
    );
    // The seccomp filter and exec watch install before the second file
    // wall so, where the watch forks, only the program it runs carries the
    // wall, not the watcher that supervises it.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    filter(only, argv)?;
    file_wall(wall)?;
    let err = link_secretive::process::carried(&argv[0])
        .args(&argv[1..])
        .exec();
    Err(err).with_context(|| format!("running {}", argv[0]))
}

/// The system baseline Landlock's file wall allows before any per-policy
/// grant: the read-only system folders ([`crate::system::SYSTEM`]) and the
/// machine's `/etc` ([`crate::system::LINUX_ETC`]) it may read, and the
/// fresh writable folders ([`crate::system::LINUX_EPHEMERAL`]) it may read
/// and write. bubblewrap mounts these very paths
/// ([`crate::linux::bwrap::args`]), from the one declaration both render
/// from, so the second wall can allow nothing the mounts do not expose. A
/// folder not there allows nothing
/// ([`crate::linux::landlock::restrict_files`]).
pub(crate) fn baseline() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let read = crate::system::SYSTEM
        .iter()
        .copied()
        .chain(std::iter::once(crate::system::LINUX_ETC))
        .map(PathBuf::from)
        .collect();
    let write = crate::system::LINUX_EPHEMERAL
        .iter()
        .map(|dir| PathBuf::from(dir.at))
        .collect();
    (read, write)
}

/// Install the second file wall for `wall`, where the kernel has Landlock:
/// read only the system [`baseline`] and its read grants, read and write
/// only the ephemeral baseline and its write grants. A no-op without
/// Landlock, so the mounts stand alone as they always could.
fn file_wall(wall: Option<&crate::Wall>) -> anyhow::Result<()> {
    let Some(wall) = wall else { return Ok(()) };
    let (mut read, mut write) = baseline();
    read.extend(wall.read.iter().cloned());
    write.extend(wall.write.iter().cloned());
    crate::linux::landlock::require_files(&read, &write)
        .context("installing the Landlock file wall; target not started")
}

/// [`enter`]'s seccomp filter and exec watch, on this process, which
/// then becomes the program (or, watching it, its parent).
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn filter(only: Option<&[PathBuf]>, argv: &[String]) -> anyhow::Result<()> {
    {
        if let Some(programs) = only {
            // Programs are listed: close the ways around the list a program
            // writes its own code and runs it. The loader's injection
            // variables, so a sandbox started with one set doesn't carry it
            // in (a program can still set it for a child, a stated gap).
            for var in ["LD_PRELOAD", "LD_LIBRARY_PATH", "LD_AUDIT"] {
                unsafe { std::env::remove_var(var) };
            }
            crate::linux::landlock::restrict_exec(programs).with_context(|| {
                let policy = if programs.is_empty() {
                    "command policy grants no programs (deny by default)"
                } else {
                    "command policy permits only listed programs"
                };
                format!("{policy}; executable restrictions were not installed; target not started")
            })?;
        }
        use crate::linux::execs::Watch;
        // `lnk sandbox learn`'s: every program started, to its descriptor.
        let learn = std::env::var(crate::linux::execs::LEARN_FD).ok();
        unsafe { std::env::remove_var(crate::linux::execs::LEARN_FD) };
        let watch = match (learn, only) {
            (Some(fd), _) => {
                let fd: i32 = fd.parse().context("learning's descriptor")?;
                ensure!(fd > 2, "learning's descriptor is {fd}");
                // SAFETY: `lnk sandbox learn` left this descriptor open for us;
                // closed on exec, so the program never holds it.
                let file = unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(fd) };
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
                Some(Watch::Learn(file))
            }
            (None, Some(programs)) => Some(Watch::Refusals(programs.to_vec())),
            (None, None) => None,
        };
        // Programs listed, on a kernel that can (Linux 6.3 and newer):
        // every memory file made non-executable by the watch, so a program
        // copied into one (`memfd_create`) can't be run from it
        // (`/proc/self/fd`), while `memfd_create` and mapping it executable
        // (a JIT) still work. The seccomp filter closes `execveat` anyway.
        let seal = only.is_some() && crate::linux::execs::sealable();
        // Refusal reporting is best effort if a listener is unavailable
        // (an older kernel, or another runtime's listener). Executable
        // enforcement is not: the base filter then denies memory files.
        let listener = match &watch {
            Some(Watch::Learn(_)) => {
                Some(crate::linux::seccomp::listen(seal).context("watching what it runs")?)
            }
            Some(Watch::Refusals(_)) => match crate::linux::seccomp::listen(seal) {
                Ok(listener) => Some(listener),
                Err(err) => {
                    if seal {
                        link_format::aside(
                            link_format::Report::warning(link_format::sentence!(
                                "The executable watch is unavailable, as {} is disabled for restricted programs.",
                                link_format::Foreign::new("memfd_create")
                            ))
                            .why(link_format::Prose::from(
                                link_format::Sentence::adopt(&format!("{err:#}")),
                            )),
                        );
                    }
                    None
                }
            },
            None => None,
        };
        use crate::linux::seccomp::{Exec, MemoryFiles};
        let exec = if only.is_some() {
            if !seal {
                link_format::aside(
                    link_format::Report::warning(link_format::sentence!(
                        "{} is disabled for restricted programs.",
                        link_format::Foreign::new("memfd_create")
                    ))
                    .why(link_format::prose!(
                        "This kernel cannot seal memory files non-executable."
                    )),
                );
            }
            Exec::Only {
                memory: MemoryFiles::for_watch(seal, listener.is_some()),
            }
        } else {
            Exec::Any
        };
        crate::linux::seccomp::install(exec).context("installing the seccomp filter")?;
        if let (Some(listener), Some(watch)) = (listener, watch) {
            // SAFETY: one thread here; the child only execs or exits.
            let child = unsafe { libc::fork() };
            ensure!(
                child >= 0,
                "starting {}: {}",
                argv[0],
                std::io::Error::last_os_error()
            );
            if child > 0 {
                let status = crate::linux::execs::supervise(listener, child, watch);
                if libc::WIFSIGNALED(status) {
                    // Ended as it was: by its signal.
                    let signal = libc::WTERMSIG(status);
                    unsafe {
                        libc::signal(signal, libc::SIG_DFL);
                        libc::kill(libc::getpid(), signal);
                    }
                    std::process::exit(128 + signal);
                }
                std::process::exit(libc::WEXITSTATUS(status));
            }
        }
        Ok(())
    }
}

/// Every descriptor past stderr closed when [`enter`] becomes the
/// program: bubblewrap passes on whatever its caller left open (a CI
/// runner's pipe to its worker, a terminal, an agent's socket), which the
/// program could otherwise write or read. Its own, the exec watch's
/// listener and `learn`'s pipe, stay with the watch, which never execs.
#[cfg(target_os = "linux")]
fn close_inherited() -> anyhow::Result<()> {
    // SAFETY: marks descriptors close-on-exec; closes none now.
    let marked = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3 as libc::c_uint,
            libc::c_uint::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    };
    if marked == 0 {
        return Ok(());
    }
    // Before Linux 5.11: each one open, as /proc lists it.
    let open: Vec<i32> = std::fs::read_dir("/proc/self/fd")
        .context("listing its descriptors")?
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .filter(|&fd| fd > 2)
        .collect();
    for fd in open {
        // SAFETY: a flag on a descriptor; one already closed (the
        // listing's own) just fails.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    Ok(())
}

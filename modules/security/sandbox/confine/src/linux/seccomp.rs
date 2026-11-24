//! The kernel wall (Linux): a seccomp filter every program in the sandbox
//! runs under.
//!
//! bubblewrap's namespaces and mounts keep a program to its folders; the
//! kernel's system calls are what's left to attack. This filter refuses
//! the ones a sandboxed program doesn't need and a kernel bug is most
//! often found in, as container runtimes do: io_uring, BPF, userfaultfd,
//! perf events, the key store, ptrace and reading other processes'
//! memory, mounting, new namespaces, loading kernel code, and typing into
//! a terminal. They fail with an error (`ENOSYS` where a program falls
//! back, such as libuv without io_uring; `EPERM` otherwise), never by
//! killing the program.
//!
//! `lnk-sandbox confine -- <argv>`, first inside the sandbox,
//! installs it and becomes `argv`: the wrapped command stays a plain
//! argv, which a background service can run later.
//!
//! With `[commands]` listing programs (Landlock limiting which files run),
//! it refuses one more way around that list: running a file descriptor,
//! `execveat(fd, "", ..., AT_EMPTY_PATH)` (and `fexecve`), which runs a
//! program copied into memory (`memfd_create`) that no folder holds.
//! Landlock already refuses `execveat` and `/proc/self/fd` of a file the
//! sandbox wrote; this closes the memory file. With a working sealing
//! listener, `memfd_create` stays open for Chromium and JITs while the
//! watch makes each memory file non-executable ([`exec_notify`],
//! `crate::linux::execs`). Otherwise restricted execution refuses
//! `memfd_create` outright, so unavailable sealing cannot widen it.

use crate::constants::SYS_FCHMODAT2;
use std::io;

/// The executable restriction enforced by the base filter. Restricted
/// execution always closes descriptor exec and either seals memory files
/// through the listener or refuses their creation altogether.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exec {
    Any,
    Only { memory: MemoryFiles },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryFiles {
    Sealed,
    Denied,
}

impl MemoryFiles {
    /// Both capabilities are needed to preserve `memfd_create` safely.
    /// In particular an outer listener is not evidence of sealing.
    pub fn for_watch(sealing: bool, listening: bool) -> Self {
        if sealing && listening {
            Self::Sealed
        } else {
            Self::Denied
        }
    }
}

/// A refused system call, and the error it gets.
struct Refused {
    nr: libc::c_long,
    errno: i32,
}

const fn eperm(nr: libc::c_long) -> Refused {
    Refused {
        nr,
        errno: libc::EPERM,
    }
}

const fn enosys(nr: libc::c_long) -> Refused {
    Refused {
        nr,
        errno: libc::ENOSYS,
    }
}

/// The system calls refused outright (`clone` and `ioctl` are refused by
/// their arguments, below).
const REFUSED: &[Refused] = &[
    // io_uring: much of the kernel reached through one ring; libuv falls
    // back to threads.
    enosys(libc::SYS_io_uring_setup),
    enosys(libc::SYS_io_uring_enter),
    enosys(libc::SYS_io_uring_register),
    eperm(libc::SYS_bpf),
    eperm(libc::SYS_userfaultfd),
    eperm(libc::SYS_perf_event_open),
    // The kernel's key store.
    eperm(libc::SYS_keyctl),
    eperm(libc::SYS_add_key),
    eperm(libc::SYS_request_key),
    // Other processes' memory, and tracing.
    eperm(libc::SYS_ptrace),
    eperm(libc::SYS_process_vm_readv),
    eperm(libc::SYS_process_vm_writev),
    // Mounting, and the new mount API.
    eperm(libc::SYS_mount),
    eperm(libc::SYS_umount2),
    eperm(libc::SYS_pivot_root),
    eperm(libc::SYS_move_mount),
    eperm(libc::SYS_open_tree),
    eperm(libc::SYS_fsopen),
    eperm(libc::SYS_fsconfig),
    eperm(libc::SYS_fsmount),
    eperm(libc::SYS_fspick),
    eperm(libc::SYS_mount_setattr),
    // New namespaces (user namespaces open much of the kernel to an
    // unprivileged program); clone3 can't be read by a filter, so it gets
    // ENOSYS and libc falls back to clone, whose flags are checked.
    eperm(libc::SYS_unshare),
    eperm(libc::SYS_setns),
    enosys(libc::SYS_clone3),
    // openat2's mode is behind a pointer, so classic BPF cannot inspect
    // it. libc and callers can use openat, whose mode is checked below.
    enosys(libc::SYS_openat2),
    // Files by handle, around the mount namespace.
    eperm(libc::SYS_open_by_handle_at),
    eperm(libc::SYS_name_to_handle_at),
    // Kernel code, the machine, its clock and its accounting.
    eperm(libc::SYS_kexec_load),
    eperm(SYS_KEXEC_FILE_LOAD),
    eperm(libc::SYS_init_module),
    eperm(libc::SYS_finit_module),
    eperm(libc::SYS_delete_module),
    eperm(libc::SYS_reboot),
    eperm(libc::SYS_swapon),
    eperm(libc::SYS_swapoff),
    eperm(libc::SYS_acct),
    eperm(libc::SYS_quotactl),
    eperm(libc::SYS_syslog),
    eperm(libc::SYS_settimeofday),
    eperm(libc::SYS_clock_settime),
    eperm(libc::SYS_clock_adjtime),
    eperm(libc::SYS_adjtimex),
    eperm(libc::SYS_vhangup),
    #[cfg(target_arch = "x86_64")]
    eperm(libc::SYS_iopl),
    #[cfg(target_arch = "x86_64")]
    eperm(libc::SYS_ioperm),
    #[cfg(target_arch = "x86_64")]
    eperm(libc::SYS_modify_ldt),
    #[cfg(target_arch = "x86_64")]
    eperm(libc::SYS_uselib),
];

/// `kexec_file_load`, which the libc crate names on every Linux target
/// Link builds for but aarch64 musl: there it's the generic table's 294.
#[cfg(not(all(target_arch = "aarch64", target_env = "musl")))]
const SYS_KEXEC_FILE_LOAD: libc::c_long = libc::SYS_kexec_file_load;
#[cfg(all(target_arch = "aarch64", target_env = "musl"))]
const SYS_KEXEC_FILE_LOAD: libc::c_long = 294;

/// `clone` flags that make a namespace.
const CLONE_NEW: u32 = (libc::CLONE_NEWNS
    | libc::CLONE_NEWCGROUP
    | libc::CLONE_NEWUTS
    | libc::CLONE_NEWIPC
    | libc::CLONE_NEWUSER
    | libc::CLONE_NEWPID
    | libc::CLONE_NEWNET
    | 0x80/* CLONE_NEWTIME */) as u32;

/// `ioctl`s that type into a terminal or reach the console.
const TIOCSTI: u32 = 0x5412;
const TIOCLINUX: u32 = 0x541C;

/// Socket families no network namespace holds, refused whatever network
/// the sandbox has: `AF_VSOCK` reaches the hypervisor and the machine's
/// other virtual machines, `AF_BLUETOOTH` the machine's radios.
const UNSHARED_FAMILIES: &[u32] = &[libc::AF_VSOCK as u32, libc::AF_BLUETOOTH as u32];

/// `execveat`'s flag for running the open file descriptor itself, with no
/// path (`fexecve`): `AT_EMPTY_PATH`.
const AT_EMPTY_PATH: u32 = 0x1000;

/// Mode arguments that could leave a privileged executable on shared
/// storage. NNP protects this worker, but another host user may later run
/// the raw controller-owned inode outside its idmapped view.
const MODES: &[(libc::c_long, u32)] = &[
    (libc::SYS_fchmod, 1),
    (libc::SYS_fchmodat, 2),
    (SYS_FCHMODAT2, 2),
    (libc::SYS_mkdirat, 2),
    (libc::SYS_mknodat, 2),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_chmod, 1),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_creat, 1),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_mkdir, 1),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_mknod, 1),
];

const OPEN_MODES: &[(libc::c_long, u32, u32)] = &[
    (libc::SYS_openat, 2, 3),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_open, 1, 2),
];
const PRIVILEGED_MODE: u32 = libc::S_ISUID | libc::S_ISGID;
// O_TMPFILE contains O_DIRECTORY: only its distinct creation bit counts.
const CREATES: u32 = (libc::O_CREAT | (libc::O_TMPFILE & !libc::O_DIRECTORY)) as u32;

#[cfg(target_arch = "x86_64")]
const ARCH: u32 = 0xC000_003E; // AUDIT_ARCH_X86_64
#[cfg(target_arch = "aarch64")]
const ARCH: u32 = 0xC000_00B7; // AUDIT_ARCH_AARCH64

// Classic BPF, as seccomp runs it.
const LD_ABS_W: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
const JEQ: u16 = 0x15; // BPF_JMP | BPF_JEQ | BPF_K
const JGE: u16 = 0x35; // BPF_JMP | BPF_JGE | BPF_K
const JSET: u16 = 0x45; // BPF_JMP | BPF_JSET | BPF_K
const RET: u16 = 0x06; // BPF_RET | BPF_K
const ALLOW: u32 = 0x7fff_0000; // SECCOMP_RET_ALLOW
const ERRNO: u32 = 0x0005_0000; // SECCOMP_RET_ERRNO
const USER_NOTIF: u32 = 0x7fc0_0000; // SECCOMP_RET_USER_NOTIF

// Where seccomp_data keeps each field (little-endian: an argument's low
// 32 bits come first).
const NR: u32 = 0;
const ARCH_AT: u32 = 4;
const fn arg(i: u32) -> u32 {
    16 + 8 * i
}

/// One BPF instruction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insn {
    pub code: u16,
    pub jt: u8,
    pub jf: u8,
    pub k: u32,
}

const fn insn(code: u16, jt: u8, jf: u8, k: u32) -> Insn {
    Insn { code, jt, jf, k }
}

const fn deny(errno: i32) -> Insn {
    insn(RET, 0, 0, ERRNO | errno as u32)
}

/// The filter, instruction by instruction. Unrestricted execution keeps
/// descriptor execution and memory files. An allowlist refuses descriptor
/// exec and, without a sealing listener, `memfd_create` with `ENOSYS` so
/// applications can use their usual fallback when available.
pub fn program(exec: Exec) -> Vec<Insn> {
    let mut p = vec![
        // Another architecture's system calls (i386 on x86_64) have other
        // numbers: none of them.
        insn(LD_ABS_W, 0, 0, ARCH_AT),
        insn(JEQ, 1, 0, ARCH),
        deny(libc::ENOSYS),
        insn(LD_ABS_W, 0, 0, NR),
    ];
    if cfg!(target_arch = "x86_64") {
        // x32's system calls (the same numbers, with bit 30 set).
        p.extend([insn(JGE, 0, 1, 0x4000_0000), deny(libc::ENOSYS)]);
    }
    for refused in REFUSED {
        p.extend([insn(JEQ, 0, 1, refused.nr as u32), deny(refused.errno)]);
    }
    for &(nr, mode) in MODES {
        p.extend([
            insn(JEQ, 0, 3, nr as u32),
            insn(LD_ABS_W, 0, 0, arg(mode)),
            insn(JSET, 0, 1, PRIVILEGED_MODE),
            deny(libc::EPERM),
            insn(LD_ABS_W, 0, 0, NR),
        ]);
    }
    for &(nr, flags, mode) in OPEN_MODES {
        p.extend([
            insn(JEQ, 0, 5, nr as u32),
            insn(LD_ABS_W, 0, 0, arg(flags)),
            insn(JSET, 0, 3, CREATES),
            insn(LD_ABS_W, 0, 0, arg(mode)),
            insn(JSET, 0, 1, PRIVILEGED_MODE),
            deny(libc::EPERM),
            insn(LD_ABS_W, 0, 0, NR),
        ]);
    }
    if matches!(
        exec,
        Exec::Only {
            memory: MemoryFiles::Denied
        }
    ) {
        p.extend([
            insn(JEQ, 0, 1, libc::SYS_memfd_create as u32),
            deny(libc::ENOSYS),
        ]);
    }
    // clone with a namespace flag (its first argument).
    p.extend([
        insn(JEQ, 0, 3, libc::SYS_clone as u32),
        insn(LD_ABS_W, 0, 0, arg(0)),
        insn(JSET, 0, 1, CLONE_NEW),
        deny(libc::EPERM),
        insn(LD_ABS_W, 0, 0, NR),
    ]);
    // ioctl typing into a terminal (its second argument).
    p.extend([
        insn(JEQ, 0, 4, libc::SYS_ioctl as u32),
        insn(LD_ABS_W, 0, 0, arg(1)),
        insn(JEQ, 1, 0, TIOCSTI),
        insn(JEQ, 0, 1, TIOCLINUX),
        deny(libc::EPERM),
        insn(LD_ABS_W, 0, 0, NR),
    ]);
    // A socket of a family no network namespace holds (its first
    // argument), as a kernel without it would answer.
    for &family in UNSHARED_FAMILIES {
        p.extend([
            insn(JEQ, 0, 3, libc::SYS_socket as u32),
            insn(LD_ABS_W, 0, 0, arg(0)),
            insn(JEQ, 0, 1, family),
            deny(libc::EAFNOSUPPORT),
            insn(LD_ABS_W, 0, 0, NR),
        ]);
    }
    // execveat running a file descriptor itself (AT_EMPTY_PATH, its fifth
    // argument): only where [commands] lists programs, so a program copied
    // into a memory file can't be run around the list. A path exec stays
    // allowed (Landlock judges the file).
    if matches!(exec, Exec::Only { .. }) {
        p.extend([
            insn(JEQ, 0, 3, libc::SYS_execveat as u32),
            insn(LD_ABS_W, 0, 0, arg(4)),
            insn(JSET, 0, 1, AT_EMPTY_PATH),
            deny(libc::EACCES),
            insn(LD_ABS_W, 0, 0, NR),
        ]);
    }
    p.push(insn(RET, 0, 0, ALLOW));
    p
}

/// `memfd_create`'s flags for a memory file that can never be run
/// (`MFD_NOEXEC_SEAL`), and for one that can (`MFD_EXEC`): Linux 6.3 and
/// newer.
pub const MFD_NOEXEC_SEAL: u32 = 0x8;
pub const MFD_EXEC: u32 = 0x10;
/// `memfd_create`'s flag for a memory file of huge pages, on hugetlbfs.
pub const MFD_HUGETLB: u32 = 0x4;

/// The filter that hands each `execve` and `execveat` to a listener
/// ([`listen`]) and lets every other call through: it refuses nothing
/// itself (another architecture's calls are [`program`]'s to refuse).
/// With `memfd`, it also hands over each `memfd_create` that doesn't ask
/// for a memory file that can never be run (`MFD_NOEXEC_SEAL` without
/// `MFD_EXEC`), which the listener makes so instead; one that asks runs
/// as it is, the listener's own among them. One of huge pages
/// (`MFD_HUGETLB`) is refused (`EINVAL`, as a kernel without them says):
/// hugetlbfs doesn't keep the seal, so its mode can be made executable
/// again and the program run.
pub fn exec_notify(memfd: bool) -> Vec<Insn> {
    let mut p = vec![
        insn(LD_ABS_W, 0, 0, ARCH_AT),
        insn(JEQ, 1, 0, ARCH),
        insn(RET, 0, 0, ALLOW),
        insn(LD_ABS_W, 0, 0, NR),
        insn(JEQ, 0, 1, libc::SYS_execve as u32),
        insn(RET, 0, 0, USER_NOTIF),
        insn(JEQ, 0, 1, libc::SYS_execveat as u32),
        insn(RET, 0, 0, USER_NOTIF),
    ];
    if memfd {
        // Its flags are the second argument.
        p.extend([
            insn(JEQ, 0, 7, libc::SYS_memfd_create as u32),
            insn(LD_ABS_W, 0, 0, arg(1)),
            insn(JSET, 0, 1, MFD_HUGETLB),
            deny(libc::EINVAL),
            insn(JSET, 2, 0, MFD_EXEC),
            insn(JSET, 0, 1, MFD_NOEXEC_SEAL),
            insn(RET, 0, 0, ALLOW),
            insn(RET, 0, 0, USER_NOTIF),
        ]);
    }
    p.push(insn(RET, 0, 0, ALLOW));
    p
}

/// Installs [`exec_notify`] on this process, for good, and returns its
/// listener: every `exec` of this process and those it starts waits for
/// the listener's answer (`crate::linux::execs`), and with `memfd` every
/// `memfd_create` that could make a runnable file. Close-on-exec, as the
/// kernel makes it.
pub fn listen(memfd: bool) -> io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;
    let mut filter = sock_filters(exec_notify(memfd));
    let fprog = libc::sock_fprog {
        len: filter.len() as libc::c_ushort,
        filter: filter.as_mut_ptr(),
    };
    // SAFETY: plain prctl and seccomp calls; `fprog` points at `filter`,
    // which lives until the kernel has copied it.
    let fd = unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(io::Error::last_os_error());
        }
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER,
            libc::SECCOMP_FILTER_FLAG_NEW_LISTENER,
            &fprog as *const libc::sock_fprog,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the kernel just gave this descriptor, and nothing else owns it.
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) })
}

fn sock_filters(program: Vec<Insn>) -> Vec<libc::sock_filter> {
    program
        .into_iter()
        .map(|i| libc::sock_filter {
            code: i.code,
            jt: i.jt,
            jf: i.jf,
            k: i.k,
        })
        .collect()
}

/// Installs the filter on this process, for good: it and every program it
/// runs after. No new privileges first, which the kernel requires (and
/// setuid programs lose their power). `exec` chooses how executable
/// restrictions hold (see [`program`]).
pub fn install(exec: Exec) -> io::Result<()> {
    let mut filter = sock_filters(program(exec));
    let fprog = libc::sock_fprog {
        len: filter.len() as libc::c_ushort,
        filter: filter.as_mut_ptr(),
    };
    // SAFETY: plain prctl calls; `fprog` points at `filter`, which lives
    // until the kernel has copied it.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            &fprog as *const libc::sock_fprog,
        ) != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `program` as seccomp would for a call `nr` with `args`.
    fn run(program: &[Insn], arch: u32, nr: i64, args: [u64; 6]) -> u32 {
        let word = |at: u32| -> u32 {
            match at {
                NR => nr as u32,
                ARCH_AT => arch,
                at => args[((at - 16) / 8) as usize] as u32,
            }
        };
        let (mut pc, mut a) = (0usize, 0u32);
        loop {
            let i = program[pc];
            pc += 1;
            match i.code {
                LD_ABS_W => a = word(i.k),
                RET => return i.k,
                JEQ | JGE | JSET => {
                    let taken = match i.code {
                        JEQ => a == i.k,
                        JGE => a >= i.k,
                        _ => a & i.k != 0,
                    };
                    pc += if taken { i.jt } else { i.jf } as usize;
                }
                other => panic!("unknown instruction {other:#x}"),
            }
        }
    }

    fn verdict(nr: i64, args: [u64; 6]) -> u32 {
        run(&program(Exec::Any), ARCH, nr, args)
    }

    #[test]
    fn refused_calls_get_their_error_and_the_rest_run() {
        let none = [0; 6];
        assert_eq!(
            verdict(libc::SYS_io_uring_setup, none),
            ERRNO | libc::ENOSYS as u32
        );
        assert_eq!(verdict(libc::SYS_bpf, none), ERRNO | libc::EPERM as u32);
        assert_eq!(verdict(libc::SYS_unshare, none), ERRNO | libc::EPERM as u32);
        assert_eq!(verdict(libc::SYS_clone3, none), ERRNO | libc::ENOSYS as u32);
        for fine in [
            libc::SYS_read,
            libc::SYS_write,
            libc::SYS_openat,
            libc::SYS_execve,
        ] {
            assert_eq!(verdict(fine, none), ALLOW, "{fine}");
        }
        for refused in REFUSED {
            assert_ne!(verdict(refused.nr, none), ALLOW, "{}", refused.nr);
        }
    }

    #[test]
    fn only_an_exec_is_handed_to_the_listener() {
        let none = [0; 6];
        for memfd in [false, true] {
            let notify = exec_notify(memfd);
            for exec in [libc::SYS_execve, libc::SYS_execveat] {
                assert_eq!(run(&notify, ARCH, exec, none), USER_NOTIF, "{exec}");
            }
            for other in [libc::SYS_read, libc::SYS_openat, libc::SYS_clone] {
                assert_eq!(run(&notify, ARCH, other, none), ALLOW, "{other}");
            }
            assert_eq!(run(&notify, 0x4000_0003, libc::SYS_execve, none), ALLOW);
        }
        // Without `memfd`, a memory file is made as asked.
        let plain = exec_notify(false);
        assert_eq!(run(&plain, ARCH, libc::SYS_memfd_create, none), ALLOW);
    }

    #[test]
    fn a_memory_file_that_could_run_is_handed_to_the_listener() {
        let notify = exec_notify(true);
        let flags = |flags: u32| {
            let args = [0x1000, flags as u64, 0, 0, 0, 0];
            run(&notify, ARCH, libc::SYS_memfd_create, args)
        };
        const CLOEXEC: u32 = 0x1;
        assert_eq!(flags(0), USER_NOTIF);
        assert_eq!(flags(CLOEXEC), USER_NOTIF);
        assert_eq!(flags(MFD_EXEC), USER_NOTIF);
        assert_eq!(flags(MFD_EXEC | MFD_NOEXEC_SEAL), USER_NOTIF);
        // Asking for memory that never runs (the listener's own call).
        assert_eq!(flags(MFD_NOEXEC_SEAL), ALLOW);
        assert_eq!(flags(MFD_NOEXEC_SEAL | CLOEXEC), ALLOW);
        // Huge pages, whose seal doesn't hold: refused, however asked.
        let einval = ERRNO | libc::EINVAL as u32;
        assert_eq!(flags(MFD_HUGETLB), einval);
        assert_eq!(flags(MFD_HUGETLB | MFD_NOEXEC_SEAL), einval);
        assert_eq!(flags(MFD_HUGETLB | MFD_NOEXEC_SEAL | CLOEXEC), einval);
        // A huge page size in the high bits doesn't hide it.
        assert_eq!(flags(MFD_HUGETLB | (21 << 26)), einval);
    }

    #[test]
    fn unavailable_sealing_or_listener_denies_memory_files_without_widening_execution() {
        for (sealing, listening) in [(false, false), (false, true), (true, false), (true, true)] {
            let memory = MemoryFiles::for_watch(sealing, listening);
            assert_eq!(memory == MemoryFiles::Sealed, sealing && listening);
            let filter = program(Exec::Only { memory });
            for flags in [
                0,
                1,
                MFD_EXEC,
                MFD_NOEXEC_SEAL,
                MFD_EXEC | MFD_NOEXEC_SEAL,
                MFD_HUGETLB,
                MFD_HUGETLB | MFD_NOEXEC_SEAL,
            ] {
                let args = [0x1000, flags as u64, 0, 0, 0, 0];
                let want = if memory == MemoryFiles::Denied {
                    ERRNO | libc::ENOSYS as u32
                } else {
                    ALLOW
                };
                assert_eq!(
                    run(&filter, ARCH, libc::SYS_memfd_create, args),
                    want,
                    "{memory:?}, flags {flags:#x}"
                );
                assert_eq!(
                    run(&program(Exec::Any), ARCH, libc::SYS_memfd_create, args),
                    ALLOW,
                    "unrestricted execution keeps memory files"
                );
            }
            assert_eq!(
                run(
                    &filter,
                    ARCH,
                    libc::SYS_execveat,
                    [0, 0, 0, 0, AT_EMPTY_PATH as u64, 0]
                ),
                ERRNO | libc::EACCES as u32,
                "descriptor exec remains closed"
            );
            for syscall in [
                libc::SYS_read,
                libc::SYS_openat,
                libc::SYS_mmap,
                libc::SYS_execve,
            ] {
                assert_eq!(run(&filter, ARCH, syscall, [0; 6]), ALLOW, "{syscall}");
            }
        }
    }

    #[test]
    fn clone_and_ioctl_are_judged_by_their_arguments() {
        let thread = (libc::CLONE_VM | libc::CLONE_THREAD | libc::CLONE_SIGHAND) as u64;
        assert_eq!(verdict(libc::SYS_clone, [thread, 0, 0, 0, 0, 0]), ALLOW);
        let userns = libc::CLONE_NEWUSER as u64;
        assert_eq!(
            verdict(libc::SYS_clone, [userns, 0, 0, 0, 0, 0]),
            ERRNO | libc::EPERM as u32
        );
        assert_eq!(
            verdict(libc::SYS_ioctl, [0, TIOCSTI as u64, 0, 0, 0, 0]),
            ERRNO | libc::EPERM as u32
        );
        assert_eq!(
            verdict(libc::SYS_ioctl, [0, TIOCLINUX as u64, 0, 0, 0, 0]),
            ERRNO | libc::EPERM as u32
        );
        // TCGETS: what every terminal program asks.
        assert_eq!(verdict(libc::SYS_ioctl, [0, 0x5401, 0, 0, 0, 0]), ALLOW);
    }

    /// A vsock or Bluetooth socket is refused, since no network namespace
    /// holds either; the families a namespace holds are not.
    #[test]
    fn sockets_no_network_namespace_holds_are_refused() {
        let socket = |family: i32| verdict(libc::SYS_socket, [family as u64, 1, 0, 0, 0, 0]);
        for family in [libc::AF_VSOCK, libc::AF_BLUETOOTH] {
            assert_eq!(
                socket(family),
                ERRNO | libc::EAFNOSUPPORT as u32,
                "{family}"
            );
        }
        for family in [
            libc::AF_UNIX,
            libc::AF_INET,
            libc::AF_INET6,
            libc::AF_NETLINK,
        ] {
            assert_eq!(socket(family), ALLOW, "{family}");
        }
    }

    #[test]
    fn no_mode_change_or_creation_can_leave_setid_artifacts() {
        for &(nr, mode) in MODES {
            for bits in [0o600, 0o755, 0o1755, 0o2755, 0o4755, 0o6755] {
                let mut args = [0; 6];
                args[mode as usize] = bits;
                assert_eq!(
                    verdict(nr, args),
                    if bits & u64::from(PRIVILEGED_MODE) == 0 {
                        ALLOW
                    } else {
                        ERRNO | libc::EPERM as u32
                    },
                    "syscall {nr}, mode {bits:o}"
                );
            }
        }
        for &(nr, flags, mode) in OPEN_MODES {
            for creating in [0, libc::O_DIRECTORY, libc::O_CREAT, libc::O_TMPFILE] {
                let mut args = [0; 6];
                args[flags as usize] = creating as u64;
                args[mode as usize] = 0o6755;
                assert_eq!(
                    verdict(nr, args),
                    if creating as u32 & CREATES == 0 {
                        ALLOW
                    } else {
                        ERRNO | libc::EPERM as u32
                    }
                );
                args[mode as usize] = 0o600;
                assert_eq!(verdict(nr, args), ALLOW);
            }
        }
        assert_eq!(
            verdict(libc::SYS_openat2, [0; 6]),
            ERRNO | libc::ENOSYS as u32
        );
        assert_eq!(verdict(libc::SYS_read, [u64::MAX; 6]), ALLOW);
    }

    #[test]
    fn other_architectures_run_nothing() {
        let i386 = 0x4000_0003;
        assert_eq!(
            run(&program(Exec::Any), i386, libc::SYS_read, [0; 6]),
            ERRNO | libc::ENOSYS as u32
        );
        if cfg!(target_arch = "x86_64") {
            assert_eq!(
                verdict(0x4000_0000 | libc::SYS_read, [0; 6]),
                ERRNO | libc::ENOSYS as u32
            );
        }
    }

    #[test]
    fn running_a_file_descriptor_is_refused_only_with_fd_exec() {
        // execveat with a real path (AT_EMPTY_PATH not set): Landlock's to
        // judge, left alone either way.
        let path = [0u64, 0, 0, 0, 0, 0];
        let empty = [0u64, 0, 0, 0, AT_EMPTY_PATH as u64, 0];
        for exec in [
            Exec::Any,
            Exec::Only {
                memory: MemoryFiles::Sealed,
            },
            Exec::Only {
                memory: MemoryFiles::Denied,
            },
        ] {
            let v = |args: [u64; 6]| run(&program(exec), ARCH, libc::SYS_execveat, args);
            assert_eq!(v(path), ALLOW, "{exec:?}");
            assert_eq!(
                v(empty),
                if matches!(exec, Exec::Only { .. }) {
                    ERRNO | libc::EACCES as u32
                } else {
                    ALLOW
                },
                "{exec:?}"
            );
        }
        // Plain execve is never touched.
        assert_eq!(
            run(
                &program(Exec::Only {
                    memory: MemoryFiles::Sealed
                }),
                ARCH,
                libc::SYS_execve,
                [0; 6]
            ),
            ALLOW
        );
    }

    #[test]
    fn every_jump_lands_inside_the_program() {
        for program in [
            program(Exec::Any),
            program(Exec::Only {
                memory: MemoryFiles::Sealed,
            }),
            program(Exec::Only {
                memory: MemoryFiles::Denied,
            }),
            exec_notify(false),
            exec_notify(true),
        ] {
            assert!(program.len() < u16::MAX as usize);
            for (pc, i) in program.iter().enumerate() {
                if i.code != RET && i.code != LD_ABS_W {
                    assert!(pc + 1 + (i.jt.max(i.jf) as usize) < program.len(), "{pc}");
                }
            }
            assert_eq!(program.last().map(|i| i.code), Some(RET));
        }
    }

    #[test]
    fn fchmodat2_is_one_number_on_every_architecture() {
        // Where libc names it, it names the number every other target is given.
        #[cfg(target_arch = "x86_64")]
        assert_eq!(SYS_FCHMODAT2, libc::SYS_fchmodat2);
        assert_eq!(SYS_FCHMODAT2, 452);
    }
}

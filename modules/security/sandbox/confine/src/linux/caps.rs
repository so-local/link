//! What this machine's kernel and bubblewrap can enforce, probed once
//! into one value so each wall engages from the same reading rather than
//! from a `cfg!` or a probe scattered at its use. The sandbox runs with
//! whatever it finds: a missing capability drops its layer, it never
//! replaces one with something weaker ([`crate::linux`]).

use std::num::NonZeroU32;

/// The capabilities a Linux sandbox's walls rest on, as this machine
/// reports them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// The Landlock ABI the kernel reports, or None where Landlock is
    /// unavailable ([`super::landlock::probe`]). Each right the wall uses
    /// is masked to what the ABI knows.
    pub landlock: Option<NonZeroU32>,
    /// The seccomp filter and the exec watch that rides on it are built
    /// for this architecture (x86_64 and aarch64): [`super::FILTERED`].
    pub seccomp: bool,
    /// `memfd_create` can seal a memory file non-executable (Linux 6.3 and
    /// newer), so a listed-command sandbox need not refuse memory files
    /// outright ([`super::execs::sealable`]).
    pub memfd_seal: bool,
    /// `close_range` marks every inherited descriptor close-on-exec in one
    /// call (Linux 5.11 and newer); without it the first program inside
    /// walks `/proc/self/fd` ([`super::enter`]).
    pub close_range: bool,
    /// bubblewrap can refuse new user namespaces inside (`--disable-userns`,
    /// 0.8 and newer), which open much of the kernel to an unprivileged
    /// program.
    pub no_userns: bool,
}

impl Capabilities {
    /// What this machine can enforce, probed now. Cheap: a version query
    /// each (no descriptor, no ruleset), and bubblewrap's version from its
    /// cached answer ([`super::bwrap::disables_userns`]).
    pub fn here() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self {
                landlock: super::landlock::probe().ok(),
                seccomp: super::FILTERED,
                memfd_seal: memfd_seal(),
                close_range: close_range(),
                no_userns: super::bwrap::bwrap().is_ok_and(super::bwrap::disables_userns),
            }
        }
        #[cfg(not(target_os = "linux"))]
        Self::default()
    }

    /// The Landlock ABI, when the kernel has one.
    pub fn landlock_abi(&self) -> Option<u32> {
        self.landlock.map(NonZeroU32::get)
    }
}

/// Whether `memfd_create` can seal a memory file non-executable here.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn memfd_seal() -> bool {
    super::execs::sealable()
}

/// No exec watch is built off x86_64 and aarch64, so sealing never
/// engages there.
#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "x86_64", target_arch = "aarch64"))
))]
fn memfd_seal() -> bool {
    false
}

/// Whether `close_range` with `CLOSE_RANGE_CLOEXEC` is available: ask the
/// kernel to mark a one-descriptor range that can hold nothing open
/// (`u32::MAX`). A supporting kernel answers 0; an older one fails the
/// syscall or the flag.
#[cfg(target_os = "linux")]
fn close_range() -> bool {
    // SAFETY: a flag-only call on a single descriptor number that is never
    // open; it closes nothing and changes no state.
    let marked = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            libc::c_uint::MAX,
            libc::c_uint::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    };
    marked == 0
}

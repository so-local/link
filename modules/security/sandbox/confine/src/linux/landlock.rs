//! Landlock (Linux 5.13 and newer): the only files a sandbox may run, a
//! spec's `[commands]`. Applied by `sandbox confine`, the first program
//! inside, before it becomes the program; every program started after
//! inherits it. It limits which files are run, not what code runs: a
//! listed interpreter runs any script, the loader any file it can read
//! (`ld.so ./x`), and a listed program a library `LD_PRELOAD` names.
//!
//! Landlock judges the file `exec` is given, `/proc/self/fd/<n>` of a
//! file the sandbox wrote among them. A program copied into a memory file
//! (`memfd_create`) is in no folder, so Landlock can't judge it; the exec
//! watch makes such memory non-executable instead (`crate::linux::execs`).
//! Without sealing support or a listener, the base filter refuses memory
//! file creation. It also refuses running a file descriptor. The loader must stay
//! runnable (every dynamically linked program needs it run on its
//! behalf), so `ld.so ./x` stays open; the sandbox's `security.md` says
//! so.

use std::io;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

/// Landlock's filesystem access rights, by their ABI bit. A ruleset
/// handles a set of these, and each path it allows names a subset; a
/// right the running ABI doesn't know makes the ruleset invalid, so
/// [`file_rights`] masks to what the kernel reports.
#[cfg(target_os = "linux")]
mod fs {
    pub const EXECUTE: u64 = 1 << 0;
    pub const WRITE_FILE: u64 = 1 << 1;
    pub const READ_FILE: u64 = 1 << 2;
    pub const READ_DIR: u64 = 1 << 3;
    pub const REMOVE_DIR: u64 = 1 << 4;
    pub const REMOVE_FILE: u64 = 1 << 5;
    pub const MAKE_CHAR: u64 = 1 << 6;
    pub const MAKE_DIR: u64 = 1 << 7;
    pub const MAKE_REG: u64 = 1 << 8;
    pub const MAKE_SOCK: u64 = 1 << 9;
    pub const MAKE_FIFO: u64 = 1 << 10;
    pub const MAKE_BLOCK: u64 = 1 << 11;
    pub const MAKE_SYM: u64 = 1 << 12;
    /// ABI 2 and newer: a rename or hard link that crosses directories.
    pub const REFER: u64 = 1 << 13;
    /// ABI 3 and newer: truncating a file (`ftruncate`, `open` `O_TRUNC`).
    pub const TRUNCATE: u64 = 1 << 14;

    /// The rights that apply to a regular file, never a folder: Landlock
    /// refuses a folder-only right (`READ_DIR`, `MAKE_*`, `REFER`) on a
    /// file, so a rule over one is masked to these.
    pub const FILE: u64 = EXECUTE | READ_FILE | WRITE_FILE | TRUNCATE;

    /// What reading a file or listing a folder needs.
    pub const READ: u64 = READ_FILE | READ_DIR;
    /// What making, changing, removing or renaming within a folder needs,
    /// through ABI 1's rights (the newer ones are added by [`super::file_rights`]).
    pub const WRITE: u64 = WRITE_FILE
        | REMOVE_DIR
        | REMOVE_FILE
        | MAKE_CHAR
        | MAKE_DIR
        | MAKE_REG
        | MAKE_SOCK
        | MAKE_FIFO
        | MAKE_BLOCK
        | MAKE_SYM;
}

/// Landlock's scoping rights (ABI 6 and newer): a ruleset that handles one
/// keeps the process from reaching, across its Landlock domain, what the
/// mounts and the PID namespace don't already close.
#[cfg(target_os = "linux")]
mod scope {
    /// Connecting to an abstract unix socket owned outside the domain —
    /// X11's among them, which a host-network sandbox could otherwise reach.
    pub const ABSTRACT_UNIX_SOCKET: u64 = 1 << 0;
    /// Sending a signal to a process outside the domain.
    pub const SIGNAL: u64 = 1 << 1;
}

/// What the file wall handles and allows at `abi`: every read and write
/// right the running kernel knows. A rule over a read-only grant allows
/// the read subset; a rule over a writable grant allows all of it.
/// `REFER` (ABI 2) keeps cross-directory renames inside a writable grant
/// working, and `TRUNCATE` (ABI 3) `ftruncate`; without them the kernel
/// would refuse those even where the grant allows the files.
#[cfg(target_os = "linux")]
fn file_rights(abi: u32) -> (u64, u64) {
    let mut handled = fs::READ | fs::WRITE;
    if abi >= 2 {
        handled |= fs::REFER;
    }
    if abi >= 3 {
        handled |= fs::TRUNCATE;
    }
    (handled, fs::READ)
}

/// What a failed Landlock version probe observed. An unavailable syscall
/// alone does not tell a missing kernel implementation from a syscall
/// hidden by the outer runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    SyscallUnavailable,
    Disabled,
    Denied,
    Other,
}

/// The OS error from a failed Landlock version probe, kept intact for
/// both preflight diagnostics and the check inside the sandbox.
#[derive(Debug)]
pub struct ProbeError(io::Error);

impl ProbeError {
    pub fn reason(&self) -> Reason {
        match self.0.raw_os_error() {
            Some(libc::ENOSYS) => Reason::SyscallUnavailable,
            Some(libc::EOPNOTSUPP) => Reason::Disabled,
            Some(libc::EPERM | libc::EACCES) => Reason::Denied,
            _ => Reason::Other,
        }
    }

    pub fn os_error(&self) -> &io::Error {
        &self.0
    }
}

impl From<io::Error> for ProbeError {
    fn from(error: io::Error) -> Self {
        Self(error)
    }
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let observation = match self.reason() {
            Reason::SyscallUnavailable => {
                "syscall unavailable; kernel support may be absent or an outer runtime may filter it"
            }
            Reason::Disabled => "support disabled",
            Reason::Denied => "probe denied",
            Reason::Other => "probe failed",
        };
        write!(f, "Landlock version probe: {observation} ({})", self.0)
    }
}

impl std::error::Error for ProbeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// The ABI reported by the running kernel, or the observed OS error.
/// A successful query is a capability observation, not proof that a
/// ruleset can be installed: [`restrict_exec`] checks again and refuses
/// every installation failure before the requested program runs.
pub fn probe() -> Result<NonZeroU32, ProbeError> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: a version query uses no userspace attribute and no
        // descriptor. LANDLOCK_CREATE_RULESET_VERSION is flag 1.
        let version = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<u8>(),
                0usize,
                1u32,
            )
        };
        if version < 0 {
            return Err(io::Error::last_os_error().into());
        }
        u32::try_from(version)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Landlock version probe returned {version}"),
                )
                .into()
            })
    }
    #[cfg(not(target_os = "linux"))]
    Err(io::Error::new(io::ErrorKind::Unsupported, "not running on Linux").into())
}

/// The program loaders here (`/lib64/ld-linux-x86-64.so.2`, musl's):
/// every dynamically linked program needs its loader run.
pub fn loaders() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in ["/lib", "/lib64", "/usr/lib", "/usr/lib64"] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("ld-linux") || name.starts_with("ld-musl") {
                found.push(entry.path());
            }
        }
        // Multiarch: /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2.
        for arch in ["x86_64-linux-gnu", "aarch64-linux-gnu"] {
            for loader in ["ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1"] {
                let path = Path::new(dir).join(arch).join(loader);
                if path.exists() {
                    found.push(path);
                }
            }
        }
    }
    found
}

/// Allows running only `allowed` (files, or folders for every file in
/// them) from here on, for this process and every one it starts.
#[cfg(target_os = "linux")]
pub fn restrict_exec(allowed: &[PathBuf]) -> anyhow::Result<()> {
    restrict_exec_with(&Kernel, allowed)
}

/// The operations an executable ruleset needs. The implementation uses
/// the kernel; tests inject failures without depending on the host's
/// Landlock support or restricting the test process itself.
#[cfg(target_os = "linux")]
trait Enforcement {
    type Ruleset;

    fn probe(&self) -> Result<NonZeroU32, ProbeError>;
    /// A ruleset handling `access_fs` and `access_scope` (the rights it
    /// governs; what it does not handle it does not restrict). A nonzero
    /// `access_scope` needs ABI 6 and the larger attribute.
    fn create(&self, access_fs: u64, access_scope: u64) -> io::Result<Self::Ruleset>;
    /// Allow `access` on `path` and everything beneath it.
    fn allow(&self, ruleset: &Self::Ruleset, path: &Path, access: u64) -> io::Result<()>;
    fn no_new_privileges(&self) -> io::Result<()>;
    fn restrict(&self, ruleset: &Self::Ruleset) -> io::Result<()>;
}

#[cfg(target_os = "linux")]
fn restrict_exec_with(kernel: &impl Enforcement, allowed: &[PathBuf]) -> anyhow::Result<()> {
    use anyhow::Context;

    let abi = kernel
        .probe()
        .context("enforcing the requested executable policy: Landlock unavailable")?;
    // A layer that leaves `REFER` unhandled refuses every link or rename
    // into another folder, so this one handles it and allows it
    // everywhere: whether a move is allowed stays the file wall's.
    let refer = match abi.get() >= 2 {
        true => fs::REFER,
        false => 0,
    };
    let ruleset = kernel
        .create(fs::EXECUTE | refer, 0)
        .context("creating the Landlock executable ruleset")?;
    if refer != 0 {
        kernel
            .allow(&ruleset, Path::new("/"), refer)
            .context("allowing moves between folders with Landlock")?;
    }
    for path in allowed {
        kernel
            .allow(&ruleset, path, fs::EXECUTE)
            .with_context(|| format!("allowing {} to run with Landlock", path.display()))?;
    }
    kernel
        .no_new_privileges()
        .context("setting no new privileges for Landlock")?;
    kernel
        .restrict(&ruleset)
        .context("applying the Landlock executable ruleset")?;
    Ok(())
}

/// A second, independent file wall: beyond bubblewrap's mounts, Landlock
/// (where the kernel has it) allows reading only `read` and reading and
/// writing only `write`, each path and everything beneath it, and refuses
/// every other path, so a sandboxed program held to its mounts is held to
/// the same files again by the kernel itself. It is additive: where
/// Landlock is absent the mounts stand alone and this does nothing. The
/// caller passes the whole allow-set, the system's read-only folders among
/// it ([`crate::linux::enter`]).
#[cfg(target_os = "linux")]
pub fn restrict_files(read: &[PathBuf], write: &[PathBuf]) -> anyhow::Result<()> {
    restrict_files_with(&Kernel, read, write, false)
}

/// [`restrict_files`], for a wall that was asked for (`--file-wall`): a
/// Landlock probe that fails inside refuses the launch, rather than run
/// the program with the mounts alone where the wall was promised.
#[cfg(target_os = "linux")]
pub fn require_files(read: &[PathBuf], write: &[PathBuf]) -> anyhow::Result<()> {
    restrict_files_with(&Kernel, read, write, true)
}

#[cfg(target_os = "linux")]
fn restrict_files_with(
    kernel: &impl Enforcement,
    read: &[PathBuf],
    write: &[PathBuf],
    required: bool,
) -> anyhow::Result<()> {
    use anyhow::Context;

    // Absent Landlock the mounts are the only file wall; adding nothing
    // keeps the sandbox running exactly as it does without it, unless the
    // wall was asked for.
    let abi = match kernel.probe() {
        Ok(abi) => abi,
        Err(err) if required => {
            return Err(anyhow::Error::new(err)
                .context("the file wall was asked for, and Landlock isn't here inside"));
        }
        Err(_) => return Ok(()),
    };
    let (handled, read_access) = file_rights(abi.get());
    // ABI 6 and newer also scopes the domain: a sandboxed program can't
    // reach the machine's abstract unix sockets (X11's among them, open to
    // a host-network sandbox before this) or signal a process outside the
    // sandbox. Older kernels don't know the right, so it engages only there.
    let scoped = if abi.get() >= 6 {
        scope::ABSTRACT_UNIX_SOCKET | scope::SIGNAL
    } else {
        0
    };
    let ruleset = kernel
        .create(handled, scoped)
        .context("creating the Landlock file ruleset")?;
    for path in read {
        kernel
            .allow(&ruleset, path, read_access)
            .with_context(|| format!("allowing {} to be read with Landlock", path.display()))?;
    }
    for path in write {
        kernel
            .allow(&ruleset, path, handled)
            .with_context(|| format!("allowing {} to be written with Landlock", path.display()))?;
    }
    kernel
        .no_new_privileges()
        .context("setting no new privileges for Landlock")?;
    kernel
        .restrict(&ruleset)
        .context("applying the Landlock file ruleset")?;
    Ok(())
}

/// Whether the open descriptor (`O_PATH` is fine) is a directory, so a
/// rule over it may carry the folder rights.
#[cfg(target_os = "linux")]
fn is_dir(file: &std::fs::File) -> io::Result<bool> {
    use std::os::fd::AsRawFd;
    // SAFETY: a zeroed stat buffer filled by fstat on an open descriptor.
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    if unsafe { libc::fstat(file.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stat.st_mode & libc::S_IFMT == libc::S_IFDIR)
}

#[cfg(target_os = "linux")]
struct Kernel;

#[cfg(target_os = "linux")]
impl Enforcement for Kernel {
    type Ruleset = std::os::fd::OwnedFd;

    fn probe(&self) -> Result<NonZeroU32, ProbeError> {
        probe()
    }

    fn create(&self, access_fs: u64, access_scope: u64) -> io::Result<std::os::fd::OwnedFd> {
        use std::os::fd::FromRawFd;

        // ABI 6's attribute, with the network (ABI 4) and scope (ABI 6)
        // fields between. The kernel reads only the length passed, so a
        // scope-free ruleset passes the shorter length an older kernel
        // knows; a scoped one passes the whole attribute and needs ABI 6.
        #[repr(C)]
        struct RulesetAttr {
            handled_access_fs: u64,
            handled_access_net: u64,
            handled_access_scope: u64,
        }
        let attr = RulesetAttr {
            handled_access_fs: access_fs,
            handled_access_net: 0,
            handled_access_scope: access_scope,
        };
        let len = if access_scope == 0 {
            std::mem::size_of::<u64>()
        } else {
            std::mem::size_of::<RulesetAttr>()
        };
        // SAFETY: the attribute's layout is Landlock's ABI, and `len` never
        // exceeds the attribute's size.
        let ruleset = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &attr as *const RulesetAttr,
                len,
                0u32,
            )
        };
        if ruleset < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor owned only here. Every error path
        // closes it, including a failure while adding a rule.
        Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(ruleset as i32) })
    }

    fn allow(&self, ruleset: &std::os::fd::OwnedFd, path: &Path, access: u64) -> io::Result<()> {
        use std::os::fd::AsRawFd;

        #[repr(C, packed)]
        struct PathBeneath {
            allowed_access: u64,
            parent_fd: i32,
        }
        // A path not there (a folder listed for later) allows nothing.
        let file = match link_files::nofollow::open_path(path, 0) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        // Landlock refuses a directory's rights on a file (`EINVAL`): a
        // regular file (a program, a granted file) carries only the rights
        // that apply to a file, never `READ_DIR`, `MAKE_*` or `REFER`.
        let allowed = if is_dir(&file)? {
            access
        } else {
            access & fs::FILE
        };
        let rule = PathBeneath {
            allowed_access: allowed,
            parent_fd: file.as_raw_fd(),
        };
        // SAFETY: both descriptors are open, and the packed attribute
        // has Landlock's required layout.
        let added = unsafe {
            libc::syscall(
                libc::SYS_landlock_add_rule,
                ruleset.as_raw_fd(),
                1u32, // LANDLOCK_RULE_PATH_BENEATH.
                &rule as *const PathBeneath,
                0u32,
            )
        };
        if added == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn no_new_privileges(&self) -> io::Result<()> {
        // SAFETY: changes only this process's irreversible privilege flag.
        let set = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        if set == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn restrict(&self, ruleset: &std::os::fd::OwnedFd) -> io::Result<()> {
        use std::os::fd::AsRawFd;

        // SAFETY: an open ruleset descriptor, restricted on this process.
        let restricted =
            unsafe { libc::syscall(libc::SYS_landlock_restrict_self, ruleset.as_raw_fd(), 0u32) };
        if restricted == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn probe_failures_keep_the_observation_and_os_error() {
        use std::error::Error;

        for (errno, reason, message) in [
            (
                libc::ENOSYS,
                Reason::SyscallUnavailable,
                "kernel support may be absent or an outer runtime may filter it",
            ),
            (libc::EOPNOTSUPP, Reason::Disabled, "support disabled"),
            (libc::EPERM, Reason::Denied, "probe denied"),
            (libc::EACCES, Reason::Denied, "probe denied"),
            (libc::EINVAL, Reason::Other, "probe failed"),
        ] {
            let error = ProbeError::from(io::Error::from_raw_os_error(errno));
            assert_eq!(error.reason(), reason);
            assert_eq!(error.os_error().raw_os_error(), Some(errno));
            assert_eq!(
                error
                    .source()
                    .unwrap()
                    .downcast_ref::<io::Error>()
                    .unwrap()
                    .raw_os_error(),
                Some(errno)
            );
            assert!(error.to_string().contains(message), "{error}");
            assert!(
                error.to_string().contains(&format!("os error {errno}")),
                "{error}"
            );
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Step {
        Probe,
        Create,
        Add,
        Privileges,
        Restrict,
    }

    struct Fake {
        fail: Option<Step>,
        steps: std::cell::RefCell<Vec<Step>>,
        closed: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl Fake {
        fn step(&self, step: Step) -> io::Result<()> {
            self.steps.borrow_mut().push(step);
            if self.fail == Some(step) {
                Err(io::Error::from_raw_os_error(libc::EPERM))
            } else {
                Ok(())
            }
        }
    }

    struct FakeRuleset(std::rc::Rc<std::cell::Cell<bool>>);

    impl Drop for FakeRuleset {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    impl Enforcement for Fake {
        type Ruleset = FakeRuleset;

        fn probe(&self) -> Result<NonZeroU32, ProbeError> {
            self.step(Step::Probe)?;
            Ok(NonZeroU32::new(6).unwrap())
        }
        fn create(&self, _access_fs: u64, _access_scope: u64) -> io::Result<Self::Ruleset> {
            self.step(Step::Create)?;
            Ok(FakeRuleset(self.closed.clone()))
        }
        fn allow(&self, _: &Self::Ruleset, _: &Path, _access: u64) -> io::Result<()> {
            self.step(Step::Add)
        }
        fn no_new_privileges(&self) -> io::Result<()> {
            self.step(Step::Privileges)
        }
        fn restrict(&self, _: &Self::Ruleset) -> io::Result<()> {
            self.step(Step::Restrict)
        }
    }

    /// With the file wall asked for, a Landlock probe that fails inside
    /// refuses the launch; without it, the mounts stand alone.
    #[test]
    fn the_walls_inside_refuse_what_they_cant_hold() {
        let failing = || Fake {
            fail: Some(Step::Probe),
            steps: Default::default(),
            closed: Default::default(),
        };
        assert!(restrict_files_with(&failing(), &[], &[], true).is_err());
        assert!(restrict_files_with(&failing(), &[], &[], false).is_ok());
    }

    #[test]
    fn every_installation_failure_stops_and_closes_the_ruleset() {
        // Moves between folders allowed, then the program: two rules.
        let steps = [
            Step::Probe,
            Step::Create,
            Step::Add,
            Step::Add,
            Step::Privileges,
            Step::Restrict,
        ];
        for (at, failed) in steps.iter().enumerate() {
            // A step failing fails where it first comes.
            if steps[..at].contains(failed) {
                continue;
            }
            let kernel = Fake {
                fail: Some(*failed),
                steps: Default::default(),
                closed: Default::default(),
            };
            let error = restrict_exec_with(&kernel, &["/allowed/program".into()]).unwrap_err();
            assert_eq!(*kernel.steps.borrow(), steps[..=at], "{failed:?}");
            assert_eq!(
                error
                    .root_cause()
                    .downcast_ref::<io::Error>()
                    .unwrap()
                    .raw_os_error(),
                Some(libc::EPERM),
                "{error:#}"
            );
            assert_eq!(
                kernel.closed.get(),
                at > 1,
                "ruleset cleanup after {failed:?}"
            );
        }
        let kernel = Fake {
            fail: None,
            steps: Default::default(),
            closed: Default::default(),
        };
        restrict_exec_with(&kernel, &["/allowed/program".into()]).unwrap();
        assert_eq!(*kernel.steps.borrow(), steps);
        let kernel = Fake {
            fail: None,
            steps: Default::default(),
            closed: Default::default(),
        };
        restrict_exec_with(&kernel, &[]).unwrap();
        assert_eq!(
            *kernel.steps.borrow(),
            [
                Step::Probe,
                Step::Create,
                Step::Add,
                Step::Privileges,
                Step::Restrict
            ],
            "an internal empty list still installs a deny-by-default ruleset"
        );
    }

    #[test]
    fn only_what_is_allowed_runs() {
        if let Err(error) = probe() {
            link_test_kit::gate::lacks("Landlock", error, Some(link_test_kit::gate::NATIVE));
            return;
        }
        let find = |name: &str| {
            link_files::find_program(name, &std::env::var_os("PATH").unwrap()).unwrap()
        };
        let (sh, true_, ls) = (find("sh"), find("true"), find("ls"));
        // In a child, so this test's own process stays unrestricted.
        let mut allowed = vec![sh.clone(), true_.clone()];
        allowed.extend(loaders());
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            let code = match restrict_exec(&allowed) {
                Err(_) => 3,
                Ok(()) => {
                    let ran = |p: &Path| {
                        link_secretive::process::command(p)
                            .status()
                            .map(|s| s.success())
                    };
                    match (ran(&true_), ran(&ls)) {
                        (Ok(true), Err(_)) => match restrict_exec(&[]) {
                            Ok(()) if ran(&true_).is_err() => 0,
                            _ => 4,
                        },
                        (Ok(true), Ok(_)) => 1,
                        _ => 2,
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
            "1: ls ran; 2: true didn't; 3: no ruleset; 4: deny-by-default didn't hold"
        );
    }

    /// Under the wall, a folder is made inside a write grant by walking
    /// to it from `/` (`link_files::nofollow::Dir::passing`): the folders
    /// above it, which it may neither read nor write, are passed through.
    #[test]
    fn a_folder_in_a_write_grant_is_made_from_the_root_down() {
        if let Err(error) = probe() {
            link_test_kit::gate::lacks("Landlock", error, Some(link_test_kit::gate::NATIVE));
            return;
        }
        let root = link_test_kit::TempDir::new("file-wall-walk");
        let root = root.canonicalize().unwrap();
        let writable = root.join("writable");
        std::fs::create_dir_all(&writable).unwrap();
        let made = writable.join("made/deeper");
        // In a child, so this test's own process stays unrestricted.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            let code = match restrict_files(&[], std::slice::from_ref(&writable)) {
                Err(_) => 10,
                Ok(()) => {
                    let walk = || -> std::io::Result<()> {
                        let mut dir = link_files::nofollow::Dir::root_to_pass()?;
                        let parts: Vec<_> = made.components().skip(1).collect();
                        let (last, above) = parts.split_last().unwrap();
                        for part in above {
                            dir = dir.passing(part.as_os_str(), true, 0o700)?;
                        }
                        // Opened to read, write and lock, in the grant.
                        let made = dir.create_folder(last.as_os_str(), 0o700)?;
                        made.sync_all()
                    };
                    match walk() {
                        Ok(()) => 0,
                        Err(_) => 1,
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
            "10: no ruleset; 1: making a folder from / was refused above the grant"
        );
        assert!(made.is_dir());
    }

    /// The second file wall holds on its own: with the filesystem wide
    /// open (no bubblewrap, every real path reachable), Landlock still lets
    /// a process read only its read grant and write only its write grant,
    /// and refuses every other path. This is what makes it an independent
    /// wall rather than a mirror of the mounts.
    #[test]
    fn the_file_wall_denies_a_path_it_was_not_granted() {
        if let Err(error) = probe() {
            link_test_kit::gate::lacks("Landlock", error, Some(link_test_kit::gate::NATIVE));
            return;
        }
        let root = link_test_kit::TempDir::new("file-wall");
        let root = root.canonicalize().unwrap();
        let readable = root.join("readable");
        let writable = root.join("writable");
        let secret = root.join("secret");
        for dir in [&readable, &writable, &secret] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(readable.join("f"), "r").unwrap();
        std::fs::write(secret.join("f"), "s").unwrap();
        // In a child, so this test's own process stays unrestricted.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            let code = match restrict_files(
                std::slice::from_ref(&readable),
                std::slice::from_ref(&writable),
            ) {
                Err(_) => 10,
                Ok(()) => {
                    let read = |p: std::path::PathBuf| std::fs::read(p).is_ok();
                    let wrote = |p: std::path::PathBuf| std::fs::write(p, b"x").is_ok();
                    // The mounts are wide: only Landlock stands between the
                    // process and the whole real filesystem.
                    match (
                        read(readable.join("f")),  // granted read: allowed
                        read(secret.join("f")),    // never granted: refused
                        wrote(writable.join("g")), // granted write: allowed
                        wrote(secret.join("g")),   // never granted: refused
                    ) {
                        (true, false, true, false) => 0,
                        _ => 1,
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
            "10: no ruleset; 1: the wall let a non-granted path through, or denied a granted one"
        );
    }

    /// Where the kernel has ABI 6, the file wall also scopes the domain: a
    /// process under it can't connect to an abstract unix socket bound
    /// outside the sandbox (X11's gap in host mode) or signal a process
    /// outside it. The PID namespace and the mounts already close these in
    /// a real sandbox; this proves the kernel closes them too.
    #[test]
    fn the_scope_closes_abstract_sockets_and_signals_outside() {
        let abi = match probe() {
            Ok(abi) => abi,
            Err(error) => {
                link_test_kit::gate::lacks("Landlock", error, Some(link_test_kit::gate::NATIVE));
                return;
            }
        };
        if abi.get() < 6 {
            link_test_kit::gate::lacks(
                "Landlock's scope",
                format_args!(
                    "the kernel's Landlock ABI is {}, and scope is 6's",
                    abi.get()
                ),
                None,
            );
            return;
        }
        // An abstract socket bound here, outside any sandbox domain.
        let name = format!("\0lnk-scope-{}", std::process::id());
        let mut addr = unsafe { std::mem::zeroed::<libc::sockaddr_un>() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (slot, byte) in addr.sun_path.iter_mut().zip(name.bytes()) {
            *slot = byte as libc::c_char;
        }
        let len = (std::mem::size_of::<libc::sa_family_t>() + name.len()) as libc::socklen_t;
        // SAFETY: a fresh AF_UNIX socket bound to the abstract address above.
        let listener = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        assert!(listener >= 0);
        let addr_ptr = &addr as *const _ as *const libc::sockaddr;
        assert_eq!(unsafe { libc::bind(listener, addr_ptr, len) }, 0);
        assert_eq!(unsafe { libc::listen(listener, 1) }, 0);

        let parent = std::process::id();
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            let code = match restrict_files(&[], &[]) {
                Err(_) => 10,
                Ok(()) => {
                    // SAFETY: a new socket connected to the abstract address.
                    let sock = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
                    let reached = unsafe { libc::connect(sock, addr_ptr, len) } == 0;
                    unsafe { libc::close(sock) };
                    // Signalling the parent, which is outside this domain.
                    let signalled = unsafe { libc::kill(parent as i32, 0) } == 0;
                    match (reached, signalled) {
                        (false, false) => 0, // both refused: the scope holds
                        (true, _) => 1,      // reached an outside abstract socket
                        (_, true) => 2,      // signalled outside the sandbox
                    }
                }
            };
            unsafe { libc::_exit(code) };
        }
        let mut status = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        unsafe { libc::close(listener) };
        assert!(libc::WIFEXITED(status));
        assert_eq!(
            libc::WEXITSTATUS(status),
            0,
            "10: no ruleset; 1: reached an outside abstract socket; 2: signalled outside"
        );
    }
}

//! Whether a test can run here, decided once per capability.
//!
//! Each gate answers whether this machine has what the test needs. Where
//! it doesn't, the gate says so on the test's output (the test, what it
//! lacked and why) and the test returns; under the variable that requires
//! the capability, such as `LNK_REQUIRE_SANDBOX` where bubblewrap is
//! installed for the tests, the gate fails the test instead. A test never
//! skips without saying why.

use std::path::{Path, PathBuf};

/// The variable that requires the sandbox's capabilities: bubblewrap,
/// sshd, and test folders out of the temp folders (a checkout outside
/// them, and a short `LNK_TEST_STATE_DIR` for the fixtures it reads).
pub const SANDBOX: &str = "LNK_REQUIRE_SANDBOX";
/// The variable that requires the kernel's own confinement: Landlock.
pub const NATIVE: &str = "LNK_REQUIRE_NATIVE";
/// The variable that requires the host programs tests run, such as
/// `python3` or `git`.
pub const PROGRAMS: &str = "LNK_REQUIRE_PROGRAMS";
/// The variable that requires sources a step outside Cargo generates,
/// such as a JavaScript workspace's generated modules, which a check
/// reads as they're written.
pub const GENERATED: &str = "LNK_REQUIRE_GENERATED";

/// Whether a fixture's file and ancestors meet production ownership
/// checks. Container roots or writable temp ancestors cannot stand in
/// for private settings, even when their final file has mode 0600.
pub fn trusted_file(path: &Path, owners: &[u32]) -> bool {
    match link_files::fs::read_trusted(
        path,
        link_files::fs::Trust {
            owners,
            deny_mode: 0o022,
            ancestors: true,
            no_link: true,
        },
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        "a test's trusted fixture",
    ) {
        Ok(_) => true,
        Err(error) => lacks("trusted fixture ancestors", error, Some(SANDBOX)),
    }
}

/// Probe the production settings reader on an owned valid TOML fixture,
/// or its not-yet-created path, before testing settings behavior.
pub fn settings(path: &Path) -> bool {
    match link_files::settings::read::<toml::Table>(path) {
        Ok(_) => true,
        Err(error) => lacks("trusted settings ancestors", error, Some(SANDBOX)),
    }
}

/// What every gate does when the capability is missing: fails the test
/// when `required_by` is set, else says the test skips and returns false.
/// For a suite's own gate, such as a capability only its code can probe.
pub fn lacks(lacked: &str, why: impl std::fmt::Display, required_by: Option<&str>) -> bool {
    let thread = std::thread::current();
    let test = thread.name().unwrap_or("a test");
    if let Some(variable) = required_by
        && std::env::var_os(variable).is_some_and(|value| !value.is_empty())
    {
        panic!("{test}: {variable} is set, but it lacks {lacked}: {why}");
    }
    // Stays a print macro: the libtest harness captures it per test and
    // `--show-output` lists it, which is how the skip is reported. Written
    // past that capture (link-format's aside), a skip would lose its test.
    eprintln!("{test}: skipping, as it lacks {lacked}: {why}");
    false
}

/// Whether bubblewrap runs here, with a network of its own, as the
/// sandbox runs it. Required by [`SANDBOX`].
pub fn bubblewrap() -> bool {
    if !cfg!(target_os = "linux") {
        return lacks("bubblewrap", "it runs only on Linux", None);
    }
    let ran = crate::env::command("bwrap", &std::env::temp_dir())
        .args(["--unshare-net", "--ro-bind", "/", "/", "true"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();
    match ran {
        Ok(ran) if ran.status.success() => true,
        Ok(ran) => lacks(
            "bubblewrap",
            format_args!(
                "bwrap --unshare-net failed: {}",
                String::from_utf8_lossy(&ran.stderr).trim()
            ),
            Some(SANDBOX),
        ),
        Err(err) => lacks("bubblewrap", format_args!("bwrap: {err}"), Some(SANDBOX)),
    }
}

/// Whether the kernel enforces Landlock, as the sandbox's own confinement
/// needs. Required by [`NATIVE`].
pub fn landlock() -> bool {
    #[cfg(target_os = "linux")]
    {
        // The ABI's version, asked of the kernel with no ruleset.
        // SAFETY: a plain syscall, given no pointer.
        let version = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<libc::c_void>(),
                0usize,
                1u32,
            )
        };
        if version >= 1 {
            return true;
        }
        let err = std::io::Error::last_os_error();
        lacks(
            "Landlock",
            format_args!("the kernel refuses it: {err}"),
            Some(NATIVE),
        )
    }
    #[cfg(not(target_os = "linux"))]
    lacks("Landlock", "it is Linux's", None)
}

/// Whether a wrapped sandbox can run: bubblewrap supplies its namespaces,
/// and Landlock confines the program it wraps. Tests of plain sandbox runs
/// that do not wrap an agent can use [`bubblewrap`] alone.
pub fn sandbox_with_landlock() -> bool {
    bubblewrap() && landlock()
}

/// The sshd program, where one is installed. Required by [`SANDBOX`].
pub fn sshd() -> Option<PathBuf> {
    let found = ["/usr/sbin/sshd", "/usr/bin/sshd"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file());
    if found.is_none() {
        lacks(
            "sshd",
            "neither /usr/sbin/sshd nor /usr/bin/sshd is installed",
            Some(SANDBOX),
        );
    }
    found
}

/// Whether the sandbox runs here as `lnk_sandbox`, the workspace's own
/// program, runs it with `HOME` at `home`: `run --commands.toolchains shell -- /bin/true`
/// exits successfully. A program not built fails the test, as that is the
/// run's error, not the machine's. Required by [`SANDBOX`].
pub fn sandbox(lnk_sandbox: &Path, home: &Path) -> bool {
    crate::programs::require(lnk_sandbox, crate::programs::BUILD);
    let ran = crate::env::command(lnk_sandbox, home)
        .args(["run", "--commands.toolchains", "shell", "--", "/bin/true"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();
    let shown = lnk_sandbox.display();
    match ran {
        Ok(ran) if ran.status.success() => true,
        Ok(ran) => lacks(
            "a sandbox that runs",
            format_args!(
                "{shown} run --commands.toolchains shell -- /bin/true {}: {}",
                ran.status,
                String::from_utf8_lossy(&ran.stderr).trim()
            ),
            Some(SANDBOX),
        ),
        Err(err) => lacks(
            "a sandbox that runs",
            format_args!("{shown}: {err}"),
            Some(SANDBOX),
        ),
    }
}

/// Whether the test runs as root, for a test of what only root may do.
/// Nothing requires it: CI runs as an ordinary user, and root-only code is
/// run as root before it merges.
pub fn root() -> bool {
    crate::users::root() || lacks("root", "it runs as an ordinary user", None)
}

/// Whether the host program `program` runs here: `program args` exits
/// successfully, such as `python3 --version`. Required by [`PROGRAMS`].
pub fn program(program: &str, args: &[&str]) -> bool {
    let ran = crate::env::command(program, &std::env::temp_dir())
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match ran {
        Ok(status) if status.success() => true,
        Ok(status) => lacks(
            program,
            format_args!("{program} {} {status}", args.join(" ")),
            Some(PROGRAMS),
        ),
        Err(err) => lacks(program, format_args!("{program}: {err}"), Some(PROGRAMS)),
    }
}

/// Whether the host program `program` is installed here: it starts,
/// whatever it answers, for a program that has no argument it answers
/// successfully, such as `ssh-keygen`. Required by [`PROGRAMS`].
pub fn installed(program: &str) -> bool {
    let ran = crate::env::command(program, &std::env::temp_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match ran {
        Ok(_) => true,
        Err(err) => lacks(program, format_args!("{program}: {err}"), Some(PROGRAMS)),
    }
}

/// Whether `dir`, a test's folder kept out of the temp folders (its
/// `CARGO_TARGET_TMPDIR`), is out of them: the sandbox refuses a spec
/// that reads one, as anyone can write there. A checkout under `/tmp` has
/// its target there too. Required by [`SANDBOX`].
pub fn out_of_tmp(dir: &Path) -> bool {
    let resolved = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let dir = resolved(dir);
    let temp = [resolved(Path::new("/tmp")), resolved(&std::env::temp_dir())];
    match temp.iter().find(|t| dir.starts_with(t)) {
        Some(t) => lacks(
            "a test folder out of the temp folders",
            format_args!(
                "{} is in {}, which the sandbox refuses a spec to read; set a short \
                 LNK_TEST_STATE_DIR outside it, or run from a checkout outside it",
                dir.display(),
                t.display()
            ),
            Some(SANDBOX),
        ),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lacking_test_skips_unless_required() {
        assert!(!lacks("nothing", "a test of the gate", None));
        assert!(!lacks(
            "nothing",
            "a test of the gate",
            Some("LNK_TEST_KIT_UNSET")
        ));
        let required = std::panic::catch_unwind(|| {
            lacks("nothing", "a test of the gate", Some("PATH"));
        });
        let said = *required.unwrap_err().downcast::<String>().unwrap();
        assert!(said.contains("PATH is set, but it lacks nothing"), "{said}");
    }

    #[test]
    fn a_program_runs_or_is_lacked() {
        assert!(program("sh", &["-c", "true"]));
        if std::env::var_os(PROGRAMS).is_none() {
            assert!(!program("lnk-test-kit-no-such-program", &[]));
            assert!(!program("sh", &["-c", "exit 3"]));
        }
    }

    #[test]
    fn the_temp_folders_are_refused() {
        if std::env::var_os(SANDBOX).is_none() {
            // A real folder: on a Mac only one that exists resolves to
            // `/private/tmp`, as the temp folders do.
            let checkout = crate::TempDir::under(Path::new("/tmp"), "checkout");
            assert!(!out_of_tmp(&checkout));
        }
        assert!(out_of_tmp(Path::new("/usr")));
    }
}

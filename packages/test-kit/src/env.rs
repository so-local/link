//! The environment a test gives a program it runs.
//!
//! A child inherits nothing of the test's environment but what it needs
//! to run here: a test then passes or fails alike whoever runs it, from
//! whatever shell. Its own `HOME` and `TMPDIR`, so nothing of the
//! machine's user's settings or files reaches it; the machine's `PATH`,
//! where the host programs a [gate](crate::gate) found are; the proxy
//! and CA variables, through which this machine reaches the network; and
//! the test's own `LNK_*`, such as `LNK_REQUIRE_SANDBOX` or
//! `LNK_TEST_TIME_SCALE`. Its runtime directory is its home's `.run`
//! ([`runtime`]), beside its `.tmp`, so a grant of a folder in its home
//! never holds the runtime directory's sockets. Never the user's `USER`,
//! `LOGNAME` or `XDG_*`: a program that needs to know its user asks the
//! system. On a Mac, a [stand-in](crate::keychain) for the Keychain,
//! never the user's own.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The machine's variables a child is given as they are, besides `PATH`
/// and the test's `LNK_*`: how this machine reaches the network.
pub const PASSED: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
    "NODE_EXTRA_CA_CERTS",
];

/// `program`, to run with the kit's environment ([`clean`]), `home` its
/// `HOME`.
pub fn command(program: impl AsRef<OsStr>, home: &Path) -> Command {
    let program = program.as_ref();
    let mut command = crate::recording::command(program)
        .unwrap_or_else(|| link_secretive::process::command(program));
    clean(&mut command, home);
    command
}

/// The runtime directory (`XDG_RUNTIME_DIR`) [`clean`] gives a child
/// whose home is `home`: its `.run`.
pub fn runtime(home: &Path) -> PathBuf {
    home.join(".run")
}

/// [`runtime`], made private to this user if it isn't there: for a
/// child whose environment a test sets whole without [`clean`], so that
/// its sockets are its own and never in the machine-wide runtime folder.
pub fn made_runtime(home: &Path) -> PathBuf {
    let run = runtime(home);
    std::fs::create_dir_all(&run)
        .and_then(|()| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700))
        })
        .unwrap_or_else(|err| {
            panic!(
                "making a child's runtime directory, {}: {err}",
                run.display()
            )
        });
    run
}

/// Gives `command` the kit's environment and nothing else: `HOME` is
/// `home`, `TMPDIR` its `.tmp` and `XDG_RUNTIME_DIR` its `.run` (each
/// made here, the runtime directory private to its user), then `PATH`, [`PASSED`] and
/// the test's `LNK_*` as the test has them, but for the one test a system
/// program's process runs ([`ONE`](crate::program::ONE)) and any that holds a
/// secret (`link_secretive::process::variables`). A variable the test sets on
/// `command` afterwards is added to these.
pub fn clean<'a>(command: &'a mut Command, home: &Path) -> &'a mut Command {
    let tmp = home.join(".tmp");
    std::fs::create_dir_all(&tmp)
        .unwrap_or_else(|err| panic!("making a child's TMPDIR, {}: {err}", tmp.display()));
    let run = made_runtime(home);
    command
        .env_clear()
        .env("HOME", home)
        .env("TMPDIR", &tmp)
        .env("XDG_RUNTIME_DIR", &run);
    command.envs(link_secretive::process::variables(|name| {
        name == "PATH"
            || (name.starts_with("LNK_") && name != crate::program::ONE)
            || PASSED.contains(&name)
    }));
    command.envs(crate::keychain::isolated(home))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_child_has_its_own_home_and_nothing_of_the_user() {
        let home = crate::TempDir::new("env");
        let output = command("env", &home).env("LNK_SET", "1").output().unwrap();
        let seen = String::from_utf8_lossy(&output.stdout);
        let names: Vec<&str> = seen
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(n, _)| n)
            .collect();
        assert!(
            seen.contains(&format!("HOME={}\n", home.display())),
            "{seen}"
        );
        assert!(
            seen.contains(&format!("TMPDIR={}\n", home.join(".tmp").display())),
            "{seen}"
        );
        assert!(names.contains(&"LNK_SET"), "{seen}");
        assert!(
            seen.contains(&format!("XDG_RUNTIME_DIR={}\n", runtime(&home).display())),
            "{seen}"
        );
        for name in names {
            assert!(
                ["HOME", "TMPDIR", "PATH", "XDG_RUNTIME_DIR"].contains(&name)
                    || name.starts_with("LNK_")
                    || PASSED.contains(&name),
                "{name} reached the child: {seen}"
            );
        }
    }
}

//! The programs a test runs: the workspace's built ones, cargo, and
//! programs a test writes itself.
//!
//! Tests run side by side in one process. While one test holds a program
//! file open for writing, another test's child, forked in that instant,
//! inherits the open file until it runs its own program; running the first
//! program then fails with "Text file busy" (`ETXTBSY`, Linux). Written by a
//! child process of its own (`install`), a program is never open for
//! writing in the test's process, so no child can inherit it.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// `lnk` writes and copies programs the same way, so the kit's are its own.
pub use link_files::{copy_program, write_program};

/// Prepare an executable with explicit Unix permissions for a permission
/// boundary test. Changing its mode never opens its bytes for writing.
#[cfg(unix)]
pub fn write_program_with_mode(
    path: &Path,
    contents: impl AsRef<[u8]>,
    mode: u32,
) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    write_program(path, contents)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// The shell line a stand-in `lnk` answers the release's history with
/// (`lnk --link-history`): an empty one, in the protocol version its asker
/// states (`LNK_HISTORY_PROTOCOL`), as a release that retired nothing does.
const NO_HISTORY: &str = "[ \"$1\" = --link-history ] && \
    { printf '{\"protocol\":%s}\\n' \"$LNK_HISTORY_PROTOCOL\"; exit 0; }\n";

/// A stand-in `lnk` at `path` running `script`, a `/bin/sh` script, which
/// first answers the release's history as the core does, empty: a program
/// reading a spec whose names follow history asks its core, and refuses
/// the spec when the core doesn't answer.
#[cfg(unix)]
pub fn write_core(path: &Path, script: impl AsRef<str>) -> std::io::Result<()> {
    let script = script.as_ref();
    let Some(body) = script.strip_prefix("#!/bin/sh\n") else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a stand-in core is a #!/bin/sh script",
        ));
    };
    write_program(path, format!("#!/bin/sh\n{NO_HISTORY}{body}"))
}

/// A built program at `to` as well, for a test that needs it beside its
/// stand-ins and doesn't change it: a hard link, else a copy across
/// filesystems. A Mac checks a new file's whole code on its first run, so
/// many tests each running their own copy of a large program wait in turn.
pub fn link_program(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::hard_link(from, to).or_else(|_| copy_program(from, to))
}

/// The cargo running these tests, for a test that builds or reads the
/// workspace, without the variables cargo sets for a test (its package's
/// `CARGO_MANIFEST_DIR`, `CARGO_PKG_NAME`, ...). A build script that reads
/// one reruns when it changes, so with them each test's build would
/// recompile `ring` and everything above it, and the next `cargo test`
/// would recompile it back.
pub fn cargo() -> Command {
    link_component::cargo::for_test(link_secretive::process::command)
}

/// The command that builds a workspace's programs, which its tests run
/// from `target/<profile>`: Cargo builds a test's own package's programs
/// for it (`CARGO_BIN_EXE_*`), and no other's.
pub const BUILD: &str = "cargo build --workspace --bins";

/// What a test says when `program` isn't built: one sentence every such
/// test shares, naming `by`, the command that builds it, so a run with
/// none built shows one cause, not a different one per test.
pub fn not_built(program: &Path, by: &str) -> String {
    format!(
        "{} isn't built: the programs a test runs are built first ({by})",
        program.display()
    )
}

/// `program`, failing the test when it isn't built (`by` builds it): a
/// missing program is the run's error, not the machine's, so it fails
/// rather than skips.
#[track_caller]
pub fn require<'a>(program: &'a Path, by: &str) -> &'a Path {
    assert!(program.is_file(), "{}", not_built(program, by));
    program
}

/// The build directory holding this test, `target/<profile>`, where
/// `cargo test` put every program of the packages it selected.
pub fn programs() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.to_path_buf()))
        .expect("a test program in target/<profile>/deps")
}

/// The build directory of the workspace holding `root`, built there with
/// `cargo build --bins`: the nearest `target/<profile>` above it.
pub fn programs_in(root: &Path) -> PathBuf {
    let own = programs();
    let profile = own.file_name().expect("target/<profile>");
    root.ancestors()
        .map(|dir| dir.join("target").join(profile))
        .find(|dir| dir.is_dir())
        .unwrap_or(own)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::TempDir;
    use link_component::cargo::set_for_tests;

    #[test]
    fn cargo_runs_without_what_it_set_for_this_test() {
        assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
        // Its environment set whole (`process::command`), without them.
        let given: Vec<_> = cargo()
            .get_envs()
            .filter(|(_, value)| value.is_some())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        assert!(given.contains(&"PATH".into()), "{given:?}");
        assert!(!given.contains(&"CARGO_MANIFEST_DIR".into()), "{given:?}");
        assert!(!given.contains(&"CARGO_PKG_NAME".into()), "{given:?}");
        // Cargo's own settings stay.
        for kept in ["CARGO_HOME", "CARGO_TARGET_DIR", "CARGO_INCREMENTAL"] {
            assert!(!set_for_tests(kept), "{kept}");
        }
    }

    #[test]
    fn programs_are_written_ready_to_run() {
        let dir = TempDir::new("testing");
        let program = dir.join("hello");
        write_program(&program, "#!/bin/sh\necho hi\n").unwrap();
        // Written again over itself, as tests do.
        write_program(&program, "#!/bin/sh\necho hello\n").unwrap();
        let output = link_secretive::process::command(&program).output().unwrap();
        assert_eq!(output.stdout, b"hello\n");
        copy_program(&program, &dir.join("copy")).unwrap();
        assert_eq!(
            link_secretive::process::command(dir.join("copy"))
                .output()
                .unwrap()
                .stdout,
            b"hello\n"
        );
        link_program(&program, &dir.join("link")).unwrap();
        assert_eq!(
            link_secretive::process::command(dir.join("link"))
                .output()
                .unwrap()
                .stdout,
            b"hello\n"
        );
        // Nothing staged is left.
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(left.len(), 3);
    }

    #[test]
    fn a_stand_in_core_answers_an_empty_history_then_runs_its_script() {
        let dir = TempDir::new("testing");
        let core = dir.join("lnk");
        write_core(&core, "#!/bin/sh\necho \"asked $*\"\nexit 3\n").unwrap();
        let history = link_secretive::process::command(&core)
            .arg("--link-history")
            .env("LNK_HISTORY_PROTOCOL", "7")
            .output()
            .unwrap();
        assert!(history.status.success(), "{history:?}");
        assert_eq!(history.stdout, b"{\"protocol\":7}\n");
        let other = link_secretive::process::command(&core)
            .args(["box", "list"])
            .output()
            .unwrap();
        assert_eq!(other.status.code(), Some(3));
        assert_eq!(other.stdout, b"asked box list\n");
        assert!(write_core(&dir.join("other"), "echo no shebang\n").is_err());
    }
}

/// Released Link binaries from Cargo metadata, indexed by component identity.
/// Fixtures under `tests/` are not installed programs.
pub fn declared(
    roots: &[std::path::PathBuf],
) -> anyhow::Result<std::collections::BTreeMap<String, std::path::PathBuf>> {
    use anyhow::{Context, ensure};
    use std::path::Path;
    let mut found = std::collections::BTreeMap::new();
    for root in roots {
        let root = root.canonicalize()?;
        let output = crate::cargo()
            .current_dir(&root)
            .args([
                "metadata",
                "--no-deps",
                "--offline",
                "--format-version",
                "1",
            ])
            .output()?;
        ensure!(
            output.status.success(),
            "cargo metadata: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        for package in metadata["packages"].as_array().context("Cargo packages")? {
            let folder = Path::new(package["manifest_path"].as_str().context("manifest path")?)
                .parent()
                .context("manifest folder")?
                .to_path_buf();
            if folder
                .strip_prefix(&root)
                .unwrap_or(&folder)
                .components()
                .any(|c| c.as_os_str() == "tests")
            {
                continue;
            }
            for target in package["targets"].as_array().context("Cargo targets")? {
                if !target["kind"]
                    .as_array()
                    .context("target kind")?
                    .iter()
                    .any(|k| k == "bin")
                {
                    continue;
                }
                let name = target["name"].as_str().context("target name")?;
                let identity = if name == "lnk" {
                    "core"
                } else if let Some(name) = name.strip_prefix("lnk-") {
                    name
                } else {
                    continue;
                };
                ensure!(
                    found.insert(identity.into(), folder.clone()).is_none(),
                    "duplicate program {name}"
                );
            }
        }
    }
    Ok(found)
}

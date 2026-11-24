//! Optional command observation for tests run by a recording controller.
//! The controller selects executable names and supplies a recorder. Product
//! programs stay in place, including copies a test makes for its fixtures.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// A controller-owned TOML manifest, inherited by the test's children.
pub const ENV: &str = "LNK_TEST_RECORDER";

/// Program basenames to observe, exactly or by a declared prefix.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Programs {
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub prefixes: Vec<String>,
}

/// The recorder receives `args`, then the original basename, executable
/// and its arguments. It must preserve the command's environment, streams,
/// exit and signal behavior. An absent manifest leaves commands unchanged.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recorder {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub selected: Programs,
}

/// An executable path handed to code that starts its own commands. When
/// selected by the controller, a shim in `owned_folder` execs the recorder
/// with the same protocol as [`crate::env::command`]. Otherwise the original
/// path is returned and no files are written. The caller owns the folder
/// for the lifetime of every command using the returned path.
#[cfg(unix)]
pub fn program(original: &Path, owned_folder: &Path) -> anyhow::Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    let Some((recorder, name)) = selected(original.as_os_str())? else {
        return Ok(original.to_path_buf());
    };
    anyhow::ensure!(
        owned_folder.is_absolute(),
        "the shim folder must be absolute"
    );
    std::fs::create_dir_all(owned_folder)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = owned_folder.join(format!("recorded-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
    let mut script = b"#!/bin/sh\nexec".to_vec();
    // Single quotes preserve every path/argument byte, including shell
    // substitutions. Only a literal quote needs to leave and reenter them.
    let mut quoted = |value: &OsStr| -> anyhow::Result<()> {
        anyhow::ensure!(
            !value.as_bytes().contains(&0),
            "a recorder command cannot contain a NUL byte"
        );
        script.extend_from_slice(b" '");
        for byte in value.as_bytes() {
            match byte {
                b'\'' => script.extend_from_slice(b"'\"'\"'"),
                _ => script.push(*byte),
            }
        }
        script.push(b'\'');
        Ok(())
    };
    quoted(recorder.program.as_os_str())?;
    for arg in &recorder.args {
        quoted(OsStr::new(arg))?;
    }
    quoted(OsStr::new(&name))?;
    quoted(original.as_os_str())?;
    script.extend_from_slice(b" \"$@\"\n");
    crate::write_program(&path, script)?;
    Ok(path)
}

pub(crate) fn command(program: &OsStr) -> Option<Command> {
    let (recorder, name) = selected(program).unwrap_or_else(|error| panic!("{error:#}"))?;
    let mut command = link_secretive::process::command(recorder.program);
    command.args(recorder.args).arg(name).arg(program);
    Some(command)
}

fn selected(program: &OsStr) -> anyhow::Result<Option<(Recorder, String)>> {
    let Some(path) = std::env::var_os(ENV) else {
        return Ok(None);
    };
    let read = || -> anyhow::Result<Recorder> {
        let recorder: Recorder = toml::from_str(&std::fs::read_to_string(&path)?)?;
        anyhow::ensure!(
            recorder.program.is_absolute(),
            "the recorder must be absolute"
        );
        anyhow::ensure!(
            recorder.selected.prefixes.iter().all(|p| !p.is_empty()),
            "a recording prefix cannot be empty"
        );
        Ok(recorder)
    };
    let recorder = read()
        .with_context(|| format!("reading command recorder {}", Path::new(&path).display()))?;
    let Some(name) = Path::new(program).file_name().and_then(OsStr::to_str) else {
        return Ok(None);
    };
    if !recorder.selected.names.iter().any(|n| n == name)
        && !recorder
            .selected
            .prefixes
            .iter()
            .any(|p| name.starts_with(p))
    {
        return Ok(None);
    }
    Ok(Some((recorder, name.to_string())))
}

#[cfg(test)]
mod tests;

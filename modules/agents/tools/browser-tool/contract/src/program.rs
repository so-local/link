//! The Chromium program an agent's spec names (`[tool.browser]
//! program`), checked before the browser is given it: an executable
//! program by its absolute path, never a launcher script, which would run
//! inside the browser's sandbox and be refused the programs it calls.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, ensure};

/// Checks `program`, named by `setting` (`[tool.browser] program`): an
/// absolute path to an executable file that isn't a script. Each refusal
/// names the setting and says what to name instead.
pub fn check(program: &Path, setting: &str) -> anyhow::Result<()> {
    let shown = program.display();
    ensure!(
        program.is_absolute(),
        "{setting} {shown} isn't an absolute path: name the browser's program from /"
    );
    ensure!(
        program.is_file(),
        "{setting} {shown} isn't a file here: name a Chromium program this machine has"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(program)
            .with_context(|| format!("{setting} {shown}"))?
            .permissions()
            .mode();
        ensure!(
            mode & 0o111 != 0,
            "{setting} {shown} isn't executable: name a program you can run"
        );
    }
    let mut start = [0u8; 2];
    let read = std::fs::File::open(program)
        .and_then(|mut file| file.read(&mut start))
        .with_context(|| format!("{setting} {shown} can't be read"))?;
    ensure!(
        start[..read] != *crate::constants::SCRIPT_MARK,
        "{setting} {shown} is a script (it starts with #!), which would run inside the \
         browser's sandbox and be refused the programs it calls: name the Chromium program it \
         starts instead"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;
    use std::path::Path;

    const SETTING: &str = "[tool.browser] program";

    /// `bytes` written to `name` in `dir`, with `mode`.
    fn file(dir: &Path, name: &str, bytes: &[u8], mode: u32) -> std::path::PathBuf {
        let path = dir.join(name);
        link_test_kit::write_program_with_mode(&path, bytes, mode).unwrap();
        path
    }

    /// `program`'s refusal, which names the setting and says `why`.
    fn refused(program: &Path, why: &str) {
        let err = format!("{:#}", check(program, SETTING).expect_err(why));
        assert!(err.starts_with(SETTING) && err.contains(why), "{err}");
    }

    /// An executable program by its absolute path is taken; a script, a
    /// relative path, a missing file, a folder and a file that isn't
    /// executable are refused, each by the setting's name.
    #[test]
    fn only_an_executable_program_by_its_absolute_path_is_taken() {
        let dir = link_test_kit::TempDir::new("browser-program");
        let program = file(&dir, "chrome", b"\x7fELF\x02\x01\x01", 0o755);
        check(&program, SETTING).unwrap();
        refused(
            &file(&dir, "launcher", b"#!/bin/sh\nexec chrome\n", 0o755),
            "is a script",
        );
        refused(Path::new("chrome"), "isn't an absolute path");
        refused(&dir.join("missing"), "isn't a file here");
        refused(&dir, "isn't a file here");
        refused(
            &file(&dir, "plain", b"\x7fELF\x02\x01\x01", 0o644),
            "isn't executable",
        );
        // Shorter than the mark: a program still, if an odd one.
        check(&file(&dir, "short", b"#", 0o755), SETTING).unwrap();
    }
}

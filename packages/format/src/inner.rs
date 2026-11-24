//! A trace across module calls. A command run for another's own (an
//! inner run, [`INNER`](crate::INNER) set) shows its report without a
//! footer and writes no record: the outer command's report is the one a
//! person reads. So an inner run that fails leaves its trace in the
//! folder [`INNER`](crate::INNER) names, and the outer run's report, when
//! it fails too, takes the latest one's steps under its own, each marked
//! with the program it came from. No caller passes anything along.

use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::Trace;

/// The folder this process's inner runs leave their traces in: under
/// its machine's records, by its process, so it's private and its own.
pub fn folder(records: &Path) -> PathBuf {
    records.join("inner").join(std::process::id().to_string())
}

/// What an inner run leaves: the command it was and its report's trace.
#[derive(Serialize, Deserialize)]
struct Left {
    program: String,
    trace: Trace,
}

/// Leaves `trace` for the run that ran this one, in the folder
/// [`INNER`](crate::INNER) names: nothing when it names none (`1`, from an
/// older `lnk`), or the trace can't be written, which never fails the
/// command.
pub(crate) fn leave(program: &str, trace: &Trace) {
    let Some(dir) = std::env::var_os(crate::INNER)
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
    else {
        return;
    };
    let _ = write(&dir, program, trace);
}

pub(crate) fn write(dir: &Path, program: &str, trace: &Trace) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    // Its time to the nanosecond, zero-padded, so names sort oldest first.
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!("{nanos:024}-{}.json", std::process::id());
    let left = Left {
        program: program.to_string(),
        trace: trace.clone(),
    };
    let text = serde_json::to_string(&left).map_err(std::io::Error::other)?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.join(name))?
        .write_all(text.as_bytes())
}

/// The trace the latest inner run that failed left in `dir`, its steps as
/// from its program, and `dir` emptied: none when none failed.
pub(crate) fn take(dir: &Path) -> Option<Trace> {
    let mut left: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    left.sort();
    let latest = left
        .last()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str::<Left>(&text).ok());
    clear(dir);
    latest.map(|l| l.trace.from_program(&l.program))
}

/// `dir` removed: what an earlier process of the same number left, or
/// what a run that ended well never took.
pub(crate) fn clear(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Step;

    fn step(what: &str) -> Step {
        Step {
            what: what.into(),
            at: Some(format!("modules/x/src/{what}.rs:1")),
            program: None,
        }
    }

    #[test]
    fn the_latest_inner_failure_is_taken_once_as_from_its_program() {
        let temp = link_test_kit::TempDir::new("inner");
        let dir = temp.join("trace");
        clear(&dir);
        let first = Trace {
            steps: vec![step("earlier")],
        };
        let latest = Trace {
            steps: vec![step("listing"), step("reading")],
        };
        write(&dir, "lnk box list", &first).unwrap();
        write(&dir, "lnk box list", &latest).unwrap();
        let taken = take(&dir).unwrap();
        assert_eq!(
            taken
                .steps
                .iter()
                .map(|s| s.what.as_str())
                .collect::<Vec<_>>(),
            ["listing", "reading"]
        );
        assert!(
            taken
                .steps
                .iter()
                .all(|s| s.program.as_deref() == Some("lnk box list"))
        );
        assert!(!dir.exists());
        assert!(take(&dir).is_none());
    }
}

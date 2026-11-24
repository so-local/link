//! Read-only wire state, shared by the sandbox command and its callers.
//! This is current control state, never a telemetry cache.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The sandbox's private settings. Reading never creates them.
pub fn wire_dir() -> anyhow::Result<PathBuf> {
    link_files::layout::Hidden::Sandboxes.try_path()
}

/// The traffic snapshots the proxy publishes in `dir`.
pub fn traffic_dir(dir: &Path) -> PathBuf {
    dir.join("traffic")
}

pub fn stop_file(dir: &Path) -> PathBuf {
    dir.join("stopped")
}

pub fn pause_file(dir: &Path, sandbox: &str) -> PathBuf {
    dir.join("paused").join(sandbox)
}

/// The sandboxes' wires that are closed, as `lnk sandbox list --json`
/// says of each sandbox. Observation confers no authorization or control.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Wires {
    pub stopped: bool,
    pub paused: Vec<Paused>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Paused {
    pub sandbox: String,
    pub why: String,
}

impl Wires {
    /// Read the owner's flags afresh on every call, without creating a
    /// directory or starting a program. Missing state is open, unreadable
    /// state is an error. The proxy still enforces its own checks.
    pub fn read(dir: &Path) -> std::io::Result<Self> {
        let stopped = stop_file(dir).try_exists()?;
        let entries = match std::fs::read_dir(dir.join("paused")) {
            Ok(entries) => Some(entries),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let mut paused = Vec::new();
        for entry in entries.into_iter().flatten() {
            let entry = entry?;
            let Ok(sandbox) = entry.file_name().into_string() else {
                continue;
            };
            let why = match std::fs::read_to_string(entry.path()) {
                Ok(why) => why,
                // Resumed while the directory was being read.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            paused.push(Paused {
                sandbox,
                why: why.trim().to_string(),
            });
        }
        paused.sort_by(|a, b| a.sandbox.cmp(&b.sandbox));
        Ok(Self { stopped, paused })
    }

    pub fn closed(&self, name: &str) -> Option<String> {
        if self.stopped {
            return Some("every wire is stopped (lnk sandbox start opens them)".into());
        }
        self.paused
            .iter()
            .find(|p| p.sandbox == name)
            .map(|p| format!("paused, {} (lnk sandbox start {name})", p.why))
    }
}

/// Current wire state here; unavailable when the sandbox is absent or
/// its private state cannot be read. Starts no child process.
pub fn wires() -> Option<Wires> {
    link_core::find_module("sandbox")?;
    Wires::read(&wire_dir().ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_read_observes_pause_resume_and_stop_without_a_cache() {
        let root = link_test_kit::TempDir::new("wire-state");
        let dir = root.join("sandbox");
        assert_eq!(Wires::read(&dir).unwrap(), Wires::default());
        assert!(!dir.exists(), "a read created private settings");
        let pause = pause_file(&dir, "main-link");
        link_files::write_private(&pause, b"by a test\n").unwrap();
        assert!(
            Wires::read(&dir)
                .unwrap()
                .closed("main-link")
                .unwrap()
                .contains("by a test")
        );
        assert_eq!(Wires::read(&dir).unwrap().closed("work-link"), None);
        std::fs::remove_file(&pause).unwrap();
        assert_eq!(Wires::read(&dir).unwrap().closed("main-link"), None);
        link_files::write_private(&stop_file(&dir), b"stopped").unwrap();
        assert!(
            Wires::read(&dir)
                .unwrap()
                .closed("work-link")
                .unwrap()
                .contains("stopped")
        );
        std::fs::remove_file(stop_file(&dir)).unwrap();
        assert_eq!(Wires::read(&dir).unwrap(), Wires::default());
        // A corrupt/unreadable entry cannot be reported as an open wire.
        std::fs::create_dir(&pause).unwrap();
        assert!(Wires::read(&dir).is_err());
    }
}

//! Published traffic snapshots. Providers keep their own files and
//! advertise their directory with an agent; readers never clean it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::asked::Traffic;

/// A provider's existing private `<pid>.json` snapshots. The provider
/// names itself so its command answer isn't counted a second time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub provider: String,
    pub directory: PathBuf,
}

/// Live snapshots and files belonging to dead or reused process IDs.
/// Only the publishing owner may remove `stale`; an observer only reads.
#[derive(Debug, Default)]
pub struct Read {
    pub traffic: Vec<Traffic>,
    pub stale: Vec<PathBuf>,
}

/// Read the same typed snapshots for a command or an in-process
/// consumer. No command, mutation, cache or extra authority is involved.
pub fn read(directory: &Path) -> Read {
    let mut found = Read::default();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let pid = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|_| path.extension().is_some_and(|e| e == "json"));
        let Some(pid) = pid else { continue };
        if !alive(pid) {
            found.stale.push(path);
            continue;
        }
        let traffic = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Traffic>(&b).ok())
            .filter(|t| t.pid == pid);
        let started = super::started(pid);
        if let Some(traffic) = &traffic
            && started.is_some()
            && traffic.started != started
        {
            found.stale.push(path);
            continue;
        }
        found.traffic.extend(traffic);
    }
    found.traffic.sort_by_key(|t| t.pid);
    found
}

fn alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only asks whether the process is there.
    pid > 0
        && (unsafe { libc::kill(pid, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_are_fresh_identity_checked_and_read_only() {
        let dir = link_test_kit::TempDir::new("traffic-snapshots");
        let pid = std::process::id();
        let file = dir.join(format!("{pid}.json"));
        let mut traffic = Traffic {
            pid,
            sandbox: "main-link".into(),
            up: 10,
            down: 20,
            started: super::super::started(pid),
        };
        let write = |traffic: &Traffic| {
            link_files::write_private(&file, &serde_json::to_vec(traffic).unwrap()).unwrap();
        };
        write(&traffic);
        assert_eq!(read(&dir).traffic, [traffic.clone()]);
        traffic.up = 3; // A reset is still the publisher's value.
        write(&traffic);
        assert_eq!(read(&dir).traffic, [traffic.clone()]);
        let dead = dir.join("999999999.json");
        std::fs::write(&dead, b"{}").unwrap();
        assert!(read(&dir).stale.contains(&dead));
        assert!(dead.exists(), "an observer removed an owner's file");
        if traffic.started.is_some() {
            traffic.started = None;
            write(&traffic);
            let read = read(&dir);
            assert!(read.traffic.is_empty());
            assert!(read.stale.contains(&file));
            assert!(file.exists());
        }
        traffic.pid += 1;
        write(&traffic);
        assert!(read(&dir).traffic.is_empty());
        std::fs::write(&file, b"partial {").unwrap();
        assert!(read(&dir).traffic.is_empty());
    }
}

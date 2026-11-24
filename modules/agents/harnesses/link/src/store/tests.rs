//! The store's tests, each beside what it tests.

mod copies;
mod folders;
mod log;
mod prompt;
mod schemas;
mod snapshots;
mod turns;

use super::*;
use crate::agent::Folder;
use crate::event;

fn world(name: &str) -> (PathBuf, Agents) {
    let root = std::env::temp_dir().join(format!("lnk-harness-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut agents = Agents::default();
    for agent in ["main", "other"] {
        agents.add(
            Agent::new(agent).unwrap(),
            root.join(agent).join("home"),
            Folder {
                home: root.join(agent).join("files"),
                agent: root.join(agent).join("own"),
            },
        );
    }
    (root, agents)
}

fn said(seq: u64, text: &str) -> Event {
    Event {
        seq,
        ..Event::user(text)
    }
}

fn fifo(path: &Path) {
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: a valid C string.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
}

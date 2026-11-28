//! The files git tracks in a workspace, which the checks that read every
//! file of a kind (`package.json`, the licenses) walk.

use std::path::Path;

/// The files git tracks under `root`, by their paths from it.
#[expect(clippy::disallowed_methods, reason = "git lists the tracked files")]
pub fn tracked(root: &Path) -> Vec<String> {
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .expect("git ls-files");
    assert!(listed.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|file| !file.is_empty())
        .map(str::to_string)
        .collect()
}

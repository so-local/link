//! Other programs' sockets in the home folder, covered where a sandbox
//! reads them: a read-only mount doesn't stop `connect()`, so a sandbox
//! reading `~` would otherwise reach Docker Desktop's socket, which runs
//! a container outside, or an SSH connection's shared one. The folders
//! are `home_sockets.toml`'s. A socket outside them, or made in a mixed
//! folder after the sandbox starts, is the gaps page's.

use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// `home_sockets.toml`, as read.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Listed {
    runtime: Vec<PathBuf>,
    mixed: Vec<PathBuf>,
    depth: usize,
    entries: usize,
}

/// `home_sockets.toml`, read once.
fn listed() -> &'static Listed {
    static READ: std::sync::OnceLock<Listed> = std::sync::OnceLock::new();
    READ.get_or_init(|| {
        toml::from_str(include_str!("home_sockets.toml")).expect("home_sockets.toml reads")
    })
}

/// bubblewrap's arguments covering the sockets in [`Listed`]'s folders
/// under `home` that the sandbox reads: the whole home (`read_home`), or a
/// grant in `read` holding the folder or inside it. Mounted after the
/// grants, so each covers what a grant shows.
pub fn args(home: &Path, read_home: bool, read: &[PathBuf]) -> Vec<String> {
    let listed = listed();
    let mut args = Vec::new();
    for (folder, runtime) in listed
        .runtime
        .iter()
        .map(|f| (f, true))
        .chain(listed.mixed.iter().map(|f| (f, false)))
    {
        for reached in reached(&home.join(folder), read_home, read) {
            if runtime {
                if real_folder(&reached) {
                    args.extend(["--tmpfs".into(), reached.display().to_string()]);
                }
            } else {
                for socket in sockets(&reached, listed.depth, listed.entries) {
                    args.extend([
                        "--ro-bind".into(),
                        "/dev/null".into(),
                        socket.display().to_string(),
                    ]);
                }
            }
        }
    }
    args
}

/// What of `folder` the sandbox reads: all of it, when the home or a
/// grant holds it, or each grant inside it.
fn reached(folder: &Path, read_home: bool, read: &[PathBuf]) -> Vec<PathBuf> {
    if read_home || read.iter().any(|grant| folder.starts_with(grant)) {
        return vec![folder.to_path_buf()];
    }
    read.iter()
        .filter(|grant| grant.starts_with(folder))
        .cloned()
        .collect()
}

/// Whether `path` is a folder itself, never through a link.
fn real_folder(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// The sockets in `folder`, itself one or in its folders `depth` deep,
/// following no link, reading at most `entries` entries.
fn sockets(folder: &Path, depth: usize, entries: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut read = 0;
    let mut queue = vec![(folder.to_path_buf(), 0)];
    while let Some((path, at)) = queue.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_socket() {
            found.push(path);
            continue;
        }
        if !meta.is_dir() || at >= depth {
            continue;
        }
        let Ok(children) = std::fs::read_dir(&path) else {
            continue;
        };
        for child in children.flatten() {
            if read >= entries {
                return found;
            }
            read += 1;
            queue.push((child.path(), at + 1));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The listed folders are the home folder's, plain and relative.
    #[test]
    fn the_list_reads_and_its_folders_are_the_homes() {
        let listed = listed();
        assert!(listed.depth > 0 && listed.entries > 0);
        for folder in listed.runtime.iter().chain(&listed.mixed) {
            assert!(
                folder
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
                "{}",
                folder.display()
            );
        }
    }

    /// Under a read of the home folder, a daemon's runtime folder is an
    /// empty one and each socket in a mixed folder an empty file; a file
    /// beside a socket, a link and a socket too deep are left; with no read
    /// of them, nothing is covered.
    #[test]
    fn sockets_a_sandbox_reads_are_covered() {
        let home = link_test_kit::TempDir::short("home-sockets");
        let home = home.canonicalize().unwrap();
        let run = home.join(".docker/run");
        std::fs::create_dir_all(&run).unwrap();
        let ssh = home.join(".ssh");
        std::fs::create_dir_all(ssh.join("sockets/deeper")).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "key").unwrap();
        let shared = ssh.join("cm");
        let nested = ssh.join("sockets/m");
        let deep = ssh.join("sockets/deeper/m");
        let _held: Vec<_> = [&shared, &nested, &deep]
            .iter()
            .map(|path| std::os::unix::net::UnixListener::bind(path).unwrap())
            .collect();
        std::os::unix::fs::symlink(&shared, ssh.join("linked")).unwrap();

        let covered = args(&home, true, &[]);
        let has = |flag: &str, path: &Path| {
            let path = path.display().to_string();
            match flag {
                "--tmpfs" => covered.windows(2).any(|w| w[0] == flag && w[1] == path),
                _ => covered
                    .windows(3)
                    .any(|w| w[0] == flag && w[1] == "/dev/null" && w[2] == path),
            }
        };
        assert!(has("--tmpfs", &run), "{covered:?}");
        assert!(
            has("--ro-bind", &shared) && has("--ro-bind", &nested),
            "{covered:?}"
        );
        for left in [ssh.join("id_ed25519"), ssh.join("linked"), deep] {
            assert!(
                !covered.iter().any(|arg| *arg == left.display().to_string()),
                "{} covered: {covered:?}",
                left.display()
            );
        }
        // A grant holding them, or one inside a runtime folder, is a read
        // of them; a grant beside them isn't.
        assert_eq!(args(&home, false, std::slice::from_ref(&ssh)).len(), 6);
        assert_eq!(args(&home, false, &[run.join("x")]), Vec::<String>::new());
        std::fs::create_dir(run.join("x")).unwrap();
        assert_eq!(
            args(&home, false, &[run.join("x")]),
            ["--tmpfs".to_string(), run.join("x").display().to_string()]
        );
        assert!(args(&home, false, &[home.join("code")]).is_empty());
    }
}

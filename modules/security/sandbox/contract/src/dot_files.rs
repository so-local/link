//! The dot files a person's tools run from the top of a folder, outside
//! any sandbox (`dot_files.toml`), and Link's own data folders, where one
//! that wasn't there is kept out rather than covered: the one list the
//! sandbox and the file tool share.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

/// `dot_files.toml`, as read.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DotFiles {
    /// The names at the top of a folder that run outside the sandbox.
    pub names: Vec<String>,
    /// What hook managers read from the top of a folder.
    pub hook_managers: Vec<String>,
    /// Link's own data folders, where none of [`DotFiles::names`] is
    /// made: refused and swept instead.
    pub swept: Vec<link_files::layout::Home>,
}

/// `dot_files.toml`, read once.
pub fn get() -> &'static DotFiles {
    static READ: OnceLock<DotFiles> = OnceLock::new();
    READ.get_or_init(|| {
        toml::from_str(include_str!("dot_files.toml")).expect("dot_files.toml reads")
    })
}

/// Whether `name` (one part of a path) is one of the names, whatever its
/// case, as a Mac's disk finds it.
pub fn named(name: &str) -> bool {
    get().names.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// Link's data folders on this machine, as the file layout places them;
/// none where the home folder can't be found.
pub fn swept_folders() -> Vec<PathBuf> {
    get()
        .swept
        .iter()
        .filter_map(|home| home.try_path().ok())
        .collect()
}

/// Whether `folder` (resolved) is in one of `swept` (resolved), or is one.
pub fn in_swept(folder: &Path, swept: &[PathBuf]) -> bool {
    swept.iter().any(|data| folder.starts_with(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_reads_and_names_match_whatever_their_case() {
        let read = get();
        assert_eq!(read.names, [".git", ".envrc", ".vscode", ".idea"]);
        assert!(read.hook_managers.iter().any(|h| h == ".husky"));
        assert_eq!(
            read.swept,
            [
                link_files::layout::Home::Agents,
                link_files::layout::Home::Teams
            ]
        );
        assert!(named(".GIT") && named(".Vscode") && !named(".github"));
    }

    #[test]
    fn a_folder_is_swept_only_below_a_data_folder() {
        let swept = [
            PathBuf::from("/h/Link/Agents"),
            PathBuf::from("/h/Link/Teams"),
        ];
        assert!(in_swept(Path::new("/h/Link/Agents/main/home"), &swept));
        assert!(in_swept(Path::new("/h/Link/Teams"), &swept));
        assert!(!in_swept(Path::new("/h/Link/AgentsX"), &swept));
        assert!(!in_swept(Path::new("/h/code/app"), &swept));
    }
}

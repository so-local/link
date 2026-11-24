//! The repository's instructions: in each folder from the one the agent
//! works in, its Home, up to the repository's root, the folder's
//! `AGENTS.md`, else its `CLAUDE.md`, the nearest folder's first. The
//! root is the nearest folder holding `.git` (a folder, or a worktree's
//! file). Outside a repository, or where its root is out of the
//! sandbox's sight, Home's alone. A file the sandbox doesn't let the
//! harness read, a link, or anything but a regular file is left out:
//! what's read is what its sandbox lets it read, and nothing through a
//! link. Read at each turn, so an edit counts at the next message.

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use link_agent_spec::SECTION_MAX;

/// The names a folder's instructions are under, the first there read.
pub const NAMES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// One folder's instructions: where the file is, and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub text: String,
}

/// The folders whose instructions are read, nearest first: `home` and
/// each folder above it up to the nearest holding `.git`; `home` alone
/// where none is in sight.
pub fn folders(home: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for folder in home.ancestors() {
        folders.push(folder.to_path_buf());
        match std::fs::symlink_metadata(folder.join(".git")) {
            Ok(_) => return folders,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            // A folder it can't look in: the root is out of sight.
            Err(_) => break,
        }
    }
    vec![home.to_path_buf()]
}

/// Each folder's instructions ([`folders`]), nearest first: its first
/// file of [`NAMES`] that holds something, at most a section's most of
/// it.
pub fn read(home: &Path) -> Vec<Found> {
    folders(home)
        .into_iter()
        .filter_map(|folder| {
            NAMES.iter().find_map(|name| {
                let path = folder.join(name);
                let text = read_file(&path)?;
                Some(Found { path, text })
            })
        })
        .collect()
}

/// The folders of each of [`folders`] a repository keeps its skills in,
/// as Agent Skills and Claude Code read them.
pub const SKILLS: [&str; 2] = [".agents/skills", ".claude/skills"];

/// A skill of the repository's: its name and what it's for, and its
/// `SKILL.md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub skill: link_harness_wire::skill::Skill,
    pub path: PathBuf,
}

/// The repository's skills, from `home` up to its root ([`folders`]),
/// each folder's [`SKILLS`] in turn: a folder holding a `SKILL.md` that
/// says its name and what it's for, never through a link, the nearest of
/// a name the one kept, at most `most` of them, by name in each folder.
pub fn skills(home: &Path, most: usize) -> Vec<Skill> {
    let mut found: Vec<Skill> = Vec::new();
    for folder in folders(home) {
        for skills in SKILLS {
            let skills = folder.join(skills);
            let is_dir = |path: &Path| {
                std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
            };
            if !is_dir(&skills) {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&skills) else {
                continue;
            };
            let mut names: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .filter(|path| is_dir(path))
                .collect();
            names.sort();
            for dir in names {
                let path = dir.join(link_harness_wire::skill::FILE);
                let Some(skill) =
                    read_file(&path).and_then(|text| link_harness_wire::skill::parse(&text).ok())
                else {
                    continue;
                };
                if found.len() >= most {
                    return found;
                }
                if !found.iter().any(|f| f.skill.name == skill.name) {
                    found.push(Skill { skill, path });
                }
            }
        }
    }
    found
}

/// The regular file at `path`, never through a link, at most
/// [`SECTION_MAX`] bytes of it: none when it isn't there, can't be read,
/// or holds nothing.
pub fn read_file(path: &Path) -> Option<String> {
    read_file_receipt(path, &path.display().to_string()).map(|read| read.text)
}

pub(crate) fn read_file_receipt(
    path: &Path,
    source: &str,
) -> Option<crate::store::InstructionRead> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        // A pipe doesn't hold the turn; a link isn't followed.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(SECTION_MAX + 1).read_to_end(&mut bytes).ok()?;
    crate::store::InstructionRead::bounded(source, &bytes, SECTION_MAX as usize, true)
        .ok()
        .flatten()
}

/// The repository's section: each file under its path, nearest first,
/// in at most `max` bytes, whole files only past the nearest, which the
/// prompt cuts at its most; none without a file. Whether one was left
/// out for room.
pub fn section(found: &[Found], max: usize) -> (String, bool) {
    if found.is_empty() {
        return (String::new(), false);
    }
    let mut text = format!(
        "# The repository's instructions\n\nThe repository you work in says how to work in it, \
         in its {} (else {}) files, the nearest folder's first: where they differ, the \
         nearest folder's holds.\n",
        NAMES[0], NAMES[1]
    );
    for (n, one) in found.iter().enumerate() {
        let part = format!("\n## {}\n\n{}\n", one.path.display(), one.text);
        if n > 0 && text.len() + part.len() > max {
            return (text, true);
        }
        text.push_str(&part);
    }
    (text, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_test_kit::TempDir;

    /// A repository in `dir`, its root `dir/repo` and Home two folders
    /// down: its own `.git`, since a machine's temporary folder may be
    /// inside a checkout.
    fn repository(dir: &Path) -> (PathBuf, PathBuf) {
        let root = dir.join("repo");
        let home = root.join("app/web");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir(root.join(".git")).unwrap();
        (root, home)
    }

    #[test]
    fn the_nearest_folder_is_read_first_up_to_the_root() {
        let dir = TempDir::new("repository-walk");
        let (root, home) = repository(dir.path());
        std::fs::write(root.join("AGENTS.md"), "Run the checks.").unwrap();
        // A folder with both: AGENTS.md alone.
        std::fs::write(root.join("app/AGENTS.md"), "Use tabs.").unwrap();
        std::fs::write(root.join("app/CLAUDE.md"), "Never read.").unwrap();
        std::fs::write(home.join("CLAUDE.md"), "Write TypeScript.").unwrap();
        // Above the root: never read.
        std::fs::write(dir.path().join("AGENTS.md"), "Outside.").unwrap();

        let found = read(&home);
        let said: Vec<(&Path, &str)> = found
            .iter()
            .map(|f| (f.path.as_path(), f.text.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                (home.join("CLAUDE.md").as_path(), "Write TypeScript."),
                (root.join("app/AGENTS.md").as_path(), "Use tabs."),
                (root.join("AGENTS.md").as_path(), "Run the checks."),
            ]
        );
    }

    #[test]
    fn a_worktree_s_git_file_is_its_root() {
        let dir = TempDir::new("repository-worktree");
        let root = dir.path().join("tree");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join(".git"), "gitdir: /elsewhere\n").unwrap();
        assert_eq!(folders(&root.join("src")), [root.join("src"), root.clone()]);
    }

    #[test]
    fn outside_a_repository_home_s_alone() {
        let dir = TempDir::short("repository-none");
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(home.join("AGENTS.md"), "Mine.").unwrap();
        if let Some(root) = home
            .ancestors()
            .find(|folder| std::fs::symlink_metadata(folder.join(".git")).is_ok())
        {
            link_test_kit::gate::lacks(
                "a fixture outside a repository",
                format_args!("{} is inside {}", home.display(), root.display()),
                None,
            );
            return;
        }
        assert_eq!(folders(&home), std::slice::from_ref(&home));
        assert_eq!(read(&home).len(), 1);
    }

    #[test]
    fn links_empty_files_and_folders_are_left_out() {
        let dir = TempDir::new("repository-left-out");
        let (root, home) = repository(dir.path());
        std::fs::write(dir.path().join("secret.md"), "Not this.").unwrap();
        // A link as AGENTS.md: not followed.
        std::os::unix::fs::symlink(dir.path().join("secret.md"), home.join("AGENTS.md")).unwrap();
        // An empty AGENTS.md: CLAUDE.md is read.
        std::fs::write(root.join("app/AGENTS.md"), " \n").unwrap();
        std::fs::write(root.join("app/CLAUDE.md"), "Claude's.").unwrap();
        // A folder named AGENTS.md: left out.
        std::fs::create_dir(root.join("AGENTS.md")).unwrap();

        let found = read(&home);
        let texts: Vec<&str> = found.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(texts, ["Claude's."]);
    }

    #[test]
    fn the_farthest_files_leave_first_for_room() {
        let found = |path: &str, text: &str| Found {
            path: PathBuf::from(path),
            text: text.to_string(),
        };
        let files = [
            found("/r/a/AGENTS.md", "near"),
            found("/r/AGENTS.md", &"far ".repeat(100)),
        ];
        let (text, cut) = section(&files, 4096);
        assert!(!cut);
        assert!(
            text.starts_with("# The repository's instructions\n"),
            "{text}"
        );
        let near = text.find("## /r/a/AGENTS.md\n\nnear\n").unwrap();
        assert!(near < text.find("## /r/AGENTS.md").unwrap(), "{text}");

        let (text, cut) = section(&files, 300);
        assert!(cut);
        assert!(text.contains("near") && !text.contains("far"), "{text}");
        assert_eq!(section(&[], 300), (String::new(), false));
    }
}

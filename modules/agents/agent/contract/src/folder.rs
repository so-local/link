//! An agent's folder on this machine (`~/Link/Agents/<name>/`), laid out
//! as `docs/architecture/configuration.md` says ("An agent's
//! folder"): `home/`, the person's (or the folder `lnk agent start
//! --files` named), and `agent/`, the agent's own, open to the person.
//! Each collection kept at [`link_store_contract::storage::FILE`] is one folder:
//! `home` is Home, any other `agent/<collection>/`.
//!
//! What the agent sees of it, its tools and its sandbox file's `[files]`
//! naming folders by their first part as they did before Home and Agent
//! were two, is Home with its instructions, skills and conversations
//! beside Home's own folders ([`SEEN`], [`Folder::seen`]): a path whose
//! first part names one of those is in `agent/`, any other in Home. A
//! move carries what the agent sees the same way (`files/<path>`,
//! [`crate::moving`]).

use link_format::WithinExt;
use std::path::{Path, PathBuf};

use link_store_contract::storage::Collection;

/// The person's folder, in the agent's: its `home` collection, unless
/// `lnk agent start --run.files` named another.
pub const HOME: &str = "home";

/// The agent's own folder, in the agent's: each of its collections but
/// `home` kept at `file`, a folder each.
pub const AGENT: &str = "agent";

/// The collections the agent sees beside Home's folders, by their names
/// (`instructions/instruction.md`): every other first part is Home's.
/// Its `memory` and `schedule` it never sees but through its tools.
pub const SEEN: [Collection; 3] = [
    Collection::Instructions,
    Collection::Skills,
    Collection::Conversations,
];

/// The collection of [`SEEN`] a path of what the agent sees names by its
/// first part, if any (`./` before it read past): `memory/` is Home's.
pub fn seen_collection(path: &str) -> Option<Collection> {
    Collection::named_by(path, &SEEN)
}

/// An agent's two folders on this machine: Home, and its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// Its `home` collection: `<folder>/home`, or the one it was given.
    pub home: PathBuf,
    /// Its own folder: `<folder>/agent`.
    pub agent: PathBuf,
}

impl Folder {
    /// The agent whose folder is `dir` (`~/Link/Agents/<name>`), its Home
    /// `home` when it was given one, else `dir/home`.
    pub fn new(dir: &Path, home: Option<&Path>) -> Self {
        Self {
            home: home.map_or_else(|| dir.join(HOME), Path::to_path_buf),
            agent: dir.join(AGENT),
        }
    }

    /// A folder holding both as one, `dir`: a team's shared folder, or an
    /// agent's a harness laid out before the two. What the agent sees is
    /// all in it.
    pub fn flat(dir: &Path) -> Self {
        Self {
            home: dir.to_path_buf(),
            agent: dir.to_path_buf(),
        }
    }

    /// Where `collection` is kept at `file`: Home, or `agent/<name>`.
    pub fn of(&self, collection: Collection) -> PathBuf {
        match collection {
            Collection::Home => self.home.clone(),
            other => self.agent.join(other.directory()),
        }
    }

    /// Where memory engine `engine` keeps the agent's index and the rest
    /// of its own state: `agent/memory/<engine>`.
    pub fn index(&self, engine: &str) -> PathBuf {
        self.of(Collection::Memory).join(engine)
    }

    /// Where `path` of what the agent sees is (`instructions/a.md`,
    /// `work/x`): in its own folder where the first part names one of
    /// [`SEEN`], else in Home. `""` is Home.
    pub fn seen(&self, path: &str) -> PathBuf {
        match seen_collection(path) {
            Some(_) => self.agent.join(path),
            None if path.is_empty() => self.home.clone(),
            None => self.home.join(path),
        }
    }

    /// An error where its own folder, or one of its collections' folders
    /// in it, is a link: Link never follows one there.
    pub fn check_links(&self) -> anyhow::Result<()> {
        let link =
            |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
        let paths = std::iter::once(self.agent.clone()).chain(
            Collection::ALL
                .into_iter()
                .filter(|c| *c != Collection::Home)
                .map(|c| self.of(c)),
        );
        for path in paths {
            anyhow::ensure!(
                !link(&path),
                "{} is a link: Link doesn't follow it (remove it to go on)",
                path.display()
            );
        }
        Ok(())
    }

    /// Makes each folder of the layout that isn't there, Home's own
    /// ([`link_harness_contract::harness::layout::IN_HOME`]) and one a collection in its
    /// own folder; what's there is left. Its own folder and its memory's
    /// are the user's alone. A link where its own folder or one of its
    /// collections' is (a backup's, a hand's) is refused, never followed.
    pub fn make(&self) -> anyhow::Result<()> {
        self.check_links()?;
        link_files::create_private_dir(&self.agent)
            .within(format!("creating {}", self.agent.display()))?;
        std::fs::create_dir_all(&self.home).within(format!("creating {}", self.home.display()))?;
        for name in link_harness_contract::harness::layout::IN_HOME {
            make_folder(&self.home.join(name))?;
        }
        for collection in Collection::ALL {
            if collection == Collection::Home {
                continue;
            }
            let path = self.of(collection);
            match collection {
                Collection::Memory => link_files::create_private_dir(&path)
                    .within(format!("creating {}", path.display()))?,
                _ => make_folder(&path)?,
            }
        }
        Ok(())
    }
}

/// Whether a folder is at `path`, not a link to one.
pub fn is_folder(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}

/// Makes folder `path` if nothing is there; whatever is there is left.
pub fn make_folder(path: &Path) -> anyhow::Result<()> {
    match std::fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(err).within(format!("creating {}", path.display())),
    }
}

/// Why the harness mustn't write `files`, if it mustn't: every
/// sandbox's rule ([`link_sandbox_contract::unsafe_files`]).
pub use link_sandbox_contract::unsafe_files;

#[cfg(test)]
mod tests {
    use super::*;

    /// Each collection's folder, and what the agent sees, from one
    /// layout.
    #[test]
    fn each_collection_has_one_folder() {
        let folder = Folder::new(Path::new("/h/Link/Agents/ada"), None);
        assert_eq!(folder.home, Path::new("/h/Link/Agents/ada/home"));
        assert_eq!(
            folder.of(Collection::Instructions),
            Path::new("/h/Link/Agents/ada/agent/instructions")
        );
        assert_eq!(
            folder.of(Collection::Schedule),
            Path::new("/h/Link/Agents/ada/agent/schedule")
        );
        assert_eq!(
            folder.index("link-memory"),
            Path::new("/h/Link/Agents/ada/agent/memory/link-memory")
        );
        assert_eq!(
            folder.seen("instructions/notes.md"),
            Path::new("/h/Link/Agents/ada/agent/instructions/notes.md")
        );
        assert_eq!(
            folder.seen("chats"),
            Path::new("/h/Link/Agents/ada/agent/chats")
        );
        // Home's memory/ is the person's documents, not the collection.
        assert_eq!(
            folder.seen("memory/a.pdf"),
            Path::new("/h/Link/Agents/ada/home/memory/a.pdf")
        );
        assert_eq!(folder.seen(""), Path::new("/h/Link/Agents/ada/home"));
        // A Home it was given; its own folder stays its own.
        let given = Folder::new(Path::new("/h/Link/Agents/ada"), Some(Path::new("/h/code")));
        assert_eq!(given.seen("work"), Path::new("/h/code/work"));
        assert_eq!(
            given.seen("skills/a/SKILL.md"),
            Path::new("/h/Link/Agents/ada/agent/skills/a/SKILL.md")
        );
    }

    #[test]
    fn its_folders_are_made_and_left() {
        let dir = std::env::temp_dir().join(format!("lnk-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = Folder::new(&dir, None);
        folder.make().unwrap();
        std::fs::write(folder.of(Collection::Instructions).join("a.md"), "a").unwrap();
        folder.make().unwrap();
        for name in [
            "home/work",
            "home/memory",
            "agent/instructions/a.md",
            "agent/schedule",
        ] {
            assert!(dir.join(name).exists(), "{name}");
        }
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&folder.agent), 0o700);
        assert_eq!(mode(&folder.of(Collection::Memory)), 0o700);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

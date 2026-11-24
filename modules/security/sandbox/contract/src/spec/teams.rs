//! A team's shared folder as a sandbox spec grants it: `[teams.<team>]`,
//! the folders of it an agent writes and only reads. Only the harness
//! knows where a team's shared folder is, so a sandbox run of its own
//! refuses the table, and an agent's sandbox file is where it's used.

use std::path::PathBuf;

use super::TeamFolders;

/// The name in [`TeamFolders`] for a team's whole shared folder.
pub const WHOLE: &str = ".";

/// `teams.toml`, as read.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Paths {
    in_paths: String,
}

/// Where an agent's file tool takes a team's shared folder in a call's
/// paths, `teams` (`teams/<team>/`), whatever its place on disk: one
/// folder's name, from `teams.toml`, read once.
pub fn in_paths() -> &'static str {
    static READ: std::sync::OnceLock<Paths> = std::sync::OnceLock::new();
    &READ
        .get_or_init(|| toml::from_str(include_str!("teams.toml")).expect("teams.toml reads"))
        .in_paths
}

impl TeamFolders {
    /// The folders it writes.
    pub fn written(&self) -> &[String] {
        &self.write
    }

    /// The folders it only reads.
    pub fn only_read(&self) -> &[String] {
        &self.read
    }

    /// Both's folders: each written where either writes it, the rest
    /// read where either reads it.
    pub fn join(&self, more: &TeamFolders) -> TeamFolders {
        let mut write = self.written().to_vec();
        for name in more.written() {
            if !write.contains(name) {
                write.push(name.clone());
            }
        }
        let mut read: Vec<String> = Vec::new();
        for name in self.only_read().iter().chain(more.only_read()) {
            if !write.contains(name) && !read.contains(name) {
                read.push(name.clone());
            }
        }
        TeamFolders { write, read }
    }

    /// These folders without `less`'s: a folder it names is neither
    /// written nor read any more.
    pub fn without(&self, less: &TeamFolders) -> TeamFolders {
        let gone = |name: &String| less.written().contains(name) || less.only_read().contains(name);
        TeamFolders {
            write: self
                .written()
                .iter()
                .filter(|n| !gone(n))
                .cloned()
                .collect(),
            read: self
                .only_read()
                .iter()
                .filter(|n| !gone(n))
                .cloned()
                .collect(),
        }
    }

    /// Whether it reaches nothing of the folder.
    pub fn is_empty(&self) -> bool {
        self.written().is_empty() && self.only_read().is_empty()
    }

    /// Whether it reaches everything `need` does: each folder `need`
    /// writes written, and each it reads read or written.
    pub fn covers(&self, need: &TeamFolders) -> bool {
        let reads = |name: &str| {
            self.writes(name) || self.only_read().iter().any(|n| n == WHOLE || n == name)
        };
        need.written().iter().all(|n| self.writes(n)) && need.only_read().iter().all(|n| reads(n))
    }

    /// An error for a name that isn't one of the shared folder's folders
    /// (lowercase, one level) or `.`, one named twice or in both lists,
    /// or `.` beside a folder it would hold that's reached otherwise (a
    /// folder only read inside one written: the sandbox can't keep it),
    /// naming `place`, where they're written (`[teams.research]`).
    pub fn check(&self, place: &str) -> anyhow::Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for name in self.written().iter().chain(self.only_read()) {
            anyhow::ensure!(
                name == WHOLE || folder_name(name),
                "{place} {:?} isn't a folder of the team's shared folder \
                 (lowercase, one level: \"drafts\"), or \".\" for all of it",
                link_format::clean(name)
            );
            anyhow::ensure!(
                seen.insert(name.as_str()),
                "{place} names {name} twice: a folder is written or read, not both"
            );
        }
        anyhow::ensure!(
            !self.written().iter().any(|n| n == WHOLE)
                || self.written().len() + self.only_read().len() == 1,
            "{place} writes \".\", the whole folder: it can't name another"
        );
        Ok(())
    }

    /// The folders of `shared` it writes, and those it only reads.
    pub fn paths(&self, shared: &std::path::Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let path = |name: &String| match name.as_str() {
            WHOLE => shared.to_path_buf(),
            name => shared.join(name),
        };
        (
            self.written().iter().map(path).collect(),
            self.only_read().iter().map(path).collect(),
        )
    }

    /// Whether it may write `name`, a folder of the shared folder.
    pub fn writes(&self, name: &str) -> bool {
        self.written().iter().any(|n| n == WHOLE || n == name)
    }

    /// Whether it reaches the whole shared folder, read or written: what
    /// a team's memory, which searches all of it, is handed for.
    pub fn whole(&self) -> bool {
        self.written()
            .iter()
            .chain(self.only_read())
            .any(|n| n == WHOLE)
    }

    /// Whether it writes the whole shared folder: what keeping a team's
    /// memory's index beside it takes.
    pub fn writes_whole(&self) -> bool {
        self.written().iter().any(|n| n == WHOLE)
    }

    /// Whether `name` is one a grant may name: a folder of the shared
    /// folder, lowercase, one level.
    pub fn is_folder_name(name: &str) -> bool {
        folder_name(name)
    }
}

/// Whether `name` is a folder's name as `[files]` takes one: lowercase,
/// one level.
fn folder_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A team's folder in a call's paths is under one plain folder.
    #[test]
    fn a_team_folder_is_under_one_plain_name_in_paths() {
        assert_eq!(in_paths(), "teams");
        assert!(folder_name(in_paths()));
    }
}

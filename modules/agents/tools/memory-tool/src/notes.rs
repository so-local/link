//! The agent's notes: one markdown file of its instructions
//! (`memory/notes.md`), what it noted for itself, a note a line.
//! `remember` adds a note, changes one or forgets one, and never lets the
//! file grow past its most, so the notes stay a section of the agent's
//! prompt (its harness reads the file into it).
//!
//! Link names the file (`LNK_MEMORY_NOTES`) only where the agent may
//! write its instructions; without it, there's no `remember`, and
//! nothing of the folder is ever written. The file is written in one
//! step, through the handle of the folder it's in, never through a link,
//! one writer at a time.

use std::path::{Component, Path};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use crate::folder::{Dir, Kind};

/// The most the notes hold, in bytes, when Link doesn't say (`serve
/// --notes-most`, from the agent's spec): the engine's own, for use
/// outside Link.
pub const MOST: u64 = link_memory_tool_contract::NOTES_MOST;

/// The agent's notes file, and the most it may hold.
#[derive(Clone)]
pub struct Notes {
    /// Its folder, in the folder served (`instructions`), as names.
    folder: Vec<String>,
    /// Its file's name (`notes.md`).
    name: String,
    /// Where it's kept: in the folder served, or through the agent's
    /// storage bridge.
    kept: Kept,
    /// The most it may hold, in bytes.
    pub most: u64,
}

/// Where the notes are kept.
#[derive(Clone)]
enum Kept {
    /// In the folder served, this one, written through its handle.
    Folder(std::path::PathBuf),
    /// As the agent's file of that path, through its storage bridge
    /// ([`link_store_contract::store`]), written only at the version read.
    Stored(
        std::sync::Arc<dyn link_store_contract::store::Store + Send>,
        link_store_contract::store::Agent,
    ),
}

impl std::fmt::Debug for Notes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notes")
            .field("path", &self.path())
            .field("most", &self.most)
            .finish()
    }
}

/// How a note says the conversation it was saved in: after it, as
/// `(from telegram:42)`.
const FROM_OPEN: &str = "(from ";
const FROM_CLOSE: &str = ")";

/// A note's line without the conversation it says it was saved in.
fn unlabelled(line: &str) -> &str {
    match line.rfind(&format!(" {FROM_OPEN}")) {
        Some(at) if line.ends_with(FROM_CLOSE) => &line[..at],
        _ => line,
    }
}

/// The most times a note is written again after another writer changed
/// the notes first.
const TRIES: usize = 16;

/// What `remember` did.
#[derive(Debug, PartialEq, Serialize)]
pub struct Remembered {
    /// `added`, `changed`, `forgot`, or `unchanged` (already noted).
    pub done: &'static str,
    /// The notes' path in memory, for `read`.
    pub path: String,
    /// Their size now, and the most they may be, in bytes.
    pub bytes: u64,
    pub most: u64,
}

impl Notes {
    /// The notes at `file`, a markdown file in `root` (the folder served)
    /// but never in its conversations, holding at most `most` bytes.
    pub fn new(root: &Path, file: &Path, most: u64) -> Result<Notes> {
        let rel = file.strip_prefix(root).with_context(|| {
            format!(
                "the notes ({}) are outside the folder served",
                file.display()
            )
        })?;
        let mut names = Vec::new();
        for part in rel.components() {
            match part {
                Component::Normal(name) => names.push(
                    name.to_str()
                        .context("the notes' path isn't text")?
                        .to_string(),
                ),
                _ => bail!("the notes' path ({}) isn't plain", file.display()),
            }
        }
        let Some((name, folder)) = names.split_last() else {
            bail!("the notes are a file, not the folder served");
        };
        ensure!(
            name.ends_with(".md"),
            "the notes ({}) are a markdown file",
            file.display()
        );
        ensure!(
            folder.first().map(String::as_str) != Some(crate::CONVERSATIONS)
                && names.iter().all(|n| !n.starts_with('.')),
            "the notes ({}) can't be in conversations or a hidden folder",
            file.display()
        );
        ensure!(most > 0, "the notes may hold at least a byte");
        let notes = Notes {
            folder: folder.to_vec(),
            name: name.clone(),
            kept: Kept::Folder(root.to_path_buf()),
            most,
        };
        // Made private as they're opened: a folder not made yet has none.
        if let Ok(dir) = notes.dir(root) {
            dir.make_private(&notes.name)
                .with_context(|| format!("making {} yours alone", notes.path()))?;
        }
        Ok(notes)
    }

    /// The folder the notes are in, under `root`, opened a name at a time:
    /// a link on the way is refused.
    fn dir(&self, root: &Path) -> Result<Dir> {
        let mut dir = match self.folder.is_empty() {
            true => Dir::open(root),
            false => Dir::search(root),
        }
        .with_context(|| format!("opening {}", root.display()))?;
        for name in &self.folder {
            dir = dir.folder(name).with_context(|| {
                format!("opening {} in memory (a link is refused)", self.path())
            })?;
        }
        Ok(dir)
    }

    /// The agent's notes, its file `memory/notes.md` kept wherever
    /// its machine keeps its memory, read and written through its
    /// storage bridge (`store`, as `agent`), holding at most `most` bytes.
    pub fn stored(
        store: std::sync::Arc<dyn link_store_contract::store::Store + Send>,
        agent: link_store_contract::store::Agent,
        most: u64,
    ) -> Result<Notes> {
        ensure!(most > 0, "the notes may hold at least a byte");
        Ok(Notes {
            folder: vec![
                link_store_contract::storage::Collection::Memory
                    .name()
                    .to_string(),
            ],
            name: "notes.md".into(),
            kept: Kept::Stored(store, agent),
            most,
        })
    }

    /// The notes' path in the folder served (`memory/notes.md`).
    pub fn path(&self) -> String {
        let mut path = self.folder.join("/");
        if !path.is_empty() {
            path.push('/');
        }
        path + &self.name
    }

    /// Adds `note`, or, with `replaces`, changes the one note holding that
    /// text to `note`, or forgets it when `note` is empty. Refused past the
    /// most, saying how far.
    pub fn remember(&self, note: &str, replaces: Option<&str>) -> Result<Remembered> {
        self.remember_from(note, replaces, None)
    }

    /// [`Self::remember`], the note labelled with the conversation it was
    /// saved in (`from`, its thread), so the notes read as what was said
    /// where. A note that looks like it holds a secret is refused
    /// ([`link_format::scrub::secret_shaped`]): the notes are in every
    /// later prompt.
    pub fn remember_from(
        &self,
        note: &str,
        replaces: Option<&str>,
        from: Option<&str>,
    ) -> Result<Remembered> {
        // One line each: a note's own lines joined.
        let note = note.split_whitespace().collect::<Vec<_>>().join(" ");
        let replaces = replaces.map(str::trim).filter(|r| !r.is_empty());
        ensure!(
            !note.is_empty() || replaces.is_some(),
            "a note to add, or 'replaces' to change or forget one"
        );
        if let Some(why) = link_format::scrub::secret_shaped(&note) {
            bail!(
                "nothing was noted: {why}, and your notes are never where a secret is kept (they \
                 go into every later prompt). Tell your owner to keep it in a password manager"
            );
        }
        let note = match from
            .map(str::trim)
            .filter(|f| !f.is_empty() && !note.is_empty())
        {
            Some(from) => format!("{note} {FROM_OPEN}{}{FROM_CLOSE}", link_format::clean(from)),
            None => note,
        };
        let root = match &self.kept {
            Kept::Folder(root) => root,
            Kept::Stored(store, agent) => {
                return self.remember_stored(&**store, agent, &note, replaces);
            }
        };
        let dir = self.dir(root)?;
        dir.locked(|| self.write(&dir, &note, replaces))?
    }

    /// [`Self::remember`] through the storage bridge: read, changed, and
    /// written only at the version read, read again when another wrote
    /// first.
    fn remember_stored(
        &self,
        store: &dyn link_store_contract::store::Store,
        agent: &link_store_contract::store::Agent,
        note: &str,
        replaces: Option<&str>,
    ) -> Result<Remembered> {
        let path = self.path();
        for _ in 0..TRIES {
            let (before, version) = match store.file_read(agent, &path)? {
                Some((bytes, version)) => (
                    String::from_utf8(bytes).context("the notes aren't text")?,
                    version,
                ),
                None => (String::new(), 0),
            };
            let (after, done) = self.edited(&before, note, replaces)?;
            let Some(after) = after else {
                return Ok(self.remembered(done, before.len() as u64));
            };
            match store.file_write(agent, &path, after.as_bytes(), version)? {
                link_store_contract::store::Written::Version(_) => {
                    return Ok(self.remembered(done, after.len() as u64));
                }
                link_store_contract::store::Written::Stale(_) => continue,
            }
        }
        bail!("your notes kept changing as they were written: try again")
    }

    fn write(&self, dir: &Dir, note: &str, replaces: Option<&str>) -> Result<Remembered> {
        // What's there now, past the most too (edited by hand), to say so.
        let current = || -> Result<String> {
            Ok(
                match dir.read(
                    &self.name,
                    self.most.max(crate::limits::limits().file_max_bytes),
                )? {
                    Some(bytes) => String::from_utf8(bytes).context("the notes aren't text")?,
                    None => {
                        // None yet, or a link, which is replaced, never followed.
                        let large = dir
                            .entries()?
                            .iter()
                            .any(|e| e.name == self.name && e.kind == Kind::File);
                        ensure!(!large, "your notes are past 1 MB: they're edited by hand");
                        String::new()
                    }
                },
            )
        };
        // A hand edit takes no lock: one made while the note was added is
        // read again, never written over.
        for _ in 0..TRIES {
            let before = current()?;
            let (after, done) = self.edited(&before, note, replaces)?;
            let Some(after) = after else {
                return Ok(self.remembered(done, before.len() as u64));
            };
            if current()? != before {
                continue;
            }
            dir.replace(&self.name, after.as_bytes())
                .with_context(|| format!("writing {}", self.path()))?;
            return Ok(self.remembered(done, after.len() as u64));
        }
        bail!("your notes kept changing as they were written: try again")
    }

    /// The notes `before`, `note` added, changed or forgotten: what they
    /// are after (none when unchanged), and what was done. Refused past
    /// the most, saying how far.
    fn edited(
        &self,
        before: &str,
        note: &str,
        replaces: Option<&str>,
    ) -> Result<(Option<String>, &'static str)> {
        let mut lines: Vec<String> = before.lines().map(str::to_string).collect();
        let line = format!("- {note}");
        let done = match replaces {
            // Noted already, wherever it was.
            None if lines
                .iter()
                .any(|l| unlabelled(l.trim()) == unlabelled(&line)) =>
            {
                "unchanged"
            }
            None => {
                lines.push(line);
                "added"
            }
            Some(old) => {
                let holding: Vec<usize> = (0..lines.len())
                    .filter(|&i| lines[i].contains(old))
                    .collect();
                let &[at] = holding.as_slice() else {
                    match holding.len() {
                        0 => bail!("no note holds {old:?}: your notes are in your instructions"),
                        n => bail!("{n} notes hold {old:?}: give more of the one to change"),
                    }
                };
                match note.is_empty() {
                    true => {
                        lines.remove(at);
                        "forgot"
                    }
                    false => {
                        lines[at] = line;
                        "changed"
                    }
                }
            }
        };
        let mut after = lines.join("\n");
        if !after.is_empty() {
            after.push('\n');
        }
        let bytes = after.len() as u64;
        if done == "unchanged" {
            return Ok((None, done));
        }
        // Forgetting is always allowed: it's how notes past their most
        // (edited by hand) shrink.
        ensure!(
            bytes <= self.most || done == "forgot",
            "that makes your notes {bytes} bytes, past their most ({}): change or forget \
             some first ('replaces'), or say it in fewer words",
            self.most
        );
        Ok((Some(after), done))
    }

    fn remembered(&self, done: &'static str, bytes: u64) -> Remembered {
        Remembered {
            done,
            path: self.path(),
            bytes,
            most: self.most,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn notes_are_saved_beneath_a_parent_seatbelt_does_not_let_memory_read() {
        const ROOT: &str = "LNK_TEST_NOTES_PARENT";
        if let Some(root) = std::env::var_os(ROOT).map(std::path::PathBuf::from) {
            assert!(Dir::open(&root).is_err());
            let notes = Notes::new(&root, &root.join("memory/notes.md"), 4096).unwrap();
            assert_eq!(
                notes
                    .remember("My owner's favorite color is teal.", None)
                    .unwrap()
                    .done,
                "added"
            );
            assert!(std::fs::read(root.join("private")).is_err());
            return;
        }
        if !link_test_kit::gate::program(
            "/usr/bin/sandbox-exec",
            &["-p", "(version 1)(allow default)", "/usr/bin/true"],
        ) {
            return;
        }
        let temp = link_test_kit::TempDir::new("notes-seatbelt");
        let root = temp.canonicalize().unwrap();
        std::fs::create_dir(root.join("memory")).unwrap();
        std::fs::write(root.join("private"), "unrelated private data").unwrap();
        let escaped = root
            .display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let profile = format!(
            "(version 1)(allow default)(deny file-read-data (literal \"{escaped}\") (literal \"{escaped}/private\"))"
        );
        let output = link_test_kit::env::command("/usr/bin/sandbox-exec", &temp)
            .args(["-p", &profile])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "notes::tests::notes_are_saved_beneath_a_parent_seatbelt_does_not_let_memory_read",
                "--nocapture",
            ])
            .env(ROOT, &root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            std::fs::read_to_string(root.join("memory/notes.md"))
                .unwrap()
                .contains("teal")
        );
    }

    fn folder(name: &str) -> link_test_kit::TempDir {
        let root = link_test_kit::TempDir::new(&format!("notes-{name}"));
        std::fs::create_dir_all(root.join("instructions")).unwrap();
        root
    }

    #[test]
    fn notes_are_added_changed_and_forgotten() {
        let root = folder("write");
        let file = root.join("instructions/notes.md");
        let notes = Notes::new(&root, &file, 200).unwrap();
        assert_eq!(notes.path(), "instructions/notes.md");
        let added = notes.remember("Maya is my\n sister.", None).unwrap();
        assert_eq!((added.done, added.bytes), ("added", 21));
        notes.remember("Lunch is at noon.", None).unwrap();
        assert_eq!(
            notes.remember("Maya is my sister.", None).unwrap().done,
            "unchanged"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "- Maya is my sister.\n- Lunch is at noon.\n"
        );
        let changed = notes.remember("Lunch is at one.", Some("Lunch")).unwrap();
        assert_eq!(changed.done, "changed");
        // A piece of text held by none, or by two, changes nothing.
        assert!(notes.remember("x", Some("dinner")).is_err());
        assert!(notes.remember("x", Some("is")).is_err());
        assert_eq!(notes.remember("", Some("Maya")).unwrap().done, "forgot");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "- Lunch is at one.\n"
        );
        assert!(notes.remember("  ", None).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A note says the conversation it was saved in, is noted once
    /// wherever it's said again, and is the user's alone on disk; one
    /// that looks like it holds a secret is refused, nothing written.
    #[test]
    fn notes_say_where_they_were_saved_and_never_hold_a_secret() {
        use std::os::unix::fs::PermissionsExt;
        let root = folder("from");
        let file = root.join("instructions/notes.md");
        // Kept before, open to every user: private once opened.
        std::fs::write(&file, "").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let notes = Notes::new(&root, &file, 400).unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let added = notes
            .remember_from("Maya is my sister.", None, Some("telegram:42"))
            .unwrap();
        assert_eq!(added.done, "added");
        assert_eq!(
            notes
                .remember_from("Maya is my sister.", None, Some("link:main"))
                .unwrap()
                .done,
            "unchanged"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "- Maya is my sister. (from telegram:42)\n"
        );
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        for secret in [
            "The gym locker code is 4821.",
            "Wifi password: hunter2",
            "Key sk-abcdefghijklmnopqrstuv",
        ] {
            let why = notes
                .remember_from(secret, None, Some("telegram:42"))
                .unwrap_err()
                .to_string();
            assert!(why.contains("nothing was noted"), "{why}");
        }
        assert_eq!(
            notes
                .remember_from("", Some("Maya"), Some("telegram:42"))
                .unwrap()
                .done,
            "forgot"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "");
    }

    #[test]
    fn notes_never_grow_past_their_most() {
        let root = folder("most");
        let file = root.join("instructions/notes.md");
        let notes = Notes::new(&root, &file, 40).unwrap();
        notes.remember("a short one", None).unwrap();
        let err = notes.remember(&"long ".repeat(10), None).unwrap_err();
        assert!(format!("{err}").contains("past their most (40)"), "{err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "- a short one\n");
        // Past it by hand: forgetting still shrinks it.
        std::fs::write(&file, format!("- {}\n- old\n", "x".repeat(60))).unwrap();
        assert!(notes.remember("new", None).is_err());
        assert_eq!(notes.remember("", Some("old")).unwrap().done, "forgot");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn notes_are_never_written_through_a_link_or_outside() {
        let root = folder("links");
        let elsewhere = folder("elsewhere");
        // The notes' folder a link: refused, nothing written there.
        std::fs::remove_dir(root.join("instructions")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join("instructions")).unwrap();
        let notes = Notes::new(&root, &root.join("instructions/notes.md"), 100).unwrap();
        assert!(notes.remember("x", None).is_err());
        assert!(!elsewhere.join("notes.md").exists());
        for bad in [
            elsewhere.join("notes.md"),
            root.join("chats/telegram-42/conversation.md"),
            root.join("instructions/../x.md"),
            root.join("instructions/notes.txt"),
            root.join(".hidden/notes.md"),
            root.to_path_buf(),
        ] {
            assert!(Notes::new(&root, &bad, 100).is_err(), "{}", bad.display());
        }
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_dir_all(&elsewhere).unwrap();
    }
}

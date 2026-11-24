//! A folder on Linux, watched with inotify, the kernel's notice of what
//! changes in a folder: a watch on each folder memory indexes, its
//! changes noted in the [`Journal`] at once.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::journal::Journal;

/// What a folder's watch notices.
const FILES: u32 = libc::IN_CREATE
    | libc::IN_CLOSE_WRITE
    | libc::IN_MODIFY
    | libc::IN_ATTRIB
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_DELETE
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF
    | libc::IN_DONT_FOLLOW
    | libc::IN_ONLYDIR;

/// How often the watch's thread looks to see it's stopped, in
/// milliseconds.
const STOP_POLL_MS: i32 = 200;

/// A folder memory indexes, under the one it's in: `top` (`memory`,
/// `chats`) in `base`.
pub struct Root {
    pub base: PathBuf,
    pub top: &'static str,
}

/// The watches on an index's folders, noting each change in its journal
/// until dropped.
pub struct Watch {
    stopped: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The watches, by descriptor: each folder's path from the folder served
/// (`memory/taxes`), or, for a base, the top it waits for.
struct Watched {
    fd: OwnedFd,
    by: HashMap<i32, Watching>,
    /// The watches of folders moved within the tree, set again for their
    /// new path: the move's own event on the folder (`IN_MOVE_SELF`),
    /// which comes after, keeps it.
    moved: HashSet<i32>,
    journal: Arc<Journal>,
}

enum Watching {
    /// A base folder, waiting for its top to appear.
    Base { base: PathBuf, top: &'static str },
    /// A folder memory indexes, by its path from the folder served.
    Folder(String),
}

/// Watches `roots`, noting their changes in `journal`; none where
/// inotify can't be had.
pub fn watch(roots: Vec<Root>, journal: Arc<Journal>) -> Option<Watch> {
    // SAFETY: no pointers; the flags are inotify's own.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if fd < 0 {
        journal.watching(Some(format!(
            "the folder can't be watched ({})",
            std::io::Error::last_os_error()
        )));
        return None;
    }
    // SAFETY: a descriptor inotify_init1 just gave, owned from here.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut watched = Watched {
        fd,
        by: HashMap::new(),
        moved: HashSet::new(),
        journal: journal.clone(),
    };
    journal.watching(None);
    for root in roots {
        watched.add(
            &root.base,
            Watching::Base {
                base: root.base.clone(),
                top: root.top,
            },
        );
        let at = root.base.join(root.top);
        if at.is_dir() {
            watched.add_all(&at, root.top);
        }
    }
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    let thread = std::thread::spawn(move || watched.run(&stop));
    Some(Watch {
        stopped,
        thread: Some(thread),
    })
}

impl Watched {
    /// A watch on `dir`, as `watching`: the machine's limit reached, said
    /// in memory's status, and the folder walked more often. Its
    /// descriptor, and whether the folder was watched already, under
    /// another path.
    fn add(&mut self, dir: &Path, watching: Watching) -> Option<(i32, bool)> {
        let path = CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mask = match watching {
            Watching::Base { .. } => libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_DONT_FOLLOW,
            Watching::Folder(_) => FILES,
        };
        // SAFETY: an inotify descriptor this owns and a NUL-ended path.
        let wd = unsafe { libc::inotify_add_watch(self.fd.as_raw_fd(), path.as_ptr(), mask) };
        if wd < 0 {
            if let Some(why) = unwatched_by(&std::io::Error::last_os_error()) {
                self.journal.watching(Some(why));
            }
            return None;
        }
        // The same folder's watch, as inotify gives it again.
        let again = self.by.insert(wd, watching).is_some();
        Some((wd, again))
    }

    /// Watches on `dir` and every folder in it, `rel` its path from the
    /// folder served: no hidden folder, and never a link. `dir`'s, as
    /// [`Watched::add`] says.
    fn add_all(&mut self, dir: &Path, rel: &str) -> Option<(i32, bool)> {
        let added = self.add(dir, Watching::Folder(rel.to_string()))?;
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Some(added);
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            // The entry's own type: a link is never a folder here.
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                self.add_all(&entry.path(), &format!("{rel}/{name}"));
            }
        }
        Some(added)
    }

    /// The watch `wd`, of the folder at `rel`, and those of the folders in
    /// it, taken off: the folder left the tree.
    fn remove_all(&mut self, wd: i32, rel: &str) {
        let under = format!("{rel}/");
        let mut gone: Vec<i32> = self
            .by
            .iter()
            .filter(
                |(_, watching)| matches!(watching, Watching::Folder(at) if at.starts_with(&under)),
            )
            .map(|(wd, _)| *wd)
            .collect();
        gone.push(wd);
        for wd in gone {
            self.by.remove(&wd);
            self.moved.remove(&wd);
            // SAFETY: an inotify descriptor this owns and one of its
            // watches; one the kernel took off already fails, harmlessly.
            unsafe { libc::inotify_rm_watch(self.fd.as_raw_fd(), wd) };
        }
    }

    /// Reads events until stopped, noting each change.
    fn run(mut self, stopped: &AtomicBool) {
        let mut buffer = vec![0u8; 64 << 10];
        while !stopped.load(Ordering::SeqCst) {
            let mut poll = libc::pollfd {
                fd: self.fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one pollfd for a descriptor this owns.
            if unsafe { libc::poll(&mut poll, 1, STOP_POLL_MS) } <= 0 {
                continue;
            }
            // SAFETY: a buffer this owns, read to its length at most.
            let read = unsafe {
                libc::read(
                    self.fd.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if read <= 0 {
                continue;
            }
            let mut at = 0;
            let header = std::mem::size_of::<libc::inotify_event>();
            while at + header <= read as usize {
                // SAFETY: the kernel writes whole events, a header and its
                // name; read unaligned from the buffer.
                let event: libc::inotify_event =
                    unsafe { std::ptr::read_unaligned(buffer[at..].as_ptr().cast()) };
                let name_at = at + header;
                let name_end = (name_at + event.len as usize).min(read as usize);
                let name = buffer[name_at..name_end]
                    .split(|b| *b == 0)
                    .next()
                    .and_then(|name| std::str::from_utf8(name).ok())
                    .unwrap_or_default()
                    .to_string();
                at = name_end;
                self.event(event.wd, event.mask, &name);
            }
        }
    }

    /// One event on watch `wd`, of `name` in its folder.
    fn event(&mut self, wd: i32, mask: u32, name: &str) {
        if mask & libc::IN_Q_OVERFLOW != 0 {
            // Events were lost: what changed is found by a walk.
            self.journal.walk();
            return;
        }
        let Some(watching) = self.by.get(&wd) else {
            return;
        };
        match watching {
            Watching::Base { base, top } => {
                if name == *top {
                    let at = base.join(top);
                    let top = *top;
                    self.add_all(&at, top);
                    self.journal.walk();
                }
            }
            Watching::Folder(rel) => {
                if mask & libc::IN_MOVE_SELF != 0 {
                    // Moved within the tree, its watch set for the new
                    // path already; else out of it, no longer watched.
                    if !self.moved.remove(&wd) {
                        let rel = rel.clone();
                        self.remove_all(wd, &rel);
                    }
                    self.journal.walk();
                    return;
                }
                if mask & (libc::IN_DELETE_SELF | libc::IN_IGNORED) != 0 {
                    self.by.remove(&wd);
                    self.moved.remove(&wd);
                    self.journal.walk();
                    return;
                }
                if name.is_empty() || name.starts_with('.') {
                    return;
                }
                let path = format!("{rel}/{name}");
                if mask & libc::IN_ISDIR != 0 {
                    // A folder come or gone: what's in it is found by a
                    // walk, and one come is watched.
                    if mask & (libc::IN_CREATE | libc::IN_MOVED_TO) != 0
                        && let Some(dir) = self.dir_of(&path)
                        && let Some((wd, true)) = self.add_all(&dir, &path)
                    {
                        self.moved.insert(wd);
                    }
                    self.journal.walk();
                    return;
                }
                self.journal.note(noted_as(&path));
            }
        }
    }

    /// Where the folder at `path` (from the folder served) is on disk.
    fn dir_of(&self, path: &str) -> Option<PathBuf> {
        let top = path.split('/').next()?;
        self.by.values().find_map(|watching| match watching {
            Watching::Base { base, top: t } if *t == top => Some(base.join(path)),
            _ => None,
        })
    }
}

/// Why the folder isn't watched whole, when a watch fails with `err`,
/// said in memory's status: none for a folder gone meanwhile, which a
/// walk finds gone.
fn unwatched_by(err: &std::io::Error) -> Option<String> {
    match err.raw_os_error() {
        Some(libc::ENOENT | libc::ENOTDIR) => None,
        Some(libc::ENOSPC) => Some(
            "the machine's limit on watched folders is reached \
             (fs.inotify.max_user_watches): what changes is found by \
             a walk each minute"
                .into(),
        ),
        _ => Some(format!(
            "a folder can't be watched ({err}): what changes is found by a walk each minute"
        )),
    }
}

/// The path a change to `path` is noted as: a file in a conversation's
/// folder (its events, its frames) changes its copy, which memory indexes.
fn noted_as(path: &str) -> String {
    let mut parts = path.splitn(3, '/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(top), Some(thread), Some(_)) if top == crate::CONVERSATIONS => {
            format!("{top}/{thread}/{}", crate::copies::COPY)
        }
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Takes what `journal` noted until `path` is among it, failing at
    /// the deadline: everything noted before it was read by then.
    fn noted_until(journal: &Journal, path: &str) -> Vec<String> {
        let mut noted = Vec::new();
        link_test_kit::wait::eventually(path, Duration::from_secs(10), || {
            noted.extend(journal.take(usize::MAX));
            noted.iter().any(|at| at == path).then_some(())
        });
        noted
    }

    /// A folder renamed within `memory/` stays watched under its new
    /// name; one moved out of it is watched no more.
    #[test]
    fn a_folder_renamed_stays_watched() {
        let root = link_test_kit::TempDir::new("inotify-rename");
        std::fs::create_dir_all(root.join("memory/a/deep")).unwrap();
        let journal = Arc::new(Journal::default());
        let roots = vec![Root {
            base: root.to_path_buf(),
            top: "memory",
        }];
        let _watch = watch(roots, journal.clone()).expect("inotify");
        std::fs::rename(root.join("memory/a"), root.join("memory/b")).unwrap();
        std::fs::write(root.join("memory/b/x.md"), "x").unwrap();
        noted_until(&journal, "memory/b/x.md");
        std::fs::write(root.join("memory/b/deep/y.md"), "y").unwrap();
        noted_until(&journal, "memory/b/deep/y.md");
        // Out of the tree: what changes there is noted no more.
        std::fs::rename(root.join("memory/b"), root.join("out")).unwrap();
        std::fs::write(root.join("out/z.md"), "z").unwrap();
        std::fs::write(root.join("memory/w.md"), "w").unwrap();
        let noted = noted_until(&journal, "memory/w.md");
        assert!(noted.iter().all(|at| !at.ends_with("z.md")), "{noted:?}");
    }

    /// A watch that fails for a reason other than the folder being gone
    /// says why in memory's status, the limit reached among them.
    #[test]
    fn a_watch_that_fails_says_why() {
        let of = |code| unwatched_by(&std::io::Error::from_raw_os_error(code));
        assert_eq!(of(libc::ENOENT), None);
        assert!(of(libc::ENOSPC).unwrap().contains("max_user_watches"));
        assert!(of(libc::EACCES).unwrap().contains("can't be watched"));
    }

    /// A change in a conversation's folder is its copy's; one in
    /// `memory/` is its own.
    #[test]
    fn a_conversations_files_are_noted_as_its_copy() {
        assert_eq!(
            noted_as("chats/t42/events.jsonl"),
            "chats/t42/conversation.md"
        );
        assert_eq!(
            noted_as("chats/t42/conversation.md"),
            "chats/t42/conversation.md"
        );
        assert_eq!(noted_as("memory/a/b.md"), "memory/a/b.md");
        assert_eq!(noted_as("chats/loose.md"), "chats/loose.md");
    }
}

//! The change journal ([`Journal`]): the paths that may have changed
//! since they were indexed, written by whoever learns of a change (a
//! watch on the folder, the files bridge's notice, a conversation's
//! store) and drained by the index's writer, so no call walks the folder.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// What wakes the writer when a change is noted.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The paths that may have changed, and whether a whole walk is asked.
#[derive(Default)]
pub struct Journal {
    noted: Mutex<Noted>,
    /// Whether a walk of the whole folder is asked: a watch lost events
    /// or can't see a part, or a folder appeared.
    walk: AtomicBool,
    /// Whether the folder's watch reaches all of it: else it's walked
    /// more often ([`crate::limits::Limits::walk_unwatched_secs`]).
    watched: AtomicBool,
    /// Why a watch can't reach all of it, said in memory's status.
    unwatched: Mutex<Option<String>>,
    waker: Mutex<Option<Waker>>,
    /// How many changes were noted ([`Journal::note`]), ever: a batch
    /// being written stops at the next file once it grows
    /// ([`Journal::notes`]).
    notes: AtomicU64,
}

#[derive(Default)]
struct Noted {
    paths: BTreeSet<String>,
    /// When a path was last noted.
    last: Option<Instant>,
}

impl Journal {
    /// `path` (from the folder served) may have changed: the writer is
    /// woken to index it.
    pub fn note(&self, path: impl Into<String>) {
        {
            let mut noted = self.noted.lock().unwrap_or_else(|e| e.into_inner());
            noted.paths.insert(path.into());
            noted.last = Some(Instant::now());
        }
        self.notes.fetch_add(1, Ordering::SeqCst);
        self.wake();
    }

    /// How many changes were noted, ever: a path tried again
    /// ([`Journal::retry`]) isn't one.
    pub(crate) fn notes(&self) -> u64 {
        self.notes.load(Ordering::SeqCst)
    }

    /// `path` couldn't be reached: noted again, without waking the writer,
    /// so it's tried at its next turn rather than at once.
    pub(crate) fn retry(&self, path: &str) {
        let mut noted = self.noted.lock().unwrap_or_else(|e| e.into_inner());
        noted.paths.insert(path.to_string());
        noted.last = Some(Instant::now());
    }

    /// A walk of the whole folder is asked, soon.
    pub fn walk(&self) {
        self.walk.store(true, Ordering::SeqCst);
        self.wake();
    }

    /// Whether anything waits: a path noted, or a walk asked.
    pub(crate) fn news(&self) -> bool {
        self.walk.load(Ordering::SeqCst) || self.waiting() > 0
    }

    /// Whether a walk was asked, the ask taken.
    pub(crate) fn take_walk(&self) -> bool {
        self.walk.swap(false, Ordering::SeqCst)
    }

    /// Up to `most` paths noted, taken, the first by name.
    pub(crate) fn take(&self, most: usize) -> Vec<String> {
        let mut noted = self.noted.lock().unwrap_or_else(|e| e.into_inner());
        let taken: Vec<String> = noted.paths.iter().take(most).cloned().collect();
        for path in &taken {
            noted.paths.remove(path);
        }
        taken
    }

    /// Every path noted, forgotten: a walk sees them all.
    pub(crate) fn clear(&self) {
        self.noted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .paths
            .clear();
    }

    /// How many paths wait to be indexed.
    pub fn waiting(&self) -> usize {
        self.noted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .paths
            .len()
    }

    /// When a path was last noted, while any waits.
    pub(crate) fn last(&self) -> Option<Instant> {
        let noted = self.noted.lock().unwrap_or_else(|e| e.into_inner());
        noted.last.filter(|_| !noted.paths.is_empty())
    }

    /// Notes whether a watch reaches the whole folder, and why not.
    pub fn watching(&self, why_not: Option<String>) {
        self.watched.store(why_not.is_none(), Ordering::SeqCst);
        *self.unwatched.lock().unwrap_or_else(|e| e.into_inner()) = why_not;
    }

    /// Whether a watch reaches the whole folder.
    pub(crate) fn watched(&self) -> bool {
        self.watched.load(Ordering::SeqCst)
    }

    /// Why a watch can't reach all of it, if it can't.
    pub(crate) fn unwatched(&self) -> Option<String> {
        self.unwatched
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// What wakes the writer from now on.
    pub fn wake_with(&self, waker: Waker) {
        *self.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(waker);
    }

    fn wake(&self) {
        let waker = self.waker.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(waker) = waker {
            waker();
        }
    }
}

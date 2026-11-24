//! How an index stays current without a call walking its folder: the
//! change journal ([`Journal`]), and the watch that feeds it from a
//! folder on this machine ([`watch`]).

#[cfg(target_os = "linux")]
mod inotify;
pub mod journal;
pub(crate) mod words;

pub use journal::Journal;

#[cfg(target_os = "linux")]
pub use inotify::{Root, Watch};

/// A watch on `roots`, noting what changes in `journal`; none where this
/// system has no watch here yet, said in memory's status, and the folder
/// walked more often.
#[cfg(target_os = "linux")]
pub fn watch(roots: Vec<Root>, journal: std::sync::Arc<Journal>) -> Option<Watch> {
    inotify::watch(roots, journal)
}

/// A folder memory indexes, under the one it's in.
#[cfg(not(target_os = "linux"))]
pub struct Root {
    pub base: std::path::PathBuf,
    pub top: &'static str,
}

/// No watch on this system: what changes is found by a walk.
#[cfg(not(target_os = "linux"))]
pub struct Watch;

#[cfg(not(target_os = "linux"))]
pub fn watch(_roots: Vec<Root>, journal: std::sync::Arc<Journal>) -> Option<Watch> {
    journal.watching(Some(
        "this system's folders aren't watched yet: what changes is found by a walk each minute"
            .into(),
    ));
    None
}

//! The ids a receiver has taken ([`Seen`]), kept so a replay inside its
//! window is refused, and the store kept in memory ([`SeenInMemory`]).

use super::*;

/// The longest any scheme keeps an id, 30 days (GitHub's, which signs no
/// time): what a store handed only an id, not its window, keeps it for (a
/// connection's `sift` answers with the id alone), so its caller's store and
/// every other keep the same.
pub const LONGEST_KEEP: u64 = 30 * 86_400;

/// The ids a receiver has taken, each kept until its window ends: a
/// replay inside it is refused. Behind it, the receiver's own store (a
/// database it already keeps; memory for one that restarts with its
/// sender).
pub trait Seen: Send + Sync {
    /// Whether `id` is new, keeping it until `until` (seconds since 1970)
    /// if so, `now`. An error when the store can't say.
    fn first<'a>(
        &'a self,
        id: &'a str,
        until: u64,
        now: u64,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<bool>> + Send + 'a>>;

    /// Whether what it keeps outlives a restart (a database), as a scheme
    /// that signs no time needs.
    fn durable(&self) -> bool;
}

/// A [`Seen`] in memory: for a receiver whose replays matter only while it
/// runs, and so never for a scheme that signs no time. What's past its
/// time is dropped as new ids come.
#[derive(Debug, Default)]
pub struct SeenInMemory(Mutex<HashMap<String, u64>>);

impl Seen for SeenInMemory {
    fn first<'a>(
        &'a self,
        id: &'a str,
        until: u64,
        now: u64,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<bool>> + Send + 'a>> {
        let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        kept.retain(|_, until| *until >= now);
        let first = !kept.contains_key(id);
        if first {
            kept.insert(id.to_string(), until);
        }
        Box::pin(std::future::ready(Ok(first)))
    }

    fn durable(&self) -> bool {
        false
    }
}

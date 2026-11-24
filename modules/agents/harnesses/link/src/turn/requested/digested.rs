//! Each conversation's kept digests ([`model::Digests`]), so its next
//! request digests only what it adds. Derived: a process without them
//! makes each again, once.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::agent::{Agent, Thread};
use crate::model::Digests;

/// The most conversations whose digests are kept: past it, the one asked
/// longest ago is forgotten, and its next request digests its window
/// again.
const KEPT_MAX: usize = 16;

/// The conversations' digests, and the tick each use counts.
#[derive(Default)]
pub(in crate::turn) struct Digested {
    held: Mutex<(u64, Conversations)>,
}

/// Each conversation's digests, and when it last used them.
type Conversations = HashMap<Key, (u64, Digests)>;

/// A conversation, by its agent's owner and ID and its thread.
type Key = (String, String, Thread);

impl Digested {
    /// `use_them` given `thread`'s digests, kept with what it found or
    /// made once it returns. Taken out meanwhile, so another conversation
    /// waits for none of it.
    pub(in crate::turn) fn with<T>(
        &self,
        agent: &Agent,
        thread: &Thread,
        use_them: impl FnOnce(&mut Digests) -> T,
    ) -> T {
        let key = (
            agent.owner().to_string(),
            agent.id().to_string(),
            thread.clone(),
        );
        let taken = self
            .held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .remove(&key);
        let mut digests = taken.map(|(_, digests)| digests).unwrap_or_default();
        let used = use_them(&mut digests);
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let (tick, conversations) = &mut *held;
        *tick += 1;
        conversations.insert(key, (*tick, digests));
        while conversations.len() > KEPT_MAX {
            let Some(oldest) = conversations
                .iter()
                .min_by_key(|(_, (used, _))| *used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            conversations.remove(&oldest);
        }
        used
    }
}

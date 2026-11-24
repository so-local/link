//! What this process last read of each conversation kept compact or by
//! a store driver, so the next read asks for only what was added since
//! its last event, as [`super::seen`] does for a log. Derived: a process
//! without it reads from the window's start, and one that finds the
//! last event it read changed (the conversation moved, or begun again)
//! reads again.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::event::Event;

/// The most conversations remembered: past it, the one read longest ago
/// is forgotten.
const KEPT_MAX: usize = 64;

/// A conversation, by how it's kept, its agent's owner and ID, and its
/// thread.
pub(super) type Key = (String, String, String, String);

/// The conversations a process has read, by [`Key`].
#[derive(Default)]
pub(super) struct Recent {
    held: Mutex<(u64, HashMap<Key, Kept>)>,
    /// The events read anew, all told ([`Recent::fetched`]).
    fetched: AtomicU64,
}

struct Kept {
    events: Vec<Event>,
    used: u64,
}

impl Recent {
    /// The events numbered `first` and on, `read` reading those numbered
    /// its argument and on: from the last event held, when what's held
    /// reaches back to `first` and that event is still there as it was;
    /// else from `first`.
    pub(super) fn read(
        &self,
        key: Key,
        first: u64,
        mut read: impl FnMut(u64) -> anyhow::Result<Vec<Event>>,
    ) -> anyhow::Result<Vec<Event>> {
        let first = first.max(1);
        let held = self
            .held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .remove(&key)
            .filter(|kept| kept.events.first().is_some_and(|e| e.seq <= first));
        let mut read = |from: u64| {
            let events = read(from)?;
            self.fetched
                .fetch_add(events.len() as u64, Ordering::Relaxed);
            anyhow::Ok(events)
        };
        let events = match held {
            Some(mut kept) => {
                let last = kept.events.last().cloned();
                let newer = read(last.as_ref().map_or(first, |e| e.seq))?;
                match newer.first() == last.as_ref() {
                    true => {
                        // The last held, read again to see it's as it was.
                        self.fetched.fetch_sub(1, Ordering::Relaxed);
                        kept.events.extend(newer.into_iter().skip(1));
                        kept.events
                    }
                    false => read(first)?,
                }
            }
            None => read(first)?,
        };
        let from = events.partition_point(|e| e.seq < first);
        let asked = events[from..].to_vec();
        self.keep(key, events);
        Ok(asked)
    }

    /// How many events reads took from the conversations anew, all told:
    /// each past the last one held, or every one read where what was held
    /// didn't reach back far enough or had changed. The last event held,
    /// read again to see it's still there as it was, isn't counted.
    pub(super) fn fetched(&self) -> u64 {
        self.fetched.load(Ordering::Relaxed)
    }

    /// Keeps `events` for the next read: only those from the first a turn
    /// reads by the latest window mark among them.
    fn keep(&self, key: Key, mut events: Vec<Event>) {
        events.drain(..super::seen::before_window(&events));
        if events.is_empty() {
            return;
        }
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let (tick, conversations) = &mut *held;
        *tick += 1;
        conversations.insert(
            key,
            Kept {
                events,
                used: *tick,
            },
        );
        while conversations.len() > KEPT_MAX {
            let Some(oldest) = conversations
                .iter()
                .min_by_key(|(_, kept)| kept.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            conversations.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn conversation(n: u64, from: &str) -> Vec<Event> {
        (1..=n)
            .map(|seq| Event {
                seq,
                ..Event::user(&format!("{from} {seq}"))
            })
            .collect()
    }

    /// Read again, only the events after the last one held are asked
    /// for; a conversation begun again in its place is read whole.
    #[test]
    fn a_read_asks_only_for_what_was_added() {
        let recent = Recent::default();
        let key: Key = ("compact".into(), "o".into(), "main".into(), "t".into());
        let log = RefCell::new(conversation(1000, "first"));
        let returned = RefCell::new(0usize);
        let read = |from: u64| {
            let events: Vec<Event> = log
                .borrow()
                .iter()
                .filter(|e| e.seq >= from)
                .cloned()
                .collect();
            *returned.borrow_mut() += events.len();
            Ok(events)
        };
        assert_eq!(recent.read(key.clone(), 1, read).unwrap().len(), 1000);
        *returned.borrow_mut() = 0;
        assert_eq!(recent.read(key.clone(), 990, read).unwrap().len(), 11);
        assert_eq!(
            *returned.borrow(),
            1,
            "it asked for more than the last event"
        );
        log.borrow_mut()
            .extend(conversation(1002, "first").into_iter().skip(1000));
        *returned.borrow_mut() = 0;
        assert_eq!(recent.read(key.clone(), 1001, read).unwrap().len(), 2);
        assert_eq!(*returned.borrow(), 3);
        // Begun again: its last event isn't the one held.
        *log.borrow_mut() = conversation(1002, "again");
        let events = recent.read(key, 1, read).unwrap();
        assert_eq!(events[0].text, "again 1");
        assert_eq!(events.len(), 1002);
    }
}

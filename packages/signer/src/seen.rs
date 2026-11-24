//! A replay store: what was taken, each until it would be refused anyway.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::{Refused, limits};

/// The IDs taken, each kept until it ends, so a statement is spent once.
/// One store a process, shared by every clone of what checks with it.
#[derive(Debug, Default)]
pub struct Seen {
    taken: Mutex<HashMap<String, u64>>,
}

impl Seen {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes `jti`, good until `exp`, at `now`: refused if it was taken
    /// before and hasn't ended, or if the store is full of ones that
    /// haven't.
    pub fn take(&self, jti: &str, exp: u64, now: u64) -> Result<(), Refused> {
        let mut taken = self.taken.lock().unwrap_or_else(|e| e.into_inner());
        if taken.get(jti).is_some_and(|until| *until >= now) {
            return Err(Refused::Spent);
        }
        if taken.len() >= limits().seen_max {
            taken.retain(|_, until| *until >= now);
            if taken.len() >= limits().seen_max {
                return Err(Refused::Full);
            }
        }
        taken.insert(jti.to_string(), exp);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_taken_once_until_it_ends() {
        let seen = Seen::new();
        assert_eq!(seen.take("a", 100, 50), Ok(()));
        assert_eq!(seen.take("a", 100, 60), Err(Refused::Spent));
        assert_eq!(seen.take("b", 100, 60), Ok(()));
        // Past its end it would be refused as ended anyway; its slot is free.
        assert_eq!(seen.take("a", 200, 101), Ok(()));
    }
}

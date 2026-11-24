//! The reservation contract: how an owner of IDs (the agent module's
//! agents, the secret module's secrets) makes one ID never mean two
//! things. Under its own lock, it reserves a candidate that is neither
//! live nor retired, and on delete retires the ID, which it keeps.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::candidate::{NoRandomness, candidate};
use crate::limits::limits;

/// What an owner keeps of the IDs it gave: each live while what it
/// names exists, then retired, never given again. Its owner holds its
/// own lock across a reservation or a retirement and the write that
/// keeps it, so two never interleave.
pub trait Ledger {
    /// Whether `id` names something that exists.
    fn is_live(&self, id: &str) -> bool;
    /// Whether `id` named something since deleted.
    fn is_retired(&self, id: &str) -> bool;
    /// Notes `id` live.
    fn hold(&mut self, id: &str);
    /// Notes `id` retired: live no more, never live again.
    fn retire(&mut self, id: &str);
}

/// Why an ID can't be held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// It names something that exists.
    Live,
    /// It named something since deleted.
    Retired,
    /// Every candidate drawn was refused: the ledger is broken.
    Exhausted,
    /// No candidate could be drawn.
    NoRandomness,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Live => "that ID names something that exists",
            Self::Retired => "that ID named something since deleted, and is never given again",
            Self::Exhausted => "no candidate ID was free: its owner's record is broken",
            Self::NoRandomness => "no randomness to draw a new ID from",
        })
    }
}

impl std::error::Error for Refused {}

impl From<NoRandomness> for Refused {
    fn from(_: NoRandomness) -> Self {
        Self::NoRandomness
    }
}

/// Holds `id`, from elsewhere (an arrival, a copy), unless it's live or
/// retired here.
pub fn admit(ledger: &mut impl Ledger, id: &str) -> Result<(), Refused> {
    if ledger.is_retired(id) {
        return Err(Refused::Retired);
    }
    if ledger.is_live(id) {
        return Err(Refused::Live);
    }
    ledger.hold(id);
    Ok(())
}

/// A new ID, held: the first [`candidate`] neither live nor retired.
pub fn reserve(ledger: &mut impl Ledger) -> Result<String, Refused> {
    reserve_from(ledger, candidate)
}

/// [`reserve`], drawing from `draw`: the first of its candidates that
/// [`admit`] holds, within `limits.toml`'s `reserve_attempts`.
pub fn reserve_from(
    ledger: &mut impl Ledger,
    mut draw: impl FnMut() -> Result<String, NoRandomness>,
) -> Result<String, Refused> {
    for _ in 0..limits().reserve_attempts {
        let id = draw()?;
        match admit(ledger, &id) {
            Ok(()) => return Ok(id),
            Err(Refused::Live | Refused::Retired) => continue,
            Err(other) => return Err(other),
        }
    }
    Err(Refused::Exhausted)
}

/// A ledger as two sets, kept as its owner's state: what's live and
/// what's retired.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub live: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub retired: BTreeSet<String>,
}

impl Table {
    /// Takes `id` back from retired for what it named, brought back whole
    /// (an agent restored from its backup): the one way a retired ID is
    /// live again, never for anything new, which [`reserve`] and
    /// [`admit`] refuse it to.
    pub fn revive(&mut self, id: &str) {
        self.retired.remove(id);
        self.live.insert(id.to_string());
    }
}

impl Ledger for Table {
    fn is_live(&self, id: &str) -> bool {
        self.live.contains(id)
    }
    fn is_retired(&self, id: &str) -> bool {
        self.retired.contains(id)
    }
    fn hold(&mut self, id: &str) {
        self.live.insert(id.to_string());
    }
    fn retire(&mut self, id: &str) {
        self.live.remove(id);
        self.retired.insert(id.to_string());
    }
}

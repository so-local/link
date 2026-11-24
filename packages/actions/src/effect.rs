//! An action's effect: what it does to state, one word of four. Its verb
//! names it, beside the verb's meaning, and every surface derives from it
//! what it says of the action: the command line how it runs, a tool the
//! hints a model reads, and a transport its method.

use serde::{Deserialize, Serialize};

/// What an action does to state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// Reads, changing nothing anywhere, so calling it twice is calling
    /// it once.
    Find,
    /// Makes a new thing.
    Insert,
    /// Changes a thing that exists, or sets it.
    Update,
    /// Removes a thing.
    Delete,
}

impl Effect {
    pub const ALL: [Effect; 4] = [Self::Find, Self::Insert, Self::Update, Self::Delete];

    /// Its word, as a declaration writes it.
    pub fn word(self) -> &'static str {
        match self {
            Self::Find => "find",
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }

    /// Whether it only reads: MCP's `readOnlyHint`.
    pub fn reads_only(self) -> bool {
        self == Self::Find
    }

    /// Whether it may undo or overwrite what was there: MCP's
    /// `destructiveHint`, said only of an effect that doesn't only read.
    pub fn destructive(self) -> Option<bool> {
        (!self.reads_only()).then_some(matches!(self, Self::Update | Self::Delete))
    }

    /// Whether calling it again with the same arguments changes nothing
    /// more: MCP's `idempotentHint`, said only of an effect that doesn't
    /// only read.
    pub fn idempotent(self) -> Option<bool> {
        (!self.reads_only()).then_some(self != Self::Insert)
    }
}

impl std::fmt::Display for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.word())
    }
}

#[cfg(test)]
mod tests {
    use super::Effect;

    #[test]
    fn words_round_trip_and_hints_follow_the_effect() {
        for effect in Effect::ALL {
            let word = serde_json::to_value(effect).unwrap();
            assert_eq!(word, effect.word());
            assert_eq!(serde_json::from_value::<Effect>(word).unwrap(), effect);
        }
        assert_eq!(Effect::Find.destructive(), None);
        assert_eq!(Effect::Insert.destructive(), Some(false));
        assert_eq!(Effect::Insert.idempotent(), Some(false));
        assert_eq!(Effect::Delete.destructive(), Some(true));
        assert_eq!(Effect::Update.idempotent(), Some(true));
    }
}

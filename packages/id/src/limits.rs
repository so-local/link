//! link-id's limits, read once from `limits.toml`.

use std::sync::LazyLock;

use serde::Deserialize;

/// link-id's limits, as `limits.toml` sets them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// How long a new ID's candidate is.
    pub candidate_length: usize,
    /// What a candidate is made of, each byte as likely as another.
    pub candidate_alphabet: String,
    /// How many candidates [`crate::reserve`] draws before it gives up.
    pub reserve_attempts: usize,
    /// How many parents an execution may have above it.
    pub parent_depth: usize,
}

/// link-id's limits.
pub fn limits() -> &'static Limits {
    static LIMITS: LazyLock<Limits> = LazyLock::new(|| {
        let limits: Limits = toml::from_str(include_str!("limits.toml")).expect("link-id's limits");
        limits.check().expect("link-id's limits");
        limits
    });
    &LIMITS
}

impl Limits {
    fn check(&self) -> Result<(), String> {
        let alphabet = self.candidate_alphabet.as_bytes();
        let mut sorted = alphabet.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        if alphabet.len() < 2 || sorted.len() != alphabet.len() || alphabet.len() > 128 {
            return Err("the candidate alphabet is 2 to 128 different bytes".into());
        }
        if !alphabet
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return Err("the candidate alphabet is lowercase letters and digits".into());
        }
        if !(8..=64).contains(&self.candidate_length) {
            return Err("a candidate is 8 to 64 bytes".into());
        }
        if self.reserve_attempts == 0 || self.parent_depth == 0 {
            return Err("reserving draws, and an execution may have a parent".into());
        }
        Ok(())
    }
}

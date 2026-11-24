//! The one candidate generator: a new ID's id, drawn at random, which
//! its owner reserves ([`crate::reserve`]) before it names anything.

use ring::rand::SecureRandom;

use crate::limits::limits;

/// The system's randomness failed, so no candidate was drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoRandomness;

impl std::fmt::Display for NoRandomness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no randomness to draw a new ID from")
    }
}

impl std::error::Error for NoRandomness {}

/// A new ID's candidate: `limits.toml`'s `candidate_length` bytes of its
/// `candidate_alphabet`, none likelier than another. Random, so two
/// owners' rarely meet, but only a reservation makes one an ID.
pub fn candidate() -> Result<String, NoRandomness> {
    let limits = limits();
    let alphabet = limits.candidate_alphabet.as_bytes();
    // The largest multiple of the alphabet's size a byte holds: a byte
    // above it is drawn again, so no byte of the alphabet is likelier.
    let fair = 256 - 256 % alphabet.len();
    let rng = ring::rand::SystemRandom::new();
    let mut id = String::with_capacity(limits.candidate_length);
    while id.len() < limits.candidate_length {
        let mut byte = [0u8];
        rng.fill(&mut byte).map_err(|_| NoRandomness)?;
        if usize::from(byte[0]) < fair {
            id.push(char::from(alphabet[usize::from(byte[0]) % alphabet.len()]));
        }
    }
    Ok(id)
}

/// Whether `id` is one [`candidate`] could have drawn.
pub fn is_candidate(id: &str) -> bool {
    let limits = limits();
    id.len() == limits.candidate_length
        && id
            .bytes()
            .all(|b| limits.candidate_alphabet.as_bytes().contains(&b))
}

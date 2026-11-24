//! Each message's digests, made once. A model request's evidence names
//! what it sends by SHA-256 ([`ContentDigest`]): each message, call,
//! answer and image, and where each event's words went. A conversation's
//! next request sends again nearly all its last one did, so a digest
//! made is kept, found again by the length of the bytes it's of and a
//! keyed hash of them ([`Seen`]), never the bytes themselves: what's kept
//! is about the digests alone, whatever the size of what they're of,
//! images included. What a request no longer sends is forgotten at the
//! next ([`Digests::sweep`]), so what's kept is about one window's
//! digests.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};

use link_files::reading::ContentDigest;

/// Content as it's found again: its length and a 128-bit hash of it,
/// SipHash keyed by this process alone, so no sender can make two
/// contents meet. Two contents meeting by chance would take about 2^64
/// of them.
type Seen = (usize, u64, u64);

/// The digests a conversation's requests made, by what they're of.
pub struct Digests {
    /// The hash's seed: random, this process's own.
    seed: std::hash::RandomState,
    /// Each kept, by what it's of.
    kept: HashMap<Seen, Kept>,
    /// How many were made: each a SHA-256 of content not kept.
    made: u64,
}

impl Default for Digests {
    fn default() -> Self {
        Self {
            seed: KEY.clone(),
            kept: HashMap::new(),
            made: 0,
        }
    }
}

/// The key every conversation's digests are found by, made once.
static KEY: std::sync::LazyLock<std::hash::RandomState> =
    std::sync::LazyLock::new(std::hash::RandomState::new);

/// A digest, and whether a request used it since the last sweep.
struct Kept {
    digest: ContentDigest,
    used: bool,
}

impl Digests {
    /// The digest of `bytes`: the one kept, else made and kept.
    pub fn of(&mut self, bytes: &[u8]) -> ContentDigest {
        let seen = self.seen(bytes);
        if let Some(kept) = self.kept.get_mut(&seen) {
            kept.used = true;
            return kept.digest.clone();
        }
        let digest = ContentDigest::of(bytes);
        self.made += 1;
        self.kept.insert(
            seen,
            Kept {
                digest: digest.clone(),
                used: true,
            },
        );
        digest
    }

    /// Whether `digest` is of `bytes` ([`ContentDigest::matches`]).
    pub fn matches(&mut self, digest: &ContentDigest, bytes: &[u8]) -> bool {
        digest.bytes == bytes.len() as u64 && self.of(bytes) == *digest
    }

    /// Forgets each digest no request used since the last sweep: a
    /// conversation sweeps before each request, so it keeps what the one
    /// before sent.
    pub fn sweep(&mut self) {
        self.kept.retain(|_, kept| std::mem::take(&mut kept.used));
    }

    /// How many digests were made, each of content not kept.
    pub fn made(&self) -> u64 {
        self.made
    }

    /// `bytes` as they're found again: their length and their keyed hash,
    /// read once, its two halves the hash's state finished twice.
    fn seen(&self, bytes: &[u8]) -> Seen {
        let mut hasher = self.seed.build_hasher();
        hasher.write(bytes);
        let first = hasher.finish();
        hasher.write_u8(0xff);
        (bytes.len(), first, hasher.finish())
    }

    /// The bytes it holds: its digests and what finds them, never what
    /// they're of.
    #[cfg(test)]
    fn held(&self) -> usize {
        let each = std::mem::size_of::<(Seen, Kept)>();
        self.kept.capacity() * each
            + self
                .kept
                .values()
                .map(|kept| kept.digest.sha256.capacity())
                .sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Content's digest is made once while requests use it, and again
    /// only once a request went without it.
    #[test]
    fn a_digest_is_made_once_while_requests_use_it() {
        let mut digests = Digests::default();
        let long = "x".repeat(192);
        let same_ends = format!("{}{}", &long[..64], "y".repeat(64)) + &long[..64];
        assert_eq!(
            digests.of(long.as_bytes()),
            ContentDigest::of(long.as_bytes())
        );
        assert_eq!(
            digests.of(long.as_bytes()),
            ContentDigest::of(long.as_bytes())
        );
        // Sampled the same, and not: compared whole.
        assert_eq!(
            digests.of(same_ends.as_bytes()),
            ContentDigest::of(same_ends.as_bytes())
        );
        assert_eq!(digests.made(), 2);
        assert!(digests.matches(&ContentDigest::of(b"a"), b"a"));
        assert!(!digests.matches(&ContentDigest::of(b"a"), b"b"));
        assert_eq!(digests.made(), 4);
        digests.sweep();
        digests.of(long.as_bytes());
        digests.sweep();
        // Used since the sweep before: kept; the rest made again.
        digests.of(long.as_bytes());
        digests.of(b"a");
        assert_eq!(digests.made(), 5);
    }

    /// What's kept is the digests, never the bytes they're of: a
    /// conversation that sent megabytes of images holds what one that sent
    /// a word holds.
    #[test]
    fn digests_are_kept_without_what_theyre_of() {
        let mut small = Digests::default();
        let mut large = Digests::default();
        for n in 0..8_u8 {
            small.of(&[n]);
            large.of(&vec![n; 1 << 20]);
        }
        assert_eq!(large.held(), small.held());
        assert!(large.held() < 8 << 10, "{}", large.held());
        // Found again, still: a request sending them again makes none.
        large.sweep();
        for n in 0..8_u8 {
            assert_eq!(
                large.of(&vec![n; 1 << 20]),
                ContentDigest::of(&vec![n; 1 << 20])
            );
        }
        assert_eq!(large.made(), 8);
    }
}

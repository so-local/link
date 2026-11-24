//! A signer and its verifier (`docs/more/libraries/security/signer.md`).
//!
//! - A [`Signer`] is the private half this machine keeps and signs with,
//!   which never leaves it; its [`Verifier`] is the public half another
//!   machine holds, named by its [`Fingerprint`].
//! - Every Link credential is a compact JWS, EdDSA, made and checked by
//!   [`jws`] alone: its header says what it is (`typ`) and which verifier
//!   checks it (`kid`, a fingerprint), and its claims are read only once
//!   its signature has passed.
//! - A [`signature`] is the short statement a signer makes for one
//!   exchange, for one party, spent once within a minute ([`Seen`]).
//!
//! It knows nothing of tokens or directories, and names no principal
//! itself: what a signature signs for is whatever its caller names.

mod declared {
    //! link-signer's shapes, generated from its declarations
    //! (`types.toml`).
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}
pub mod jws;
mod limits;
mod protocol;
mod seen;
pub mod signature;
mod signer;
mod verifier;

pub use declared::{Header, HeaderAlg, SignatureClaims};
pub use jws::{Refused, Unverified, Verified};
pub use limits::{Limits, limits};
pub use protocol::Protocol;
pub use seen::Seen;
pub use signer::Signer;
pub use verifier::{Fingerprint, NoVerifier, Verifier, Verifiers};

/// Unix seconds, now.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A new ID of 128 random bits, as 32 lowercase hex digits: a signature's,
/// a token's or a credential's `jti`.
pub fn jti() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .expect("the system's random source");
    link_digest::hex(&bytes)
}

#[cfg(test)]
link_test_kit::program!(unit);

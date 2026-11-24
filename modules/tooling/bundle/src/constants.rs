//! `lnk bundle`'s constants.

/// How many of a commit's hash characters a build ID carries.
pub const HASH_CHARS: usize = 12;

/// Link's signer: the signer Link's public release is compiled to trust
/// and sealed by, its public line in the public source's
/// `scripts/release/link-signers`.
pub const LINK: &str = "link";

/// Link's staging signer, its public line in the public source's
/// `scripts/release/staging-signers`.
pub const STAGING: &str = "staging";

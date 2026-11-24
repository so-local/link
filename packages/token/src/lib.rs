//! Link's one token (`docs/more/libraries/security/token.md`).
//!
//! - An access [`token`] is a compact JWS, `typ` `at+jwt`, in one shape
//!   whoever holds it: its issuer, its principal, its kind of client
//!   ([`Client`]), its audience, the wake it acts in if one, and its
//!   minutes. It lists no actions, no resources, no project, and no
//!   owner. An issuer issues it ([`Mint`]); a resource server verifies it
//!   with the verifiers of the issuers it trusts ([`Trust`], [`Issuers`]),
//!   never asking one, and decides from the assignments it reads.
//! - A [`refresh`] token is issued only by a login and traded at its issuer
//!   alone, only ever for access tokens.
//! - A random [`secret`] (a client secret, an API key, or a one-time
//!   code's machine half) is its prefix, a random body, and a checksum;
//!   its taker keeps only its hash.
//! - Every secret opens with its type's prefix, declared once
//!   ([`prefixes`]).
//!
//! Every signature is made and checked through `link-signer`, and every ID
//! a token carries is a `link-id`.

mod declared {
    //! link-token's shapes, generated from its declarations (`types.toml`).
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}
mod limits;
mod prefixes;
pub mod refresh;
pub mod secret;
pub mod token;

pub use declared::{Claims, Client, Place, RefreshClaims};
pub use limits::{Limits, limits};
pub use link_id::{Resource, Workload};
pub use link_principal::{Principal, PrincipalKind};
pub use prefixes::{Prefixes, prefixes};
pub use token::{Ask, Checked, Issuers, Mint, Refused, Trust};

/// Whether `name` can be an access token's audience, as
/// `types.toml` declares it.
pub fn is_audience(name: &str) -> bool {
    link_actions::value::Validate::validate(&declared::Audience(name.to_string()), "aud").is_ok()
}

#[cfg(test)]
link_test_kit::program!(unit);

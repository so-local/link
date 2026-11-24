//! The random secrets IAM makes, each its prefix, a random body, and a
//! checksum, so a secret scanner confirms a match offline: a client
//! secret, an API key, or a one-time code's machine half. Whoever takes
//! one keeps only its [`Hash`], never the secret.

use link_secretive::Secret;

use crate::{Refused, prefixes};

/// Which random secret, by its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// What a service or a box shows its issuer alone, for access tokens.
    ClientSecret,
    /// What a resource server issues to a service principal and accepts
    /// in place of an access token.
    ApiKey,
    /// A one-time code's machine half, spent once at its issuer for a
    /// refresh token.
    Code,
}

impl Kind {
    /// Its prefix, from `prefixes.toml`.
    pub fn prefix(self) -> &'static str {
        let prefixes = prefixes();
        match self {
            Self::ClientSecret => &prefixes.client_secret,
            Self::ApiKey => &prefixes.api_key,
            Self::Code => &prefixes.code,
        }
    }
}

/// The random body's length, in lowercase hex digits: 256 bits.
const BODY: usize = 64;
/// The checksum's length, in lowercase hex digits: 32 bits.
const CHECKSUM: usize = 8;

/// What a taker keeps of a random secret: the SHA-256 of the whole text,
/// as lowercase hex.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash(String);

impl Hash {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A hash read back from where it was kept: 64 lowercase hex digits.
    pub fn parse(text: &str) -> Result<Self, Refused> {
        match text.len() == 64 && is_hex(text) {
            true => Ok(Self(text.to_string())),
            false => Err(Refused::Claims("a secret's hash is 64 hex digits".into())),
        }
    }
}

fn is_hex(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn checksum(prefix: &str, body: &str) -> String {
    link_digest::sha256(format!("{prefix}{body}").as_bytes())[..CHECKSUM].to_string()
}

/// A new random secret of `kind`, and the hash its taker keeps.
pub fn mint(kind: Kind) -> (Secret, Hash) {
    let body = format!("{}{}", link_signer::jti(), link_signer::jti());
    let prefix = kind.prefix();
    let text = format!("{prefix}{body}{}", checksum(prefix, &body));
    let hash = Hash(link_digest::sha256(text.as_bytes()));
    (Secret::new(text), hash)
}

/// `text` as a random secret of `kind`: its prefix, a body, and a checksum
/// that matches, and the hash to look it up by. Refused, saying nothing of
/// what it held, when it's another type's or its checksum fails.
pub fn read(kind: Kind, text: &str) -> Result<Hash, Refused> {
    let malformed = || Refused::Jws(link_signer::Refused::Malformed);
    let prefix = kind.prefix();
    let rest = text.strip_prefix(prefix).ok_or_else(malformed)?;
    if rest.len() != BODY + CHECKSUM || !is_hex(rest) {
        return Err(malformed());
    }
    let (body, sum) = rest.split_at(BODY);
    if checksum(prefix, body) != sum {
        return Err(malformed());
    }
    Ok(Hash(link_digest::sha256(text.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_reads_back_to_the_hash_its_taker_keeps() {
        for kind in [Kind::ClientSecret, Kind::ApiKey, Kind::Code] {
            let (text, hash) = mint(kind);
            assert!(text.expose().starts_with(kind.prefix()));
            assert_eq!(read(kind, text.expose()).unwrap(), hash);
            assert_eq!(Hash::parse(hash.as_str()).unwrap(), hash);
            let (other, _) = mint(kind);
            assert_ne!(text.expose(), other.expose(), "random");
        }
    }

    #[test]
    fn a_secret_of_another_type_or_with_a_broken_checksum_is_refused() {
        let (text, _) = mint(Kind::ClientSecret);
        assert!(read(Kind::ApiKey, text.expose()).is_err());
        let mut typo = text.expose().to_string();
        let at = Kind::ClientSecret.prefix().len() + 3;
        let flipped = if &typo[at..=at] == "0" { "1" } else { "0" };
        typo.replace_range(at..=at, flipped);
        assert!(read(Kind::ClientSecret, &typo).is_err());
        assert!(
            read(
                Kind::ClientSecret,
                &text.expose()[..text.expose().len() - 1]
            )
            .is_err()
        );
    }
}

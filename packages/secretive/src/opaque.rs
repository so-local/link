//! [`OpaqueSecret`]: a secret Link uses and nobody reads.

use std::fmt;

use crate::{REDACTED, Secret, hmac};

/// A secret that only signs and checks (HMAC-SHA256): an agent's proof
/// secret, a binding's, a service's signing secret. There's no way to
/// its bytes, so it's never exposed, printed, copied or sent; it's only
/// [`crate::Sealed`], or kept where it's made ([`OpaqueSecret::keep`]).
pub struct OpaqueSecret(pub(crate) Secret<Vec<u8>>);

impl OpaqueSecret {
    /// A new one, of `len` random bytes, held as their hex (so it's
    /// kept as text, [`OpaqueSecret::keep`]).
    pub fn random(len: usize) -> std::io::Result<Self> {
        let mut bytes = Secret::new(vec![0u8; len]);
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes.0)
            .map_err(|_| std::io::Error::other("no randomness"))?;
        Ok(Self(
            bytes.expose_map(|bytes| hmac::hex(bytes).into_bytes()),
        ))
    }

    /// One given as text (a harness's endpoint secret, read where it's
    /// kept), taken over: from now on it only signs.
    pub fn from_text(secret: Secret) -> Self {
        Self(secret.expose_map(|text| text.as_bytes().to_vec()))
    }

    /// Taken over from bytes it was read or opened into.
    pub fn from_bytes(secret: Secret<Vec<u8>>) -> Self {
        Self(secret)
    }

    /// Whether it holds nothing: one that does signs nothing anyone
    /// should trust, and [`OpaqueSecret::verify`] refuses it.
    pub fn is_empty(&self) -> bool {
        self.0.0.is_empty()
    }

    /// How many bytes it holds (a random one's hex: twice its random
    /// bytes), for a check of its strength.
    pub fn len(&self) -> usize {
        self.0.0.len()
    }

    /// HMAC-SHA256 of `message` under it.
    pub fn sign(&self, message: &[u8]) -> [u8; 32] {
        hmac::sign(&self.0.0, message)
    }

    /// Whether `tag` is [`OpaqueSecret::sign`] of `message`, compared in
    /// constant time; never with an empty secret.
    pub fn verify(&self, message: &[u8], tag: &[u8]) -> bool {
        hmac::verify(&self.0.0, message, tag)
    }

    /// As text where it's kept: a file of mode 600 its own program reads
    /// back, or the keyring. None when its bytes aren't text: one opened
    /// from bytes is never changed to fit. Every use is
    /// reviewed.
    pub fn keep(&self) -> Option<Secret> {
        let text = std::str::from_utf8(&self.0.0).ok()?;
        Some(Secret::new(text.to_string()))
    }
}

impl fmt::Debug for OpaqueSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// Equal when their bytes are, in constant time.
impl PartialEq for OpaqueSecret {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for OpaqueSecret {}

/// Read from where it's kept, as text.
impl<'de> serde::Deserialize<'de> for OpaqueSecret {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Secret::deserialize(deserializer).map(Self::from_text)
    }
}

/// An [`OpaqueSecret`] written where it's kept, said so:
/// `#[serde(with = "link_secretive::opaque::kept")]`.
pub mod kept {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::OpaqueSecret;

    pub fn serialize<S: Serializer>(
        secret: &OpaqueSecret,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match secret.keep() {
            Some(text) => text.expose().serialize(serializer),
            None => Err(serde::ser::Error::custom(
                "an opaque secret whose bytes aren't text isn't kept as text",
            )),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<OpaqueSecret, D::Error> {
        OpaqueSecret::deserialize(deserializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static_assertions::assert_not_impl_any!(
        OpaqueSecret: Clone,
        serde::Serialize,
        AsRef<[u8]>,
        std::ops::Deref,
        fmt::Display
    );

    #[test]
    fn it_signs_and_checks_and_shows_nothing() {
        let secret = OpaqueSecret::random(32).unwrap();
        assert_eq!(secret.len(), 64);
        let tag = secret.sign(b"challenge");
        assert!(secret.verify(b"challenge", &tag));
        assert!(!secret.verify(b"another", &tag));
        assert!(!secret.verify(b"challenge", &tag[..31]));
        let other = OpaqueSecret::random(32).unwrap();
        assert!(!other.verify(b"challenge", &tag));
        assert_ne!(secret, other);
        assert_eq!(format!("{secret:?}"), "<redacted>");
    }

    #[test]
    fn an_empty_one_proves_nothing() {
        let empty = OpaqueSecret::from_text(Secret::new(String::new()));
        let tag = empty.sign(b"challenge");
        assert!(!empty.verify(b"challenge", &tag));
    }

    #[test]
    fn it_is_kept_as_its_text() {
        let given = OpaqueSecret::from_text(Secret::new("0123".to_string()));
        assert_eq!(given.keep().unwrap().expose(), "0123");
        let read: OpaqueSecret = serde_json::from_str(r#""0123""#).unwrap();
        assert_eq!(read, given);
    }

    #[test]
    fn bytes_that_arent_text_are_never_kept_changed() {
        let bytes = OpaqueSecret::from_bytes(Secret::new(vec![0xff, 0x00, 0xfe]));
        assert!(bytes.keep().is_none());
        #[derive(serde::Serialize)]
        struct Kept {
            #[serde(with = "crate::opaque::kept")]
            secret: OpaqueSecret,
        }
        assert!(serde_json::to_string(&Kept { secret: bytes }).is_err());
    }
}

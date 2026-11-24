//! [`SigningSecret`]: an Ed25519 key pair's private half, which only
//! signs.

use std::fmt;

use base64::Engine;

use crate::{REDACTED, Secret};

/// The private half of an Ed25519 key pair (RFC 8032), held as its PKCS
/// #8 document: a machine's key, which signs for its access tokens and
/// is persisted only through reviewed [`SigningSecret::keep`] calls.
/// It signs and says its public half; private bytes are held in a
/// [`Secret`] and are never printed or implicitly cloned.
pub struct SigningSecret(Secret<Vec<u8>>);

impl SigningSecret {
    /// A new pair, from the system's random source.
    pub fn generate() -> std::io::Result<Self> {
        let rng = ring::rand::SystemRandom::new();
        let document = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng)
            .map_err(|_| std::io::Error::other("no randomness for a key pair"))?;
        Ok(Self(Secret::new(document.as_ref().to_vec())))
    }

    /// One kept as [`SigningSecret::keep`] wrote it; none when it isn't
    /// one.
    pub fn from_kept(kept: &Secret) -> Option<Self> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(kept.expose().trim())
            .ok()
            .map(Secret::new)?;
        let pair = ring::signature::Ed25519KeyPair::from_pkcs8_maybe_unchecked(bytes.expose());
        pair.is_ok().then(|| Self(bytes))
    }

    /// Its pair, made for one use: ring's pair isn't zeroed when
    /// dropped, so none is kept.
    fn pair(&self) -> ring::signature::Ed25519KeyPair {
        ring::signature::Ed25519KeyPair::from_pkcs8_maybe_unchecked(self.0.expose())
            .expect("a signing secret is a key pair: checked when it was made or read")
    }

    /// The public half's 32 bytes.
    pub fn public(&self) -> [u8; 32] {
        use ring::signature::KeyPair;
        let mut out = [0u8; 32];
        out.copy_from_slice(self.pair().public_key().as_ref());
        out
    }

    /// The public half, base64url without padding: as the account
    /// service is given it.
    pub fn public_text(&self) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.public())
    }

    /// The Ed25519 signature of `message`.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        let mut out = [0u8; 64];
        out.copy_from_slice(self.pair().sign(message).as_ref());
        out
    }

    /// As text where it's kept (the Keychain, a file of mode 600 its own
    /// program reads back): its PKCS #8 document, base64url. Every use is
    /// reviewed.
    pub fn keep(&self) -> Secret {
        self.0
            .expose_map(|bytes| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
    }
}

impl fmt::Debug for SigningSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// Whether `signature` is `public`'s Ed25519 signature of `message`.
pub fn verify(public: &[u8], message: &[u8], signature: &[u8]) -> bool {
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public)
        .verify(message, signature)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    static_assertions::assert_not_impl_any!(
        SigningSecret: Clone,
        serde::Serialize,
        AsRef<[u8]>,
        std::ops::Deref,
        fmt::Display
    );

    #[test]
    fn it_signs_and_shows_only_its_public_half() {
        let secret = SigningSecret::generate().unwrap();
        let signature = secret.sign(b"assertion");
        assert!(verify(&secret.public(), b"assertion", &signature));
        assert!(!verify(&secret.public(), b"another", &signature));
        let other = SigningSecret::generate().unwrap();
        assert!(!verify(&other.public(), b"assertion", &signature));
        assert_eq!(secret.public_text().len(), 43);
        assert_eq!(format!("{secret:?}"), "<redacted>");
    }

    #[test]
    fn it_is_kept_and_read_back() {
        let secret = SigningSecret::generate().unwrap();
        let read = SigningSecret::from_kept(&secret.keep()).unwrap();
        assert_eq!(read.public(), secret.public());
        assert!(SigningSecret::from_kept(&Secret::new("not a key".into())).is_none());
        assert!(SigningSecret::from_kept(&Secret::new(String::new())).is_none());
    }
}

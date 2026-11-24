//! A signer: the private half this machine keeps and signs with.

use std::fmt;
use std::path::Path;

use link_secretive::{Secret, SigningSecret};

use crate::{Protocol, Verifier};

/// What this machine keeps to sign with, its private half, which never
/// leaves it. Another machine sees only its [`Verifier`].
pub struct Signer {
    protocol: Protocol,
    secret: SigningSecret,
    verifier: Verifier,
}

impl Signer {
    /// A new signer of `protocol`, from the system's random source.
    pub fn generate(protocol: Protocol) -> std::io::Result<Self> {
        let secret = SigningSecret::generate()?;
        Self::new(protocol, secret).map_err(std::io::Error::other)
    }

    /// One kept as [`Signer::keep`] wrote it.
    pub fn from_kept(protocol: Protocol, kept: &Secret) -> anyhow::Result<Self> {
        let secret = SigningSecret::from_kept(kept)
            .ok_or_else(|| anyhow::anyhow!("not a {protocol} signer as Link keeps one"))?;
        Self::new(protocol, secret)
    }

    /// One kept in the file at `path`, as [`Signer::keep`] wrote it.
    pub fn read(protocol: Protocol, path: &Path) -> anyhow::Result<Self> {
        let kept = link_secretive::read_secret(path)?;
        Self::from_kept(protocol, &kept).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
    }

    fn new(protocol: Protocol, secret: SigningSecret) -> anyhow::Result<Self> {
        let verifier = Verifier::ed25519(&secret.public())?;
        Ok(Self {
            protocol,
            secret,
            verifier,
        })
    }

    /// Its protocol.
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// Its public half.
    pub fn verifier(&self) -> Verifier {
        self.verifier.clone()
    }

    /// Its verifier's fingerprint, which names it.
    pub fn fingerprint(&self) -> &crate::Fingerprint {
        self.verifier.fingerprint()
    }

    /// Its signature of `message`.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.secret.sign(message)
    }

    /// As text where it's kept, a file only its program reads. Every use
    /// is reviewed.
    pub fn keep(&self) -> Secret {
        self.secret.keep()
    }
}

impl fmt::Debug for Signer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signer({} {})", self.protocol, self.fingerprint())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signer_is_kept_and_read_back() {
        let signer = Signer::generate(Protocol::Ssh).unwrap();
        let read = Signer::from_kept(Protocol::Ssh, &signer.keep()).unwrap();
        assert_eq!(read.verifier(), signer.verifier());
        let signature = signer.sign(b"message");
        assert!(read.verifier().verify(b"message", &signature));
        assert!(!read.verifier().verify(b"another", &signature));
        assert!(Signer::from_kept(Protocol::Ssh, &Secret::new("no".into())).is_err());
        assert_eq!(
            format!("{signer:?}"),
            format!("Signer(ssh {})", signer.fingerprint())
        );
    }
}

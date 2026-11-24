//! A verifier, a signer's public half, and its fingerprint.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Protocol;

/// OpenSSH's name for an Ed25519 key, the first field of its line and of
/// its wire blob.
const SSH_ED25519: &str = "ssh-ed25519";

/// What isn't a verifier, or a fingerprint, saying why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoVerifier(String);

impl fmt::Display for NoVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NoVerifier {}

/// A verifier's name, `SHA256:` and the unpadded base64 of the SHA-256 of
/// its SSH wire blob: what `ssh-keygen -l` prints for the same key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fingerprint(String);

impl Fingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Fingerprint {
    type Err = NoVerifier;
    fn from_str(text: &str) -> Result<Self, NoVerifier> {
        let digest = text
            .strip_prefix("SHA256:")
            .ok_or_else(|| NoVerifier(format!("{text:?} isn't SHA256:<digest>")))?;
        match STANDARD_NO_PAD.decode(digest) {
            Ok(bytes) if bytes.len() == 32 && digest.len() == 43 => Ok(Self(text.to_string())),
            _ => Err(NoVerifier(format!(
                "{text:?}'s digest isn't 32 bytes of base64"
            ))),
        }
    }
}

impl Serialize for Fingerprint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Fingerprint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A signer's public half, the only part another machine holds, which
/// checks what the signer signs. Only a real Ed25519 point is one: a
/// small-order or non-canonical one would check signatures anyone forged.
#[derive(Clone, PartialEq, Eq)]
pub struct Verifier {
    protocol: Protocol,
    public: [u8; 32],
    fingerprint: Fingerprint,
}

impl Verifier {
    /// The `ssh` verifier whose Ed25519 public half is `public`.
    pub fn ed25519(public: &[u8]) -> Result<Self, NoVerifier> {
        let public: [u8; 32] = public
            .try_into()
            .map_err(|_| NoVerifier("an Ed25519 verifier is 32 bytes".into()))?;
        if weak(&public) {
            return Err(NoVerifier(
                "no real Ed25519 key (a small-order or non-canonical point), which would take \
                 forged signatures"
                    .into(),
            ));
        }
        let digest = link_digest::sha256_bytes(&blob(&public));
        Ok(Self {
            protocol: Protocol::Ssh,
            public,
            fingerprint: Fingerprint(format!("SHA256:{}", STANDARD_NO_PAD.encode(digest))),
        })
    }

    /// One written as OpenSSH writes a public key, `ssh-ed25519 <blob>`
    /// and an optional comment, which is dropped.
    pub fn from_openssh(line: &str) -> Result<Self, NoVerifier> {
        let mut fields = line.split_whitespace();
        let (Some(SSH_ED25519), Some(blob_text)) = (fields.next(), fields.next()) else {
            return Err(NoVerifier("not an ssh-ed25519 public key line".into()));
        };
        let raw = STANDARD
            .decode(blob_text)
            .map_err(|_| NoVerifier("its key isn't base64".into()))?;
        let public = raw
            .strip_prefix(blob_prefix().as_slice())
            .ok_or_else(|| NoVerifier("its key isn't an ssh-ed25519 blob".into()))?;
        Self::ed25519(public)
    }

    /// As OpenSSH writes a public key, with no comment.
    pub fn to_openssh(&self) -> String {
        format!("{SSH_ED25519} {}", STANDARD.encode(blob(&self.public)))
    }

    /// Its protocol.
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// Its name.
    pub fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    /// Its Ed25519 public half.
    pub fn public(&self) -> &[u8; 32] {
        &self.public
    }

    /// Whether `signature` is its signer's of `message`.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        link_secretive::signing::verify(&self.public, message, signature)
    }
}

impl fmt::Debug for Verifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Verifier({} {})", self.protocol, self.fingerprint)
    }
}

impl Serialize for Verifier {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_openssh())
    }
}

impl<'de> Deserialize<'de> for Verifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_openssh(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// The verifiers a checker holds, each by its fingerprint, which a JWS's
/// `kid` names: found once, never tried in turn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verifiers(BTreeMap<Fingerprint, Verifier>);

impl Verifiers {
    /// None: every signature refused.
    pub fn none() -> Self {
        Self::default()
    }

    /// These and `verifier`.
    pub fn with(mut self, verifier: Verifier) -> Self {
        self.insert(verifier);
        self
    }

    pub fn insert(&mut self, verifier: Verifier) {
        self.0.insert(verifier.fingerprint.clone(), verifier);
    }

    /// The one `fingerprint` names, if held.
    pub fn get(&self, fingerprint: &str) -> Option<&Verifier> {
        self.0
            .iter()
            .find(|(name, _)| name.as_str() == fingerprint)
            .map(|(_, verifier)| verifier)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Verifier> {
        self.0.values()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<Verifier> for Verifiers {
    fn from_iter<I: IntoIterator<Item = Verifier>>(iter: I) -> Self {
        iter.into_iter().fold(Self::none(), Self::with)
    }
}

/// An SSH string's length prefix and bytes.
fn ssh_string(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).expect("an SSH string fits u32");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
}

/// What every ssh-ed25519 blob begins with: its key type and the length
/// of its 32 bytes.
fn blob_prefix() -> Vec<u8> {
    let mut out = Vec::new();
    ssh_string(&mut out, SSH_ED25519.as_bytes());
    out.extend_from_slice(&32u32.to_be_bytes());
    out
}

/// `public`'s SSH wire blob.
fn blob(public: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::new();
    ssh_string(&mut out, SSH_ED25519.as_bytes());
    ssh_string(&mut out, public);
    out
}

/// The encodings of Ed25519's small-order points, their sign bit cleared
/// (libsodium's blocklist): a signature against one can be forged for any
/// message.
const SMALL_ORDER: [[u8; 32]; 4] = [
    // 0 (order 4) and 1 (order 1) are made below; these are the rest.
    // Order 8
    [
        0x26, 0xe8, 0x95, 0x8f, 0xc2, 0xb2, 0x27, 0xb0, 0x45, 0xc3, 0xf4, 0x89, 0xf2, 0xef, 0x98,
        0xf0, 0xd5, 0xdf, 0xac, 0x05, 0xd3, 0xc6, 0x33, 0x39, 0xb1, 0x38, 0x02, 0x88, 0x6d, 0x53,
        0xfc, 0x05,
    ],
    // Order 8
    [
        0xc7, 0x17, 0x6a, 0x70, 0x3d, 0x4d, 0xd8, 0x4f, 0xba, 0x3c, 0x0b, 0x76, 0x0d, 0x10, 0x67,
        0x0f, 0x2a, 0x20, 0x53, 0xfa, 0x2c, 0x39, 0xcc, 0xc6, 0x4e, 0xc7, 0xfd, 0x77, 0x92, 0xac,
        0x03, 0x7a,
    ],
    // p - 1 (order 2)
    [
        0xec, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0x7f,
    ],
    // p + 1, that is 1 again, not canonical (order 1)
    [
        0xee, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0x7f,
    ],
];

/// Whether `key` is a point no real key is: of small order, or with a `y`
/// of `p` or more (not canonical, which covers `p` itself, 0 again).
fn weak(key: &[u8; 32]) -> bool {
    let mut y = *key;
    y[31] &= 0x7f;
    let zero = [0u8; 32];
    let mut one = [0u8; 32];
    one[0] = 1;
    if y == zero || y == one || SMALL_ORDER.contains(&y) {
        return true;
    }
    // y >= p = 2^255 - 19: the top 31 bytes all ones, the first at least 0xed.
    y[31] == 0x7f && y[1..31].iter().all(|b| *b == 0xff) && y[0] >= 0xed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Signer;

    #[test]
    fn a_verifier_is_written_as_openssh_writes_it() {
        let signer = Signer::generate(Protocol::Ssh).unwrap();
        let verifier = signer.verifier();
        let line = verifier.to_openssh();
        assert!(
            line.starts_with("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI"),
            "{line}"
        );
        let read = Verifier::from_openssh(&format!("{line} someone@machine")).unwrap();
        assert_eq!(read, verifier);
        assert_eq!(read.fingerprint().as_str().len(), "SHA256:".len() + 43);
        assert_eq!(
            read.fingerprint().as_str().parse::<Fingerprint>().unwrap(),
            *verifier.fingerprint()
        );
        for not in [
            "",
            "ssh-rsa AAAA",
            "ssh-ed25519",
            "ssh-ed25519 !!!",
            "ssh-ed25519 AAAA",
        ] {
            assert!(Verifier::from_openssh(not).is_err(), "{not}");
        }
    }

    /// OpenSSH's own fingerprint of a known key, as `ssh-keygen -l` prints it.
    #[test]
    fn its_fingerprint_is_openssh_s() {
        // RFC 8032's first test key's public half.
        let public: [u8; 32] = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ];
        let verifier = Verifier::ed25519(&public).unwrap();
        let digest = link_digest::sha256_bytes(&blob(&public));
        assert_eq!(
            verifier.fingerprint().as_str(),
            format!("SHA256:{}", STANDARD_NO_PAD.encode(digest))
        );
        assert_eq!(
            verifier.to_openssh(),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINdamAGCsQq31Uv+08lkBzoO4XLz2qYjJa8CGmj3B1Ea"
        );
    }

    #[test]
    fn no_small_order_or_non_canonical_point_is_a_verifier() {
        let mut points = SMALL_ORDER.to_vec();
        points.push([0; 32]);
        let mut one = [0u8; 32];
        one[0] = 1;
        points.push(one);
        let mut p = [0xffu8; 32];
        p[0] = 0xed;
        p[31] = 0x7f;
        points.push(p);
        for point in points {
            for sign in [0u8, 0x80] {
                let mut key = point;
                key[31] |= sign;
                assert!(Verifier::ed25519(&key).is_err(), "{key:?}");
            }
        }
        assert!(Verifier::ed25519(&[1; 31]).is_err());
    }

    #[test]
    fn verifiers_are_found_by_fingerprint() {
        let a = Signer::generate(Protocol::Ssh).unwrap().verifier();
        let b = Signer::generate(Protocol::Ssh).unwrap().verifier();
        let held: Verifiers = [a.clone()].into_iter().collect();
        assert_eq!(held.get(a.fingerprint().as_str()), Some(&a));
        assert_eq!(held.get(b.fingerprint().as_str()), None);
        assert!(Verifiers::none().is_empty());
    }
}

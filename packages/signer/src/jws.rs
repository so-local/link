//! A compact JWS, `<header>.<claims>.<signature>`, each part base64url,
//! EdDSA alone: how every Link credential is made and checked.
//!
//! A JWS is parsed into an [`Unverified`], whose header alone may be
//! read, to find the verifier its `kid` names; only once its signature
//! has passed is it [`Verified`], and only then are its claims read.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use link_actions::value::Validate;
use link_secretive::Secret;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::declared::{Header, HeaderAlg};
use crate::{Signer, Verifier, Verifiers, limits};

/// Why a JWS, or a statement in one, was refused. Says nothing of what
/// it held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not a compact JWS, or longer than any real one.
    Malformed,
    /// Its header isn't EdDSA's, or holds anything else.
    Header,
    /// Its `typ` isn't the one asked for.
    Kind,
    /// Its `kid` names no verifier held.
    UnknownVerifier,
    /// Its signature isn't its verifier's.
    Signature,
    /// Its claims aren't the shape asked for, saying where.
    Claims(String),
    /// For another party.
    Audience,
    /// Ended, or made further ahead than clocks drift.
    Time,
    /// Lives longer than its kind may.
    TooLong,
    /// Taken before.
    Spent,
    /// The replay store is full of statements that haven't ended.
    Full,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => f.write_str("not a compact JWS"),
            Self::Header => f.write_str("its header isn't EdDSA, typ and kid alone"),
            Self::Kind => f.write_str("not the kind of statement asked for (typ)"),
            Self::UnknownVerifier => f.write_str("signed by no verifier held (kid)"),
            Self::Signature => f.write_str("its signature doesn't check"),
            Self::Claims(why) => write!(f, "its claims: {why}"),
            Self::Audience => f.write_str("meant for another (aud)"),
            Self::Time => f.write_str("ended, or not yet made"),
            Self::TooLong => f.write_str("lives longer than its kind may"),
            Self::Spent => f.write_str("taken before: each is taken once"),
            Self::Full => f.write_str("too many taken at once: try again shortly"),
        }
    }
}

impl std::error::Error for Refused {}

/// `claims`, signed by `signer` as a JWS of kind `typ`, its `kid` the
/// signer's fingerprint.
pub fn sign<C: Serialize>(signer: &Signer, typ: &str, claims: &C) -> Secret {
    let header = Header {
        alg: HeaderAlg::EdDSA,
        typ: typ.to_string(),
        kid: signer.fingerprint().to_string(),
    };
    let part = |value: &dyn erased::Json| B64.encode(value.json());
    let input = format!("{}.{}", part(&header), part(claims));
    let signature = B64.encode(signer.sign(input.as_bytes()));
    Secret::new(format!("{input}.{signature}"))
}

mod erased {
    //! A value as its JSON bytes, so two shapes share one encoder.
    pub trait Json {
        fn json(&self) -> Vec<u8>;
    }
    impl<T: serde::Serialize> Json for T {
        fn json(&self) -> Vec<u8> {
            serde_json::to_vec(self).expect("a declared shape is JSON")
        }
    }
}

/// A JWS as it arrived: its header read and checked, its claims unread
/// until its signature passes.
#[derive(Debug)]
pub struct Unverified {
    header: Header,
    input: String,
    claims: Vec<u8>,
    signature: Vec<u8>,
}

impl Unverified {
    /// `text` as a compact JWS of kind `typ`: three base64url parts, its
    /// header EdDSA, `typ` and `kid` alone.
    pub fn parse(text: &str, typ: &str) -> Result<Self, Refused> {
        if text.len() > limits().jws_max {
            return Err(Refused::Malformed);
        }
        let mut parts = text.split('.');
        let (Some(head), Some(body), Some(sig), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(Refused::Malformed);
        };
        let decode = |part: &str| B64.decode(part).map_err(|_| Refused::Malformed);
        let header: Header = serde_json::from_slice(&decode(head)?).map_err(|_| Refused::Header)?;
        header.validate("header").map_err(|_| Refused::Header)?;
        if header.typ != typ {
            return Err(Refused::Kind);
        }
        let signature = decode(sig)?;
        if signature.len() != 64 {
            return Err(Refused::Malformed);
        }
        Ok(Self {
            header,
            input: format!("{head}.{body}"),
            claims: decode(body)?,
            signature,
        })
    }

    /// The fingerprint of the verifier its header names.
    pub fn kid(&self) -> &str {
        &self.header.kid
    }

    /// Checked with `verifier`, which its `kid` must name.
    pub fn verify(self, verifier: &Verifier) -> Result<Verified, Refused> {
        if verifier.fingerprint().as_str() != self.kid() {
            return Err(Refused::UnknownVerifier);
        }
        match verifier.verify(self.input.as_bytes(), &self.signature) {
            true => Ok(Verified {
                kid: self.header.kid,
                claims: self.claims,
            }),
            false => Err(Refused::Signature),
        }
    }

    /// Checked with the one of `verifiers` its `kid` names.
    pub fn verify_with(self, verifiers: &Verifiers) -> Result<Verified, Refused> {
        let verifier = verifiers
            .get(self.kid())
            .ok_or(Refused::UnknownVerifier)?
            .clone();
        self.verify(&verifier)
    }
}

/// A JWS whose signature passed: only now are its claims read.
#[derive(Debug)]
pub struct Verified {
    kid: String,
    claims: Vec<u8>,
}

impl Verified {
    /// The fingerprint of the verifier that checked it.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// Its claims as `T`, read strictly: an unknown claim, a missing one
    /// or one out of its shape is refused.
    pub fn claims<T: DeserializeOwned + Validate>(&self) -> Result<T, Refused> {
        let claims: T =
            serde_json::from_slice(&self.claims).map_err(|e| Refused::Claims(e.to_string()))?;
        claims
            .validate("claims")
            .map_err(|e| Refused::Claims(format!("{}: {}", e.at, e.why)))?;
        Ok(claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Protocol, SignatureClaims};

    fn claims() -> SignatureClaims {
        SignatureClaims {
            sub: "user:k3m4n5p6q7r8".into(),
            aud: "service:a1b2c3d4e5f6".into(),
            iat: 100,
            exp: 160,
            jti: crate::jti(),
        }
    }

    #[test]
    fn a_jws_is_checked_before_its_claims_are_read() {
        let signer = Signer::generate(Protocol::Ssh).unwrap();
        let text = sign(&signer, "lnks", &claims());
        let unverified = Unverified::parse(text.expose(), "lnks").unwrap();
        assert_eq!(unverified.kid(), signer.fingerprint().as_str());
        let verified = unverified.verify(&signer.verifier()).unwrap();
        let read: SignatureClaims = verified.claims().unwrap();
        assert_eq!(read.sub, "user:k3m4n5p6q7r8");
    }

    #[test]
    fn a_jws_of_another_kind_or_signer_is_refused() {
        let signer = Signer::generate(Protocol::Ssh).unwrap();
        let other = Signer::generate(Protocol::Ssh).unwrap();
        let text = sign(&signer, "lnks", &claims());
        assert_eq!(
            Unverified::parse(text.expose(), "at+jwt").unwrap_err(),
            Refused::Kind
        );
        let unverified = Unverified::parse(text.expose(), "lnks").unwrap();
        assert_eq!(
            unverified.verify(&other.verifier()).unwrap_err(),
            Refused::UnknownVerifier
        );
        // Signed by another, naming this signer's fingerprint.
        let forged = sign(&other, "lnks", &claims());
        let (_, rest) = forged.expose().split_once('.').unwrap();
        let (head, _) = text.expose().split_once('.').unwrap();
        let swapped = format!("{head}.{rest}");
        let unverified = Unverified::parse(&swapped, "lnks").unwrap();
        assert_eq!(
            unverified.verify(&signer.verifier()).unwrap_err(),
            Refused::Signature
        );
        assert_eq!(
            Unverified::parse(&"a".repeat(limits().jws_max + 1), "lnks").unwrap_err(),
            Refused::Malformed
        );
        for not in ["", "a.b", "a.b.c.d", "!.!.!"] {
            assert!(Unverified::parse(not, "lnks").is_err(), "{not}");
        }
    }

    #[test]
    fn a_header_holding_more_or_another_algorithm_is_refused() {
        let signer = Signer::generate(Protocol::Ssh).unwrap();
        let kid = signer.fingerprint().to_string();
        for header in [
            serde_json::json!({"alg": "none", "typ": "lnks", "kid": kid}),
            serde_json::json!({"alg": "EdDSA", "typ": "lnks", "kid": kid, "jku": "https://x"}),
            serde_json::json!({"alg": "EdDSA", "typ": "lnks"}),
        ] {
            let head = B64.encode(serde_json::to_vec(&header).unwrap());
            let text = format!("{head}.{}.{}", B64.encode(b"{}"), B64.encode([0u8; 64]));
            assert_eq!(
                Unverified::parse(&text, "lnks").unwrap_err(),
                Refused::Header,
                "{header}"
            );
        }
    }

    #[test]
    fn claims_holding_more_are_refused() {
        let signer = Signer::generate(Protocol::Ssh).unwrap();
        let mut value = serde_json::to_value(claims()).unwrap();
        value["act"] = "someone".into();
        let text = sign(&signer, "lnks", &value);
        let verified = Unverified::parse(text.expose(), "lnks")
            .unwrap()
            .verify(&signer.verifier())
            .unwrap();
        assert!(matches!(
            verified.claims::<SignatureClaims>(),
            Err(Refused::Claims(_))
        ));
    }
}

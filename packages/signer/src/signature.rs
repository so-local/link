//! A signature: the short statement a signer makes for one exchange, for
//! one party, spent once within a minute. Not the JWS signature that
//! closes it, which every JWS ends with.

use link_secretive::Secret;

use crate::jws::{self, Refused, Unverified};
use crate::{Fingerprint, Seen, SignatureClaims, Signer, Verifiers, limits};

/// A signature's `typ`, so it's never taken for anything else signed.
pub const TYP: &str = "lnks";

/// `signer`'s signature for `sub`, meant for `aud` alone, made at `now`:
/// good for one exchange, for [`crate::Limits::signature_life`] seconds.
pub fn sign(signer: &Signer, sub: &str, aud: &str, now: u64) -> Secret {
    let claims = SignatureClaims {
        sub: sub.to_string(),
        aud: aud.to_string(),
        iat: now,
        exp: now + limits().signature_life,
        jti: crate::jti(),
    };
    jws::sign(signer, TYP, &claims)
}

/// What a signature that passed says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    /// What it signs for.
    pub sub: String,
    /// The verifier that checked it.
    pub fingerprint: Fingerprint,
}

/// `text` as a signature one of `verifiers` made, meant for `aud`, good at
/// `now`, and taken from `seen`: refused if meant for another, ended,
/// made further ahead than clocks drift, living longer than a signature
/// may, or taken before.
pub fn check(
    text: &str,
    verifiers: &Verifiers,
    aud: &str,
    now: u64,
    seen: &Seen,
) -> Result<Signed, Refused> {
    let verified = Unverified::parse(text, TYP)?.verify_with(verifiers)?;
    let claims: SignatureClaims = verified.claims()?;
    if claims.aud != aud {
        return Err(Refused::Audience);
    }
    if claims.exp < claims.iat || claims.exp - claims.iat > limits().signature_life {
        return Err(Refused::TooLong);
    }
    if claims.exp <= now || claims.iat > now + limits().clock_skew {
        return Err(Refused::Time);
    }
    seen.take(&claims.jti, claims.exp, now)?;
    Ok(Signed {
        sub: claims.sub,
        fingerprint: verified
            .kid()
            .parse()
            .expect("a checked kid is a verifier's fingerprint"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Protocol;

    const IAM: &str = "service:k3m4n5p6q7r8";

    #[test]
    fn a_signature_is_taken_once_where_it_was_meant() {
        let signer = Signer::generate(Protocol::Ssh).unwrap();
        let held: Verifiers = [signer.verifier()].into_iter().collect();
        let seen = Seen::new();
        let text = sign(&signer, "user:a1b2c3d4e5f6", IAM, 1_000);
        let signed = check(text.expose(), &held, IAM, 1_010, &seen).unwrap();
        assert_eq!(signed.sub, "user:a1b2c3d4e5f6");
        assert_eq!(&signed.fingerprint, signer.fingerprint());
        assert_eq!(
            check(text.expose(), &held, IAM, 1_011, &seen),
            Err(Refused::Spent)
        );
    }

    #[test]
    fn a_signature_for_another_ended_or_unheld_is_refused() {
        let signer = Signer::generate(Protocol::Ssh).unwrap();
        let held: Verifiers = [signer.verifier()].into_iter().collect();
        let text = sign(&signer, "user:a1b2c3d4e5f6", IAM, 1_000);
        let check_at = |aud: &str, now: u64, verifiers: &Verifiers| {
            check(text.expose(), verifiers, aud, now, &Seen::new())
        };
        assert_eq!(
            check_at("service:zzzzzzzzzzzz", 1_010, &held),
            Err(Refused::Audience)
        );
        assert_eq!(check_at(IAM, 1_060, &held), Err(Refused::Time));
        assert_eq!(check_at(IAM, 990, &held), Err(Refused::Time));
        assert_eq!(
            check_at(IAM, 1_010, &Verifiers::none()),
            Err(Refused::UnknownVerifier)
        );
        // One that lives longer than a minute, though signed.
        let long = SignatureClaims {
            sub: "user:a1b2c3d4e5f6".into(),
            aud: IAM.into(),
            iat: 1_000,
            exp: 1_000 + limits().signature_life + 1,
            jti: crate::jti(),
        };
        let text = jws::sign(&signer, TYP, &long);
        assert_eq!(
            check(text.expose(), &held, IAM, 1_010, &Seen::new()),
            Err(Refused::TooLong)
        );
    }
}

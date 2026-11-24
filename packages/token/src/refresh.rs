//! A refresh token: `prefixes.toml`'s `refresh` before a JWS typed
//! `lnkr`, issued only by a login (a browser, a phone, an installation, or
//! a wake spending its one-time code) and traded at its issuer alone, only
//! ever for access tokens. It names its principal and the kind of client
//! it was issued to, and carries no actions. Its issuer records each by
//! its `jti`, so it can end one.

use link_principal::Principal;
use link_secretive::Secret;
use link_signer::{Signer, Unverified, Verifiers, jws};

use crate::declared::{Jti, PrincipalText};
use crate::token::check_kinds;
use crate::{Client, RefreshClaims, Refused, prefixes};

/// A refresh token's `typ`, inside its prefix, so it's never taken for an
/// access token.
pub const TYP: &str = "lnkr";

/// Whether `text` opens as a refresh token does.
pub fn is_refresh(text: &str) -> bool {
    text.starts_with(prefixes().refresh.as_str())
}

/// What a refresh token is issued for.
#[derive(Clone, Debug)]
pub struct Issue<'a> {
    /// Its principal.
    pub sub: &'a Principal,
    /// The kind of client it's for.
    pub client: Client,
    /// Its own ID, from `link_signer::jti`.
    pub jti: &'a str,
    /// The wake, `<kind>:<id>@<run>`, its workload and run: a wake's alone.
    pub wkl: Option<&'a str>,
    /// When it's issued, in Unix seconds.
    pub iat: u64,
    /// When it ends, in Unix seconds.
    pub exp: u64,
}

/// A refresh token that passed its checks. Its issuer still asks its own
/// record whether it was ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub sub: Principal,
    pub client: Client,
    pub jti: String,
    pub wkl: Option<String>,
    /// When it was issued.
    pub iat: u64,
    pub exp: u64,
}

fn validate<T: link_actions::value::Validate>(claims: &T) -> Result<(), Refused> {
    link_actions::value::Validate::validate(claims, "claims")
        .map_err(|e| Refused::Claims(format!("{}: {}", e.at, e.why)))
}

fn principal(text: &str) -> Result<Principal, Refused> {
    text.parse::<Principal>()
        .map_err(|e| Refused::Claims(e.to_string()))
}

/// `issuer`'s refresh token for `issue`, signed by `signer`.
pub fn mint(signer: &Signer, issuer: &Principal, issue: &Issue<'_>) -> Result<Secret, Refused> {
    check_kinds(issue.sub, issue.client, issue.wkl)?;
    if issue.exp <= issue.iat {
        return Err(Refused::Time);
    }
    let claims = RefreshClaims {
        iss: PrincipalText(issuer.as_str().to_string()),
        sub: PrincipalText(issue.sub.as_str().to_string()),
        client: issue.client,
        jti: Jti(issue.jti.to_string()),
        wkl: issue.wkl.map(str::to_string),
        iat: issue.iat,
        exp: issue.exp,
    };
    validate(&claims)?;
    let signed = jws::sign(signer, TYP, &claims);
    Ok(signed.expose_map(|jws| format!("{}{jws}", prefixes().refresh)))
}

/// `text` as a refresh token `issuer` issued with one of `verifiers`, good
/// at `now`: refused if another issuer's, ended, or its kinds don't hold
/// together.
pub fn check(
    text: &str,
    verifiers: &Verifiers,
    issuer: &Principal,
    now: u64,
) -> Result<Held, Refused> {
    let jws = text
        .strip_prefix(prefixes().refresh.as_str())
        .ok_or(Refused::Jws(link_signer::Refused::Malformed))?;
    let claims: RefreshClaims = Unverified::parse(jws, TYP)?
        .verify_with(verifiers)?
        .claims()?;
    if principal(&claims.iss.0)? != *issuer {
        return Err(Refused::Issuer);
    }
    let sub = principal(&claims.sub.0)?;
    check_kinds(&sub, claims.client, claims.wkl.as_deref())?;
    if claims.exp <= now {
        return Err(Refused::Time);
    }
    Ok(Held {
        sub,
        client: claims.client,
        jti: claims.jti.0,
        wkl: claims.wkl,
        iat: claims.iat,
        exp: claims.exp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_signer::Protocol;

    struct Issuer {
        signer: Signer,
        me: Principal,
        held: Verifiers,
    }

    fn issuer() -> Issuer {
        let signer = Signer::generate(Protocol::Ssh).unwrap();
        let held = [signer.verifier()].into_iter().collect();
        Issuer {
            signer,
            me: "service:k3m4n5p6q7r8".parse().unwrap(),
            held,
        }
    }

    fn user() -> Principal {
        "user:p6q7r8s9t2v3".parse().unwrap()
    }

    #[test]
    fn a_refresh_token_is_taken_by_its_issuer_alone_until_it_ends() {
        let issuer = issuer();
        let jti = link_signer::jti();
        let sub = user();
        let issue = Issue {
            sub: &sub,
            client: Client::App,
            jti: &jti,
            wkl: None,
            iat: 1_000,
            exp: 2_000,
        };
        let text = mint(&issuer.signer, &issuer.me, &issue).unwrap();
        assert!(text.expose().starts_with(prefixes().refresh.as_str()));
        let held = check(text.expose(), &issuer.held, &issuer.me, 1_500).unwrap();
        assert_eq!(
            held,
            Held {
                sub: sub.clone(),
                client: Client::App,
                jti: jti.clone(),
                wkl: None,
                iat: 1_000,
                exp: 2_000
            }
        );
        assert_eq!(
            check(text.expose(), &issuer.held, &issuer.me, 2_000),
            Err(Refused::Time)
        );
        let other: Principal = "service:zzzzzzzzzzzz".parse().unwrap();
        assert_eq!(
            check(text.expose(), &issuer.held, &other, 1_500),
            Err(Refused::Issuer)
        );
        let stranger = self::issuer();
        assert_eq!(
            check(text.expose(), &stranger.held, &issuer.me, 1_500),
            Err(Refused::Jws(link_signer::Refused::UnknownVerifier))
        );
    }

    /// A refresh token is never an access token, nor one an access token.
    #[test]
    fn a_refresh_token_and_an_access_token_are_never_each_other() {
        let issuer = issuer();
        let jti = link_signer::jti();
        let sub: Principal = "service:m4n5p6q7r8s9".parse().unwrap();
        let issue = Issue {
            sub: &sub,
            client: Client::Wake,
            jti: &jti,
            wkl: Some("workload:a1b2c3d4e5f6@wake-1042"),
            iat: 1_000,
            exp: 1_600,
        };
        let refresh = mint(&issuer.signer, &issuer.me, &issue).unwrap();
        assert!(refresh.expose().starts_with("lnk_refresh_"));
        assert!(is_refresh(refresh.expose()));
        let held = check(refresh.expose(), &issuer.held, &issuer.me, 1_010).unwrap();
        assert_eq!(held.wkl.as_deref(), Some("workload:a1b2c3d4e5f6@wake-1042"));
        let trust = crate::Trust::new(
            crate::Issuers::none().with(issuer.me.clone(), issuer.held.clone()),
            "issuer",
        );
        let as_access = refresh
            .expose()
            .replacen(&prefixes().refresh, &prefixes().access, 1);
        assert_eq!(
            trust.check(&as_access, 1_010),
            Err(Refused::Jws(link_signer::Refused::Kind))
        );
        let access = crate::Mint {
            signer: &issuer.signer,
            issuer: &issuer.me,
        }
        .token(
            &crate::Ask {
                sub: &sub,
                client: Client::Wake,
                aud: "store",
                wkl: issue.wkl,
                life: 60,
            },
            1_000,
        )
        .unwrap();
        let as_refresh = access
            .expose()
            .replacen(&prefixes().access, &prefixes().refresh, 1);
        assert!(check(&as_refresh, &issuer.held, &issuer.me, 1_010).is_err());
    }

    #[test]
    fn its_kind_of_client_and_principal_hold_together() {
        let issuer = issuer();
        let jti = link_signer::jti();
        let sub = user();
        let issue = |client, wkl| Issue {
            sub: &sub,
            client,
            jti: &jti,
            wkl,
            iat: 1_000,
            exp: 2_000,
        };
        assert!(mint(&issuer.signer, &issuer.me, &issue(Client::Service, None)).is_err());
        assert!(
            mint(
                &issuer.signer,
                &issuer.me,
                &issue(Client::App, Some("workload:a1b2c3d4e5f6@wake-1042"))
            )
            .is_err()
        );
        assert!(mint(&issuer.signer, &issuer.me, &issue(Client::Wake, None)).is_err());
        assert!(mint(&issuer.signer, &issuer.me, &issue(Client::Cli, None)).is_ok());
    }
}

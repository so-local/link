//! Access tokens and the assertions installations sign for them, as the spec
//! has them (`InstallationAssertionHeader`, `InstallationAssertionClaims`,
//! `AccessTokenHeader`, `AccessTokenClaims`): compact JWS, Ed25519 alone.
//!
//! - An installation signs an [`assertion`] with its key for each access token
//!   it asks for: one request, at most [`ASSERTION_LIFE`] seconds.
//! - Whatever an access token is for checks it with [`check`] and the
//!   account service's public keys alone ([`Keys`]), never asking the
//!   service. The header is read first: `alg` `EdDSA` and `typ` `at+jwt`
//!   alone, its `kid` one it holds. Then the signature, then the claims,
//!   closed: the issuer it trusts, its own audience, a subject of a kind
//!   it takes, asked for by an installation's signer or not as it takes, a grant of what it's asked to do ([`Grants::allows`]), and
//!   a life no longer than its own longest, with no allowance for clocks.

use std::fmt;

use base64::Engine;
use link_secretive::{Secret, SigningSecret};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::types::{
    AccessTokenClaims, AccessTokenHeader, AgentId, GrantAction, Grants, InstallationId, Owner,
    PrincipalKind, Scope, SigningKeys,
};

/// The longest an installation's assertion lasts, from its making.
pub const ASSERTION_LIFE: u64 = 60;

/// The longest token read: a real one is a few hundred bytes.
const MAX_TOKEN: usize = 4096;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// Unix seconds, now.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// An installation signing for an access token: who it is, and its key.
pub struct Installation<'a> {
    pub id: &'a InstallationId,
    pub account: &'a Owner,
    pub key: &'a SigningSecret,
}

/// The assertion `installation` signs for an access token of `scope` for
/// `resource`, from the account service at `service` (its origin), made
/// at `now`: good for one request, [`ASSERTION_LIFE`] seconds.
pub fn assertion(
    installation: &Installation<'_>,
    service: &str,
    resource: &str,
    scope: Scope,
    now: u64,
) -> Secret {
    signed_assertion(installation, service, resource, scope, None, now)
}

/// The assertion `installation` signs for an access token for its agent
/// `agent`, as its service (`sub` `service:<its id>`), granted `grants` at
/// `resource`: the scope `agent`, from the account service at `service`,
/// made at `now`.
pub fn agent_assertion(
    installation: &Installation<'_>,
    service: &str,
    resource: &str,
    agent: &AgentId,
    grants: &Grants,
    now: u64,
) -> Secret {
    signed_assertion(
        installation,
        service,
        resource,
        Scope::Agent,
        Some((agent, grants)),
        now,
    )
}

fn signed_assertion(
    installation: &Installation<'_>,
    service: &str,
    resource: &str,
    scope: Scope,
    agent: Option<(&AgentId, &Grants)>,
    now: u64,
) -> Secret {
    let mut jti = [0u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut jti)
        .expect("the system's random source");
    let header = json!({ "alg": "EdDSA", "typ": "installation+jwt" });
    let claims = json!({
        "iss": installation.id,
        "sub": installation.account,
        "aud": service.trim_end_matches('/'),
        "resource": resource,
        "scope": scope,
        "iat": now,
        "exp": now + ASSERTION_LIFE,
        "jti": B64.encode(jti),
    });
    let mut claims = claims;
    if let Some((agent, grants)) = agent {
        claims["agent"] = json!(agent);
        claims["grants"] = json!(grants);
    }
    sign(installation.key, &header, &claims)
}

/// An access token, signed with `key` of id `kid`: what the account
/// service makes, here for a service written in Rust that makes them,
/// and the account service's stand-ins in tests.
pub fn access_token(key: &SigningSecret, kid: &str, claims: &AccessTokenClaims) -> Secret {
    let header = json!({ "alg": "EdDSA", "typ": "at+jwt", "kid": kid });
    sign(key, &header, claims)
}

/// A key's id, from its public half as base64url (`x`): the start of
/// that text's SHA-256, base64url, so a new key has a new id. What the
/// account service names its keys by, and any service signing on these
/// terms.
pub fn key_id(x: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, x.as_bytes());
    let mut id = B64.encode(digest.as_ref());
    id.truncate(16);
    id
}

/// A compact JWS, `header.claims.signature`, each base64url, signed
/// with `key`: what every token here is, and any other Ed25519 token
/// made on the same terms.
pub fn sign(key: &SigningSecret, header: &impl Serialize, claims: &impl Serialize) -> Secret {
    let part = |bytes: Vec<u8>| B64.encode(bytes);
    let input = format!(
        "{}.{}",
        part(serde_json::to_vec(header).expect("a header serializes")),
        part(serde_json::to_vec(claims).expect("claims serialize"))
    );
    let signature = key.sign(input.as_bytes());
    Secret::new(format!("{input}.{}", B64.encode(signature)))
}

/// The account service's public keys, each by its id: what checks its
/// access tokens.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keys(Vec<(String, [u8; 32])>);

impl Keys {
    /// As the service says them (`GET /keys`).
    pub fn from_spec(keys: &SigningKeys) -> Self {
        Self(
            keys.keys
                .iter()
                .filter_map(|key| {
                    let x = <[u8; 32]>::try_from(B64.decode(key.x.as_str()).ok()?).ok()?;
                    Some((key.kid.to_string(), x))
                })
                .collect(),
        )
    }

    /// As an operator gives them to a relay: `<kid>:<x>`, each
    /// key's id and its raw 32 bytes as base64url, separated by commas,
    /// spaces or lines.
    pub fn parse(text: &str) -> Result<Self, String> {
        let keys = text
            .split([',', ' ', '\n'])
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(|key| {
                let (kid, x) = key.split_once(':').ok_or_else(|| {
                    format!("{key:?} isn't <id>:<key>: each key with its id, as GET /keys says it")
                })?;
                let ok_kid = (1..=64).contains(&kid.len())
                    && kid
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b));
                if !ok_kid {
                    return Err(format!("{kid:?} isn't a key's id"));
                }
                let bytes = B64
                    .decode(x.trim_end_matches('='))
                    .map_err(|_| format!("{x:?} isn't base64url"))?;
                let x = <[u8; 32]>::try_from(bytes)
                    .map_err(|_| format!("{x:?} isn't an Ed25519 public key (32 bytes)"))?;
                Ok((kid.to_string(), x))
            })
            .collect::<Result<Vec<_>, String>>()?;
        if keys.is_empty() {
            return Err("no key: the account service's public keys are required".into());
        }
        Ok(Self(keys))
    }

    /// As [`Keys::parse`], each `kid` also its key's own id
    /// ([`key_id`]): for keys whose ids are made, never chosen.
    pub fn parse_checked(text: &str) -> Result<Self, String> {
        let keys = Self::parse(text)?;
        for (kid, x) in &keys.0 {
            if *kid != key_id(&B64.encode(x)) {
                return Err(format!("{kid:?} isn't its key's id"));
            }
        }
        Ok(keys)
    }

    /// The same, with the key whose public half is `x` (base64url), by
    /// its own id ([`key_id`]).
    pub fn with(mut self, x: &str) -> Result<Self, String> {
        let bytes = B64
            .decode(x)
            .map_err(|_| format!("{x:?} isn't base64url"))?;
        let raw = <[u8; 32]>::try_from(bytes)
            .map_err(|_| format!("{x:?} isn't an Ed25519 public key (32 bytes)"))?;
        let kid = key_id(x);
        if self.get(&kid).is_none() {
            self.0.push((kid, raw));
        }
        Ok(self)
    }

    /// Each as [`Keys::parse`] reads it: `<kid>:<x>`.
    pub fn texts(&self) -> impl Iterator<Item = String> + '_ {
        self.0
            .iter()
            .map(|(kid, x)| format!("{kid}:{}", B64.encode(x)))
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn get(&self, kid: &str) -> Option<&[u8; 32]> {
        self.0.iter().find(|(id, _)| id == kid).map(|(_, x)| x)
    }
}

/// What a checker takes: the account service it trusts, itself, what a
/// token must be and grant there, and the longest life it allows.
#[derive(Clone, Copy, Debug)]
pub struct Expect<'a> {
    /// The account service's origin, as its tokens' `iss`.
    pub issuer: &'a str,
    /// The checker, as its tokens' `aud`: its domain or its origin.
    pub audience: &'a str,
    pub need: Need<'a>,
    /// Seconds.
    pub max_life: u64,
}

/// What a token must be and grant for one thing a checker does: whose
/// kinds of subject it takes, and the action it asks on a resource.
#[derive(Clone, Copy, Debug)]
pub struct Need<'a> {
    /// The kinds of `sub` it takes: a grant is read only for a subject
    /// this checker serves, so a broad grant to another kind is never
    /// taken for one of these.
    pub subjects: &'a [PrincipalKind],
    /// Whether a user's token it takes is one an installation's signer asked
    /// for, naming its installation (`installation`), alone, or one that names none
    /// alone, a visitor's: either is refused where the other is taken, so
    /// an installation's token is never a visitor's. A service's token never
    /// names an installation.
    pub installation: bool,
    /// The action asked and the resource it's on (`tunnel:*`), which the
    /// token's grants must allow; `None` where a token only says who it
    /// is.
    pub asks: Option<(GrantAction, &'a str)>,
}

/// Whether a token made at `iat` and ending at `exp` (Unix seconds) holds
/// at `now`: it ends after it's made, lives at most `max_life`, hasn't
/// ended, and ends no further off than its longest life. `skew` is how
/// far the clocks of its maker and its checker may differ; none where
/// the checker made it or trusts its maker's clock alone.
pub fn timely(iat: u64, exp: u64, now: u64, max_life: u64, skew: u64) -> Result<(), Refused> {
    let lives = iat <= exp && exp - iat <= max_life;
    let ended = exp.saturating_add(skew) <= now;
    let too_far = exp > now.saturating_add(max_life).saturating_add(skew);
    if !lives || ended || too_far {
        return Err(Refused::Expired);
    }
    Ok(())
}

impl Grants {
    /// Whether a grant here allows `action` on `resource` (`tunnel:web`):
    /// one of that action, `<kind>.*` of its kind or `*`, on that
    /// resource, on one its pattern begins (`tunnel:*`), or on any. Every
    /// other grant is skipped, an action this checker doesn't know among
    /// them, so a token with a newer action is taken for the one it
    /// needs.
    pub fn allows(&self, action: GrantAction, resource: &str) -> bool {
        let action = action.to_string();
        self.as_str().split(' ').any(|grant| {
            let (granted, on) = match grant.split_once('@') {
                Some((granted, on)) => (granted, Some(on)),
                None => (grant, None),
            };
            !granted.is_empty()
                && matches(granted, &action)
                && on.is_none_or(|on| matches(on, resource))
        })
    }
}

/// Whether `pattern` names `name`: the same, or ending in `*` and
/// beginning it (`*` alone names every one).
fn matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(begins) => name.starts_with(begins),
        None => pattern == name,
    }
}

/// Whether `claims` are of an account `stopped` lists that stopped after
/// they were made: such a token is refused, whatever it
/// grants, from the moment its checker holds the list.
pub fn stopped(stopped: &crate::types::StoppedAccounts, claims: &AccessTokenClaims) -> bool {
    stopped_since(stopped, claims.acc.as_str(), authorized_at(claims))
}

/// Original verified authorization, retained when a binding refreshes a token.
/// A token without `auth_time` authorizes at its mint time.
pub fn authorized_at(claims: &AccessTokenClaims) -> u64 {
    claims.auth_time.unwrap_or(claims.iat)
}

/// Whether account `account` stopped after `issued` (Unix seconds), as
/// `stopped` lists it: what was proved with a token made then, a
/// session or a visitor's cookie, holds no more. A stop's time is the
/// second after it, so a token made in its second is refused too.
pub fn stopped_since(stopped: &crate::types::StoppedAccounts, account: &str, issued: u64) -> bool {
    stopped
        .accounts
        .iter()
        .any(|s| s.account.as_str() == account && issued < s.at)
}

/// Why a token was refused, as its caller may be told: never the token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not one: made up, cut, tampered with, signed by another key, or
    /// claims this checker doesn't take.
    Invalid,
    /// Signed for another audience.
    Elsewhere,
    /// Of a subject this checker doesn't take, or without a grant of
    /// what it was asked.
    Scope,
    /// Expired, or made to last longer than this checker allows.
    Expired,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refused::Invalid => "invalid token",
            Refused::Elsewhere => "that token is for another relay or service",
            Refused::Scope => "that token isn't for this",
            Refused::Expired => "invalid or expired token",
        })
    }
}

impl std::error::Error for Refused {}

/// The claims of `token`, an access token, if `keys` and `expect` take
/// it at `now` (Unix seconds); why not otherwise.
pub fn check(
    token: &str,
    keys: &Keys,
    expect: &Expect<'_>,
    now: u64,
) -> Result<AccessTokenClaims, Refused> {
    let (_, claims): (AccessTokenHeader, AccessTokenClaims) =
        verified(token, keys, |header: &AccessTokenHeader| {
            header.kid.as_str()
        })?;
    if claims.iss.as_str() != expect.issuer.trim_end_matches('/') {
        return Err(Refused::Invalid);
    }
    if claims.aud.as_str() != expect.audience {
        return Err(Refused::Elsewhere);
    }
    if authorized_at(&claims) > claims.iat {
        return Err(Refused::Invalid);
    }
    let signer = match claims.sub.kind() {
        PrincipalKind::User => expect.need.installation == claims.installation.is_some(),
        PrincipalKind::Service => claims.installation.is_none(),
    };
    if !expect.need.subjects.contains(&claims.sub.kind()) || !signer {
        return Err(Refused::Scope);
    }
    if let Some((action, resource)) = expect.need.asks
        && !claims.scope.allows(action, resource)
    {
        return Err(Refused::Scope);
    }
    timely(claims.iat, claims.exp, now, expect.max_life, 0)?;
    Ok(claims)
}

/// The header and claims of `token`, a compact JWS, if one of `keys`
/// signed it: the header read first, as `H` and closed, the key it names
/// (`kid`) one of `keys`; then the signature; then the claims, as `C`
/// and closed. What the claims say is the caller's to check.
pub fn verified<H, C>(token: &str, keys: &Keys, kid: impl Fn(&H) -> &str) -> Result<(H, C), Refused>
where
    H: DeserializeOwned,
    C: DeserializeOwned,
{
    if token.len() > MAX_TOKEN {
        return Err(Refused::Invalid);
    }
    let mut parts = token.split('.');
    let (Some(header), Some(claims), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refused::Invalid);
    };
    let header: H = read(header)?;
    let key = keys.get(kid(&header)).ok_or(Refused::Invalid)?;
    let signature = B64.decode(signature).map_err(|_| Refused::Invalid)?;
    let input = &token[..header_len(token, claims)];
    if !link_secretive::signing::verify(key, input.as_bytes(), &signature) {
        return Err(Refused::Invalid);
    }
    let claims: C = read(claims)?;
    Ok((header, claims))
}

/// How long `token`'s signed part (its header, `.`, its claims) is.
fn header_len(token: &str, claims: &str) -> usize {
    token.find('.').map_or(0, |dot| dot + 1) + claims.len()
}

/// One part, base64url JSON, read as `T`: closed, as every spec type is.
fn read<T: DeserializeOwned>(part: &str) -> Result<T, Refused> {
    let bytes = B64.decode(part).map_err(|_| Refused::Invalid)?;
    serde_json::from_slice(&bytes).map_err(|_| Refused::Invalid)
}

#[cfg(test)]
mod tests;

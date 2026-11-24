//! An access token: issued and signed by an issuer, verified by every
//! resource server it's for with the verifiers of the issuers it trusts,
//! never asking one.

use std::collections::BTreeMap;
use std::fmt;

use link_principal::{Principal, PrincipalKind};
use link_secretive::Secret;
use link_signer::{Signer, Unverified, Verifiers, jws};

use crate::declared::{Audience, PrincipalText};
use crate::{Claims, Client, Place, limits, prefixes};

/// An access token's `typ`, as OAuth names it (RFC 9068), so a signature
/// or a refresh token is never taken for one. Written after its prefix,
/// `prefixes.toml`'s `access`, which [`Trust::check`] alone strips.
pub const TYP: &str = "at+jwt";

/// Why an access token was refused. Says nothing of what it held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not a JWS signed by a verifier of an issuer trusted here.
    Jws(link_signer::Refused),
    /// Its `iss` isn't the trusted issuer whose verifier signed it.
    Issuer,
    /// For another audience.
    Audience,
    /// Ended, or made further ahead than clocks drift.
    Time,
    /// Lives longer than this resource server takes.
    TooLong,
    /// Its claims aren't an access token's, saying where.
    Claims(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Jws(why) => write!(f, "not an access token: {why}"),
            Self::Issuer => f.write_str("issued by an issuer not trusted here (iss)"),
            Self::Audience => f.write_str("for another audience (aud)"),
            Self::Time => f.write_str("ended, or not yet made"),
            Self::TooLong => f.write_str("lives longer than access tokens here may"),
            Self::Claims(why) => write!(f, "its claims: {why}"),
        }
    }
}

impl std::error::Error for Refused {}

impl From<link_signer::Refused> for Refused {
    fn from(why: link_signer::Refused) -> Self {
        match why {
            link_signer::Refused::Claims(at) => Self::Claims(at),
            why => Self::Jws(why),
        }
    }
}

impl Client {
    /// The kind of client an assignment's `via` names for it: the app's or
    /// the command line's, and none for a service, a box, or a wake, each
    /// a service principal whose assignments name no `via`.
    pub fn via(self) -> Option<Place> {
        match self {
            Self::App => Some(Place::App),
            Self::Cli => Some(Place::Cli),
            Self::Service | Self::Box | Self::Wake => None,
        }
    }

    /// The kind of principal an access token from this kind of client
    /// names: a user from the app or a command line, a service otherwise.
    pub fn principal_kind(self) -> PrincipalKind {
        match self {
            Self::App | Self::Cli => PrincipalKind::User,
            Self::Service | Self::Box | Self::Wake => PrincipalKind::Service,
        }
    }
}

/// An error unless `sub`, `client`, and `wkl` hold together: the
/// principal the kind of client logs in, and a wake named by a wake's
/// access token alone.
pub(crate) fn check_kinds(
    sub: &Principal,
    client: Client,
    wkl: Option<&str>,
) -> Result<(), Refused> {
    if sub.kind() != client.principal_kind() {
        return Err(Refused::Claims(format!(
            "sub: an access token from a {} client names a {}",
            link_actions::value::Words::word(client),
            client.principal_kind().as_str()
        )));
    }
    match (client, wkl) {
        (Client::Wake, None) => Err(Refused::Claims("wkl: a wake's names its wake".into())),
        (Client::Wake, Some(wkl)) if !wkl.contains(link_id::RUN_SEPARATOR) => Err(Refused::Claims(
            "wkl: a wake is <kind>:<id>@<run>, its workload and run".into(),
        )),
        (Client::Wake, Some(_)) | (_, None) => Ok(()),
        (_, Some(_)) => Err(Refused::Claims("wkl: only a wake's names a wake".into())),
    }
}

/// An issuer issuing access tokens: its signer, and who it is.
pub struct Mint<'a> {
    pub signer: &'a Signer,
    pub issuer: &'a Principal,
}

/// What one access token is issued for.
#[derive(Clone, Debug)]
pub struct Ask<'a> {
    /// Who acts.
    pub sub: &'a Principal,
    /// The kind of client it logged in from.
    pub client: Client,
    /// Its audience.
    pub aud: &'a str,
    /// The wake it acts in, `<kind>:<id>@<run>`, its workload and run: a wake's alone.
    pub wkl: Option<&'a str>,
    /// Its life, in seconds.
    pub life: u64,
}

impl Mint<'_> {
    /// The access token `ask` asks for, made at `now`: refused, never
    /// issued, if its audience isn't one, its kinds don't hold together,
    /// or its life is longer than any access token's.
    pub fn token(&self, ask: &Ask<'_>, now: u64) -> Result<Secret, Refused> {
        if ask.life == 0 || ask.life > limits().token_life_max {
            return Err(Refused::TooLong);
        }
        check_kinds(ask.sub, ask.client, ask.wkl)?;
        let claims = Claims {
            iss: PrincipalText(self.issuer.as_str().to_string()),
            sub: PrincipalText(ask.sub.as_str().to_string()),
            client: ask.client,
            aud: Audience(ask.aud.to_string()),
            wkl: ask.wkl.map(str::to_string),
            iat: now,
            exp: now + ask.life,
        };
        link_actions::value::Validate::validate(&claims, "claims")
            .map_err(|e| Refused::Claims(format!("{}: {}", e.at, e.why)))?;
        let signed = jws::sign(self.signer, TYP, &claims);
        Ok(signed.expose_map(|jws| format!("{}{jws}", prefixes().access)))
    }
}

/// The issuers a resource server trusts, each known by its verifiers:
/// data it's given, never one issuer written into its code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Issuers(BTreeMap<Principal, Verifiers>);

impl Issuers {
    /// None: every access token refused.
    pub fn none() -> Self {
        Self::default()
    }

    /// These and `issuer`, known by `verifiers`.
    pub fn with(mut self, issuer: Principal, verifiers: Verifiers) -> Self {
        self.set(issuer, verifiers);
        self
    }

    /// `issuer`'s verifiers now, as it publishes a new one beside the old
    /// or drops one: added to the set when it wasn't in it.
    pub fn set(&mut self, issuer: Principal, verifiers: Verifiers) {
        self.0.insert(issuer, verifiers);
    }

    /// Each issuer trusted, and its verifiers.
    pub fn iter(&self) -> impl Iterator<Item = (&Principal, &Verifiers)> {
        self.0.iter()
    }

    /// `issuer`'s verifiers, when it's trusted.
    pub fn verifiers(&self, issuer: &Principal) -> Option<&Verifiers> {
        self.0.get(issuer)
    }

    /// The trusted issuer one of whose verifiers is `kid`, and those
    /// verifiers.
    fn signed_by(&self, kid: &str) -> Option<(&Principal, &Verifiers)> {
        self.0
            .iter()
            .find(|(_, verifiers)| verifiers.get(kid).is_some())
    }
}

/// What a resource server trusts: a set of issuers, the audience it is,
/// and the longest life it takes.
#[derive(Clone, Debug)]
pub struct Trust {
    issuers: Issuers,
    audience: String,
    longest: u64,
}

impl Trust {
    /// Access tokens `issuers` issued for `audience`.
    pub fn new(issuers: Issuers, audience: &str) -> Self {
        Self {
            issuers,
            audience: audience.to_string(),
            longest: limits().token_life_max,
        }
    }

    /// The same, taking no access token that lives longer than `longest`
    /// seconds.
    pub fn longest(mut self, longest: u64) -> Self {
        self.longest = longest.min(limits().token_life_max);
        self
    }

    /// The issuers trusted, to change their verifiers.
    pub fn issuers_mut(&mut self) -> &mut Issuers {
        &mut self.issuers
    }

    pub fn issuers(&self) -> &Issuers {
        &self.issuers
    }

    pub fn audience(&self) -> &str {
        &self.audience
    }

    /// `text` as an access token for this resource server, good at `now`:
    /// its header an access token's, its signature a verifier's of one
    /// trusted issuer, then its claims, closed: its `iss` that issuer, its
    /// audience this one, its kinds holding together, its life no longer
    /// than taken and not ended.
    pub fn check(&self, text: &str, now: u64) -> Result<Checked, Refused> {
        let jws = text
            .strip_prefix(prefixes().access.as_str())
            .ok_or(Refused::Jws(link_signer::Refused::Malformed))?;
        let unverified = Unverified::parse(jws, TYP)?;
        let (issuer, verifiers) = self
            .issuers
            .signed_by(unverified.kid())
            .ok_or(Refused::Jws(link_signer::Refused::UnknownVerifier))?;
        let claims: Claims = unverified.verify_with(verifiers)?.claims()?;
        let principal = |text: &str| {
            text.parse::<Principal>()
                .map_err(|e| Refused::Claims(e.to_string()))
        };
        if principal(&claims.iss.0)? != *issuer {
            return Err(Refused::Issuer);
        }
        if claims.aud.0 != self.audience {
            return Err(Refused::Audience);
        }
        if claims.exp < claims.iat || claims.exp - claims.iat > self.longest {
            return Err(Refused::TooLong);
        }
        if claims.exp <= now || claims.iat > now + link_signer::limits().clock_skew {
            return Err(Refused::Time);
        }
        let sub = principal(&claims.sub.0)?;
        check_kinds(&sub, claims.client, claims.wkl.as_deref())?;
        Ok(Checked {
            iss: issuer.clone(),
            sub,
            client: claims.client,
            aud: claims.aud.0,
            wkl: claims.wkl,
            iat: claims.iat,
            exp: claims.exp,
        })
    }
}

/// An access token that passed its checks: who acts, from which kind of
/// client. It allows nothing itself: the resource server reads the
/// assignments of what it's asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checked {
    iss: Principal,
    sub: Principal,
    client: Client,
    aud: String,
    wkl: Option<String>,
    iat: u64,
    exp: u64,
}

impl Checked {
    /// The issuer that issued it.
    pub fn issuer(&self) -> &Principal {
        &self.iss
    }

    /// Who acts.
    pub fn principal(&self) -> &Principal {
        &self.sub
    }

    /// The kind of client it logged in from.
    pub fn client(&self) -> Client {
        self.client
    }

    /// The `via` an assignment must name to be used with it: none for a
    /// service principal's.
    pub fn via(&self) -> Option<Place> {
        self.client.via()
    }

    pub fn audience(&self) -> &str {
        &self.aud
    }

    /// The wake it acts in, `<kind>:<id>@<run>`, its workload and run: for refusing a wake that
    /// has ended, and for logs, never for granting.
    pub fn workload(&self) -> Option<&str> {
        self.wkl.as_deref()
    }

    /// The run of the wake it acts in.
    pub fn run(&self) -> Option<&str> {
        self.wkl
            .as_deref()
            .and_then(|wkl| wkl.split_once(link_id::RUN_SEPARATOR))
            .map(|(_, run)| run)
    }

    /// When it was made, in Unix seconds.
    pub fn made(&self) -> u64 {
        self.iat
    }

    /// When it ends, in Unix seconds.
    pub fn ends(&self) -> u64 {
        self.exp
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_signer::Protocol;

    fn parse<T: std::str::FromStr>(text: &str) -> T
    where
        T::Err: fmt::Debug,
    {
        text.parse().unwrap()
    }

    struct Issuer {
        signer: Signer,
        me: Principal,
    }

    impl Issuer {
        fn new(me: &str) -> Self {
            Self {
                signer: Signer::generate(Protocol::Ssh).unwrap(),
                me: parse(me),
            }
        }

        fn mint(&self) -> Mint<'_> {
            Mint {
                signer: &self.signer,
                issuer: &self.me,
            }
        }

        fn issuers(&self) -> Issuers {
            Issuers::none().with(
                self.me.clone(),
                [self.signer.verifier()].into_iter().collect(),
            )
        }
    }

    fn wake() -> Ask<'static> {
        static SUB: std::sync::LazyLock<Principal> =
            std::sync::LazyLock::new(|| parse("service:m4n5p6q7r8s9"));
        Ask {
            sub: &SUB,
            client: Client::Wake,
            aud: "store",
            wkl: Some("workload:a1b2c3d4e5f6@wake-1042"),
            life: 300,
        }
    }

    #[test]
    fn an_access_token_is_verified_with_a_trusted_issuer_s_verifiers_alone() {
        let issuer = Issuer::new("service:k3m4n5p6q7r8");
        let token = issuer.mint().token(&wake(), 1_000).unwrap();
        assert!(token.expose().starts_with("lnk_access_"));
        let checked = Trust::new(issuer.issuers(), "store")
            .check(token.expose(), 1_100)
            .unwrap();
        assert_eq!(checked.issuer(), &issuer.me);
        assert_eq!(checked.principal().as_str(), "service:m4n5p6q7r8s9");
        assert_eq!(checked.client(), Client::Wake);
        assert_eq!(checked.via(), None);
        assert_eq!(checked.run(), Some("wake-1042"));
        assert_eq!(checked.ends(), 1_300);
    }

    #[test]
    fn any_issuer_in_the_set_is_trusted_and_none_outside_it() {
        let one = Issuer::new("service:k3m4n5p6q7r8");
        let two = Issuer::new("service:p6q7r8s9t2v3");
        let both = one.issuers().with(
            two.me.clone(),
            [two.signer.verifier()].into_iter().collect(),
        );
        let trust = Trust::new(both, "store");
        for issuer in [&one, &two] {
            let token = issuer.mint().token(&wake(), 1_000).unwrap();
            assert_eq!(
                trust.check(token.expose(), 1_100).unwrap().issuer(),
                &issuer.me
            );
        }
        let stranger = Issuer::new("service:zzzzzzzzzzzz");
        let token = stranger.mint().token(&wake(), 1_000).unwrap();
        assert_eq!(
            trust.check(token.expose(), 1_100),
            Err(Refused::Jws(link_signer::Refused::UnknownVerifier))
        );
    }

    #[test]
    fn an_issuer_never_speaks_for_another_in_the_set() {
        let one = Issuer::new("service:k3m4n5p6q7r8");
        let two = Issuer::new("service:p6q7r8s9t2v3");
        // One signs, naming two as its issuer.
        let forged = Mint {
            signer: &one.signer,
            issuer: &two.me,
        }
        .token(&wake(), 1_000)
        .unwrap();
        let both = one.issuers().with(
            two.me.clone(),
            [two.signer.verifier()].into_iter().collect(),
        );
        assert_eq!(
            Trust::new(both, "store").check(forged.expose(), 1_100),
            Err(Refused::Issuer)
        );
    }

    #[test]
    fn an_access_token_for_another_audience_or_time_is_refused() {
        let issuer = Issuer::new("service:k3m4n5p6q7r8");
        let token = issuer.mint().token(&wake(), 1_000).unwrap();
        let trust = |aud| Trust::new(issuer.issuers(), aud);
        let check = |trust: &Trust, now| trust.check(token.expose(), now);
        assert_eq!(check(&trust("models"), 1_100), Err(Refused::Audience));
        assert_eq!(check(&trust("store"), 1_300), Err(Refused::Time));
        assert_eq!(check(&trust("store"), 900), Err(Refused::Time));
        assert_eq!(
            check(&trust("store").longest(60), 1_100),
            Err(Refused::TooLong)
        );
    }

    #[test]
    fn a_signature_is_never_an_access_token() {
        let issuer = Issuer::new("service:k3m4n5p6q7r8");
        let signature =
            link_signer::signature::sign(&issuer.signer, "service:m4n5p6q7r8s9", "store", 1_000);
        let trust = Trust::new(issuer.issuers(), "store");
        assert_eq!(
            trust.check(signature.expose(), 1_010),
            Err(Refused::Jws(link_signer::Refused::Malformed)),
            "no prefix"
        );
        let prefixed = format!("{}{}", prefixes().access, signature.expose());
        assert_eq!(
            trust.check(&prefixed, 1_010),
            Err(Refused::Jws(link_signer::Refused::Kind)),
            "not an access token's typ"
        );
    }

    #[test]
    fn no_access_token_lives_longer_than_any_may() {
        let issuer = Issuer::new("service:k3m4n5p6q7r8");
        let ask = Ask {
            life: limits().token_life_max + 1,
            ..wake()
        };
        assert_eq!(issuer.mint().token(&ask, 1_000), Err(Refused::TooLong));
        let ask = Ask {
            aud: "Not An Audience",
            life: 60,
            ..wake()
        };
        assert!(matches!(
            issuer.mint().token(&ask, 1_000),
            Err(Refused::Claims(_))
        ));
    }

    /// Its kind of client says which kind of principal it names, and only
    /// a wake's names a wake.
    #[test]
    fn its_kind_of_client_and_principal_hold_together() {
        let issuer = Issuer::new("service:k3m4n5p6q7r8");
        let user: Principal = parse("user:p6q7r8s9t2v3");
        let service: Principal = parse("service:m4n5p6q7r8s9");
        let ask = |sub, client, wkl| Ask {
            sub,
            client,
            aud: "customers",
            wkl,
            life: 300,
        };
        let refused = |sub, client, wkl| {
            matches!(
                issuer.mint().token(&ask(sub, client, wkl), 1_000),
                Err(Refused::Claims(_))
            )
        };
        assert!(refused(&service, Client::App, None));
        assert!(refused(&user, Client::Service, None));
        assert!(refused(&user, Client::Box, None));
        assert!(refused(&service, Client::Wake, None));
        assert!(refused(
            &service,
            Client::Wake,
            Some("workload:a1b2c3d4e5f6")
        ));
        assert!(refused(
            &service,
            Client::Service,
            Some("workload:a1b2c3d4e5f6@wake-1042")
        ));
        let token = issuer
            .mint()
            .token(&ask(&user, Client::App, None), 1_000)
            .unwrap();
        let checked = Trust::new(issuer.issuers(), "customers")
            .check(token.expose(), 1_100)
            .unwrap();
        assert_eq!(checked.principal(), &user);
        assert_eq!(checked.via(), Some(Place::App));
        assert_eq!(checked.workload(), None);
    }
}

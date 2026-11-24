use super::*;
use crate::types::Principal;

/// Keys whose ids are made take only a key's own id; a key added by
/// its public half gets its own.
#[test]
fn a_checked_keys_ids_are_its_keys() {
    let x = B64.encode([7u8; 32]);
    let good = format!("{}:{x}", key_id(&x));
    assert!(Keys::parse_checked(&good).is_ok());
    assert!(Keys::parse_checked(&format!("chosen:{x}")).is_err());
    assert!(Keys::parse(&format!("chosen:{x}")).is_ok());
    let keys = Keys::default().with(&x).unwrap().with(&x).unwrap();
    assert_eq!(keys.texts().collect::<Vec<_>>(), vec![good]);
    assert!(Keys::default().with("not base64url!").is_err());
}

const ISSUER: &str = "https://accounts.example.com";

fn claims(aud: &str, scope: &str, iat: u64, exp: u64) -> AccessTokenClaims {
    serde_json::from_value(json!({
        "iss": ISSUER,
        "sub": USER,
        "installation": "0aa1b2c3-0000-4000-8000-000000000001",
        "acc": "acct-0123456789abcdef0123456789abcdef",
        "aud": aud,
        "scope": scope,
        "usr": "ada",
        "iat": iat,
        "exp": exp,
    }))
    .unwrap()
}

const TUNNEL: &str = "tunnel.open@tunnel:*";

/// The account's user, whom its installation's signer signs for.
const USER: &str = "user:u1s2e3r4a5d6";

fn expect(asks: Option<(GrantAction, &'static str)>) -> Expect<'static> {
    Expect {
        issuer: ISSUER,
        audience: "local.link",
        need: Need {
            subjects: &[PrincipalKind::User],
            installation: true,
            asks,
        },
        max_life: 900,
    }
}

const OPEN: Option<(GrantAction, &str)> = Some((GrantAction::TunnelOpen, "tunnel:web"));

#[test]
fn a_token_is_taken_by_its_audience_alone() {
    let key = SigningSecret::generate().unwrap();
    let keys = Keys::parse(&format!("k1:{}", key.public_text())).unwrap();
    let good = access_token(&key, "k1", &claims("local.link", TUNNEL, 1_000, 1_300));
    let said = check(good.expose(), &keys, &expect(OPEN), 1_000).unwrap();
    assert_eq!(said.usr.as_str(), "ada");
    assert_eq!(said.sub.as_str(), USER);
    assert_eq!(
        said.installation.unwrap().to_string(),
        "0aa1b2c3-0000-4000-8000-000000000001"
    );
    // Another relay's; one without the grant asked for.
    let other = access_token(&key, "k1", &claims("evil.example", TUNNEL, 1_000, 1_300));
    assert_eq!(
        check(other.expose(), &keys, &expect(OPEN), 1_000).err(),
        Some(Refused::Elsewhere)
    );
    let share = access_token(
        &key,
        "k1",
        &claims("local.link", "share.make@share:*", 1_000, 1_300),
    );
    assert_eq!(
        check(share.expose(), &keys, &expect(OPEN), 1_000).err(),
        Some(Refused::Scope)
    );
    // Another kind of subject, with every grant: not this checker's.
    for sub in ["service:m0d3l5s3rv1c", "service:a1b2c3d4e5f6"] {
        let mut theirs = claims("local.link", "*", 1_000, 1_300);
        theirs.sub = sub.parse().unwrap();
        let token = access_token(&key, "k1", &theirs);
        assert_eq!(
            check(token.expose(), &keys, &expect(OPEN), 1_000).err(),
            Some(Refused::Scope),
            "{sub}"
        );
        assert_eq!(
            check(token.expose(), &keys, &expect(None), 1_000).err(),
            Some(Refused::Scope),
            "{sub}"
        );
    }
    // The user's own, asked for by no installation: a visitor's.
    let mut visitor = claims("local.link", "*", 1_000, 1_300);
    visitor.installation = None;
    let token = access_token(&key, "k1", &visitor);
    assert_eq!(
        check(token.expose(), &keys, &expect(OPEN), 1_000).err(),
        Some(Refused::Scope)
    );
    // An agent's service's, taken where services are, never naming a
    // installation.
    let services = Expect {
        need: Need {
            subjects: &[PrincipalKind::Service],
            installation: false,
            asks: None,
        },
        ..expect(None)
    };
    let mut agent = claims("local.link", "", 1_000, 1_300);
    agent.sub = "service:a1b2c3d4e5f6".parse().unwrap();
    let named = access_token(&key, "k1", &agent);
    assert_eq!(
        check(named.expose(), &keys, &services, 1_000).err(),
        Some(Refused::Scope)
    );
    agent.installation = None;
    let own = access_token(&key, "k1", &agent);
    assert!(check(own.expose(), &keys, &services, 1_000).is_ok());
    // One that only says who it is: taken where nothing is asked.
    let identify = access_token(&key, "k1", &claims("local.link", "", 1_000, 1_300));
    assert!(check(identify.expose(), &keys, &expect(None), 1_000).is_ok());
    assert_eq!(
        check(identify.expose(), &keys, &expect(OPEN), 1_000).err(),
        Some(Refused::Scope)
    );
    // Expired, not yet over at its last second, too long a life.
    assert_eq!(
        check(good.expose(), &keys, &expect(OPEN), 1_300).err(),
        Some(Refused::Expired)
    );
    let long = access_token(&key, "k1", &claims("local.link", TUNNEL, 1_000, 1_901));
    assert_eq!(
        check(long.expose(), &keys, &expect(OPEN), 1_000).err(),
        Some(Refused::Expired)
    );
    let backdated = access_token(&key, "k1", &claims("local.link", TUNNEL, 0, 1_300));
    assert_eq!(
        check(backdated.expose(), &keys, &expect(OPEN), 1_000).err(),
        Some(Refused::Expired)
    );
}

#[test]
fn a_grant_of_an_action_unknown_is_skipped() {
    let key = SigningSecret::generate().unwrap();
    let keys = Keys::parse(&format!("k1:{}", key.public_text())).unwrap();
    let newer = claims(
        "local.link",
        "bucket.write@bucket:b7/team/* tunnel.open@tunnel:* someday.verb",
        1_000,
        1_300,
    );
    let token = access_token(&key, "k1", &newer);
    assert!(check(token.expose(), &keys, &expect(OPEN), 1_000).is_ok());
    let none_known = claims("local.link", "bucket.write@bucket:b7", 1_000, 1_300);
    let token = access_token(&key, "k1", &none_known);
    assert_eq!(
        check(token.expose(), &keys, &expect(OPEN), 1_000).err(),
        Some(Refused::Scope)
    );
}

/// The spec's cases (`Grants`' `x-allows`), which the app's
/// checker runs too.
#[test]
fn grants_allow_as_the_spec_says() {
    let spec = spec();
    let cases = spec["components"]["schemas"]["Grants"]["x-allows"]
        .as_array()
        .expect("the spec's cases");
    assert!(cases.len() >= 10);
    for case in cases {
        let grants: Grants = case["scope"].as_str().unwrap().parse().unwrap();
        let action: GrantAction = case["action"].as_str().unwrap().parse().unwrap();
        let resource = case["resource"].as_str().unwrap();
        assert_eq!(
            grants.allows(action, resource),
            case["allowed"].as_bool().unwrap(),
            "{case}"
        );
    }
}

/// A token's time, as every checker reads it: no allowance for clocks
/// unless one is given.
#[test]
fn a_token_is_timely_within_its_life() {
    // Made at 1,000 for 300 seconds, a checker's longest 900.
    assert_eq!(timely(1_000, 1_300, 1_000, 900, 0), Ok(()));
    assert_eq!(timely(1_000, 1_300, 1_299, 900, 0), Ok(()));
    assert_eq!(timely(1_000, 1_300, 1_300, 900, 0), Err(Refused::Expired));
    // Its clock ahead of the checker's: taken while it ends within
    // the longest life from now.
    assert_eq!(timely(1_000, 1_300, 400, 900, 0), Ok(()));
    assert_eq!(timely(1_000, 1_300, 399, 900, 0), Err(Refused::Expired));
    // Made to last too long, or to end before it's made.
    assert_eq!(timely(1_000, 1_901, 1_000, 900, 0), Err(Refused::Expired));
    assert_eq!(timely(1_300, 1_000, 1_000, 900, 0), Err(Refused::Expired));
    // With an allowance for clocks, either way by that much.
    assert_eq!(timely(1_000, 1_300, 1_419, 900, 120), Ok(()));
    assert_eq!(timely(1_000, 1_300, 1_420, 900, 120), Err(Refused::Expired));
    assert_eq!(timely(1_000, 1_300, 280, 900, 120), Ok(()));
    assert_eq!(timely(1_000, 1_300, 279, 900, 120), Err(Refused::Expired));
    assert_eq!(timely(0, u64::MAX, 0, u64::MAX, u64::MAX), Ok(()));
}

#[test]
fn a_refreshed_token_does_not_renew_original_authorization() {
    let mut old = claims("dispatch.test", "team.member@team:t4m9q2x7k1ab", 200, 500);
    old.auth_time = Some(100);
    let list: crate::types::StoppedAccounts = serde_json::from_value(serde_json::json!({
        "accounts": [{"account": old.acc, "at": 150}],
    }))
    .unwrap();
    assert!(
        stopped(&list, &old),
        "refresh must not erase the binding's original authority"
    );
    let cache = crate::stopped::Cache::default();
    cache.set_at(list, 200);
    assert!(cache.refuses(&old));
    old.auth_time = None;
    assert!(
        !cache.refuses(&old),
        "a token without auth_time authorizes at its iat"
    );
    old.auth_time = Some(200);
    assert!(
        !cache.refuses(&old),
        "new authorization is valid after the cut"
    );
}

#[test]
fn each_scope_is_given_the_specs_grants() {
    let spec = spec();
    let grants = spec["components"]["schemas"]["Scope"]["x-grants"]
        .as_object()
        .expect("the spec's grants");
    let scopes = [
        Scope::Tunnel,
        Scope::Share,
        Scope::Visit,
        Scope::Api,
        Scope::Identify,
        Scope::Agent,
    ];
    assert_eq!(grants.len(), scopes.len());
    for scope in scopes {
        assert_eq!(
            Some(scope.grants().as_str()),
            grants[&scope.to_string()].as_str(),
            "{scope}"
        );
    }
}

/// The kinds a principal's pattern names are the spec's
/// `PrincipalKind`, one for one, so [`Principal::kind`] reads every
/// principal the pattern lets in.
#[test]
fn a_principals_kinds_are_the_specs() {
    let spec = spec();
    let schemas = &spec["components"]["schemas"];
    let pattern = schemas["Principal"]["pattern"].as_str().unwrap();
    let named: Vec<&str> = pattern
        .strip_prefix("^(")
        .and_then(|p| p.split_once(')'))
        .map(|(kinds, _)| kinds.split('|').collect())
        .expect("the pattern's kinds");
    let kinds: Vec<&str> = schemas["PrincipalKind"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .collect();
    assert_eq!(named, kinds);
    // link-principal's grammar, which these types are, is the spec's.
    assert_eq!(kinds, link_principal::KINDS);
    let limits = link_id::limits();
    assert_eq!(
        pattern,
        format!(
            "^({}):[{}]{{{}}}$",
            link_principal::KINDS.join("|"),
            "a-z0-9",
            limits.candidate_length
        )
    );
    assert_eq!(
        limits.candidate_alphabet,
        "abcdefghijklmnopqrstuvwxyz0123456789"
    );
    for kind in kinds {
        let principal: Principal = format!("{kind}:k3m4n5p6q7r8").parse().unwrap();
        assert_eq!(principal.kind().to_string(), kind);
    }
    // An id Link minted, nothing else.
    for not in [
        "member:slack:T0ACME/U0ALICE",
        "user:0aa1b2c3-0000-4000-8000-0000000000aa",
        "service:app",
        "user:",
        "user:a b",
        "installation:@x",
    ] {
        assert!(not.parse::<Principal>().is_err(), "{not}");
    }
}

fn spec() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/account.yaml");
    serde_norway::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn claims_are_the_specs_alone() {
    let key = SigningSecret::generate().unwrap();
    let keys = Keys::parse(&format!("k1:{}", key.public_text())).unwrap();
    let header = json!({"alg": "EdDSA", "typ": "at+jwt", "kid": "k1"});
    let good = serde_json::to_value(claims("local.link", TUNNEL, 1_000, 1_300)).unwrap();
    let refused = |claims: &serde_json::Value| {
        let token = sign(&key, &header, claims);
        check(token.expose(), &keys, &expect(OPEN), 1_000).err()
    };
    assert_eq!(refused(&good), None);
    // Claims of another shape: the account in `sub`, `mch`, one scope.
    let mut old = good.clone();
    old.as_object_mut().unwrap().remove("acc");
    old["sub"] = json!("acct-0123456789abcdef0123456789abcdef");
    old["mch"] = json!("0aa1b2c3-0000-4000-8000-000000000001");
    old["scope"] = json!("tunnel");
    assert_eq!(refused(&old), Some(Refused::Invalid));
    for gone in ["sub", "acc"] {
        let mut without = good.clone();
        without.as_object_mut().unwrap().remove(gone);
        assert_eq!(refused(&without), Some(Refused::Invalid), "{gone}");
    }
    for (claim, value) in [
        ("auth_time", json!(1_001)),
        ("sub", json!("root")),
        ("sub", json!("robot:1")),
        (
            "sub",
            json!("installation:0aa1b2c3-0000-4000-8000-000000000001"),
        ),
        ("sub", json!("agent:a1b2c3d4e5f6")),
        ("installation", json!("laptop")),
        ("sub", json!("acct-0123456789abcdef0123456789abcdef")),
        (
            "acc",
            json!("installation:0aa1b2c3-0000-4000-8000-000000000001"),
        ),
        (
            "prn",
            json!("installation:0aa1b2c3-0000-4000-8000-000000000001"),
        ),
        ("scope", json!("tunnel.open  share.make")),
        ("scope", json!(" tunnel.open")),
        ("scope", json!("Tunnel.Open")),
        ("scope", json!(["tunnel.open"])),
        ("act", json!("service:m0d3l5s3rv1c")),
        (
            "act",
            json!({"sub": "service:m0d3l5s3rv1c", "act": {"sub": "service:x1x2x3x4x5x6"}}),
        ),
    ] {
        let mut bad = good.clone();
        bad[claim] = value.clone();
        assert_eq!(refused(&bad), Some(Refused::Invalid), "{claim}: {value}");
    }
    // `act` is read, and set by no one yet.
    let mut acting = good.clone();
    acting["act"] = json!({"sub": "service:m0d3l5s3rv1c"});
    let token = sign(&key, &header, &acting);
    let read = check(token.expose(), &keys, &expect(OPEN), 1_000).unwrap();
    assert_eq!(read.act.unwrap().sub.as_str(), "service:m0d3l5s3rv1c");
}

#[test]
fn only_an_ed25519_token_of_a_key_held_is_read() {
    let key = SigningSecret::generate().unwrap();
    let keys = Keys::parse(&format!("k1:{}", key.public_text())).unwrap();
    let check = |token: &str| check(token, &keys, &expect(OPEN), 1_000).err();
    let good = access_token(&key, "k1", &claims("local.link", TUNNEL, 1_000, 1_300));
    let (header, rest) = good.expose().split_once('.').unwrap();
    let (body, signature) = rest.split_once('.').unwrap();
    let swap = |header: serde_json::Value| {
        format!("{}.{body}.{signature}", B64.encode(header.to_string()))
    };
    for header in [
        json!({"alg": "none", "typ": "at+jwt", "kid": "k1"}),
        json!({"alg": "HS256", "typ": "at+jwt", "kid": "k1"}),
        json!({"alg": "EdDSA", "typ": "JWT", "kid": "k1"}),
        json!({"alg": "EdDSA", "typ": "at+jwt"}),
        json!({"alg": "EdDSA", "typ": "at+jwt", "kid": "k2"}),
        json!({"alg": "EdDSA", "typ": "at+jwt", "kid": "k1", "jku": "https://evil.example"}),
    ] {
        assert_eq!(
            check(&swap(header.clone())),
            Some(Refused::Invalid),
            "{header}"
        );
    }
    // Unsigned, signed by another key, its claims changed.
    assert_eq!(check(&format!("{header}.{body}.")), Some(Refused::Invalid));
    let stranger = SigningSecret::generate().unwrap();
    let forged = access_token(&stranger, "k1", &claims("local.link", TUNNEL, 1_000, 1_300));
    assert_eq!(check(forged.expose()), Some(Refused::Invalid));
    let mut claims = serde_json::to_value(claims("local.link", TUNNEL, 1_000, 1_300)).unwrap();
    claims["usr"] = json!("bob");
    let tampered = format!("{header}.{}.{signature}", B64.encode(claims.to_string()));
    assert_eq!(check(&tampered), Some(Refused::Invalid));
    // A claim it doesn't know, signed: refused all the same.
    claims["usr"] = json!("ada");
    claims["admin"] = json!(true);
    let extra = sign(
        &key,
        &json!({"alg": "EdDSA", "typ": "at+jwt", "kid": "k1"}),
        &claims,
    );
    assert_eq!(check(extra.expose()), Some(Refused::Invalid));
    // Made up ones, and those of no other kind.
    let installation = format!("lnk_{}", "ab".repeat(24));
    for made_up in [
        "",
        "x",
        "a.b",
        "a.b.c.d",
        &"a.".repeat(3000),
        &installation,
        "lnkr_e30.c2ln",
    ] {
        assert_eq!(check(made_up), Some(Refused::Invalid), "{made_up}");
    }
}

#[test]
fn an_assertion_is_the_installations_for_one_request() {
    let key = SigningSecret::generate().unwrap();
    let id: InstallationId = "0aa1b2c3-0000-4000-8000-000000000001".parse().unwrap();
    let account: Owner = "acct-0123456789abcdef0123456789abcdef".parse().unwrap();
    let installation = Installation {
        id: &id,
        account: &account,
        key: &key,
    };
    let one = assertion(
        &installation,
        "https://accounts.example.com/",
        "local.link",
        Scope::Tunnel,
        1_000,
    );
    let two = assertion(
        &installation,
        "https://accounts.example.com",
        "local.link",
        Scope::Tunnel,
        1_000,
    );
    let parts: Vec<&str> = one.expose().split('.').collect();
    let header: crate::types::InstallationAssertionHeader = read(parts[0]).unwrap();
    assert_eq!(
        serde_json::to_value(header).unwrap(),
        json!({"alg": "EdDSA", "typ": "installation+jwt"})
    );
    let claims: crate::types::InstallationAssertionClaims = read(parts[1]).unwrap();
    assert_eq!(claims.aud.as_str(), "https://accounts.example.com");
    assert_eq!((claims.iat, claims.exp), (1_000, 1_060));
    assert_eq!(claims.iss.to_string(), id.to_string());
    let signature = B64.decode(parts[2]).unwrap();
    let input = format!("{}.{}", parts[0], parts[1]);
    assert!(link_secretive::signing::verify(
        &key.public(),
        input.as_bytes(),
        &signature
    ));
    // Each its own: never the same twice.
    let other: crate::types::InstallationAssertionClaims =
        read(two.expose().split('.').nth(1).unwrap()).unwrap();
    assert_ne!(claims.jti.as_str(), other.jti.as_str());
    assert_eq!(format!("{one:?}"), "<redacted>");
}

#[test]
fn an_agents_assertion_names_it_and_its_grants() {
    let key = SigningSecret::generate().unwrap();
    let id: InstallationId = "0aa1b2c3-0000-4000-8000-000000000001".parse().unwrap();
    let account: Owner = "acct-0123456789abcdef0123456789abcdef".parse().unwrap();
    let installation = Installation {
        id: &id,
        account: &account,
        key: &key,
    };
    let agent: AgentId = "a1b2c3d4e5f6".parse().unwrap();
    let grants: Grants = "team.member@team:r7".parse().unwrap();
    let made = agent_assertion(
        &installation,
        "https://accounts.example.com",
        "dispatch.example.com",
        &agent,
        &grants,
        1_000,
    );
    let claims: crate::types::InstallationAssertionClaims =
        read(made.expose().split('.').nth(1).unwrap()).unwrap();
    assert_eq!(claims.scope, Scope::Agent);
    assert_eq!(
        claims.agent.as_ref().map(|a| a.as_str()),
        Some("a1b2c3d4e5f6")
    );
    assert_eq!(
        claims.grants.as_ref().map(|g| g.as_str()),
        Some("team.member@team:r7")
    );
    // An installation's own names none.
    let own = assertion(
        &installation,
        "https://accounts.example.com",
        "x.example.com",
        Scope::Identify,
        1_000,
    );
    let claims: crate::types::InstallationAssertionClaims =
        read(own.expose().split('.').nth(1).unwrap()).unwrap();
    assert!(claims.agent.is_none() && claims.grants.is_none());
    assert!("A1b2c3d4e5f6".parse::<AgentId>().is_err());
}

#[test]
fn a_stopped_accounts_tokens_made_before_it_stopped_are_refused() {
    let claims = |acc: &str, iat: u64| -> AccessTokenClaims {
        serde_json::from_value(json!({
            "iss": "https://accounts.example.com",
            "sub": USER,
            "installation": "0aa1b2c3-0000-4000-8000-000000000001",
            "acc": acc,
            "aud": "local.link",
            "scope": "",
            "usr": "ada",
            "iat": iat,
            "exp": iat + 300,
        }))
        .unwrap()
    };
    const ADA: &str = "acct-0123456789abcdef0123456789abcdef";
    const BOB: &str = "acct-fedcba9876543210fedcba9876543210";
    let list: crate::types::StoppedAccounts =
        serde_json::from_value(json!({"accounts": [{"account": ADA, "at": 1_000}]})).unwrap();
    assert!(stopped(&list, &claims(ADA, 999)));
    assert!(!stopped(&list, &claims(ADA, 1_000)));
    assert!(!stopped(&list, &claims(BOB, 999)));
    let none: crate::types::StoppedAccounts =
        serde_json::from_value(json!({"accounts": []})).unwrap();
    assert!(!stopped(&none, &claims(ADA, 1)));
}

#[test]
fn a_keys_id_is_made_from_its_public_half() {
    let key = SigningSecret::generate().unwrap();
    let id = key_id(&key.public_text());
    assert_eq!(id.len(), 16);
    assert_eq!(id, key_id(&key.public_text()));
    let other = SigningSecret::generate().unwrap();
    assert_ne!(id, key_id(&other.public_text()));
    // As the account service makes it: base64url(SHA-256(x)), cut.
    assert_eq!(key_id("x"), "LXEWQrcmsEQBYnyp");
}

#[test]
fn keys_are_read_with_their_ids() {
    let key = SigningSecret::generate().unwrap();
    let keys = Keys::parse(&format!(" k1:{x}, k-2:{x}\n", x = key.public_text())).unwrap();
    assert_eq!(keys.0.len(), 2);
    assert!(Keys::parse("").is_err());
    assert!(
        Keys::parse(&key.public_text()).is_err(),
        "a key without its id"
    );
    assert!(Keys::parse("k1:c2hvcnQ").is_err());
    assert!(Keys::parse(&format!("k.1:{}", key.public_text())).is_err());
}

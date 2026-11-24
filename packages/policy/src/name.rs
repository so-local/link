//! The names IAM checks: accounts, principals, the action list and
//! resources, each parsed once, strictly, and printed back as written.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// What every name holds after its kind, `[A-Za-z0-9._:/-]{1,128}`: one
/// opaque grammar, so an id never needs quoting or escaping.
fn opaque(text: &str) -> bool {
    (1..=128).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_:/".contains(&b))
}

/// What a refused name is said as: printable, and never long.
fn said(text: &str) -> String {
    let short: String = text.chars().take(80).collect();
    format!("{short:?}")
}

macro_rules! strings {
    ($($name:ident),*) => {$(
        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                text.parse()
            }
        }
        impl From<$name> for String {
            fn from(name: $name) -> String {
                name.to_string()
            }
        }
    )*};
}

/// An account: whose resources a name without an account means.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AccountId(String);

impl FromStr for AccountId {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        match text.strip_prefix("acct-") {
            Some(id) if opaque(id) && !id.contains('/') => Ok(AccountId(text.into())),
            _ => Err(format!("{} isn't an account: acct-<id>", said(text))),
        }
    }
}

impl AccountId {
    /// The account of a machine's one owner where there's no account
    /// service (Link here, `<user>@<machine>`): `acct-here`, whose
    /// resources are everything on that machine. No account service
    /// names one so: its ids are `acct-<32 hex>`.
    pub fn here() -> AccountId {
        AccountId("acct-here".into())
    }

    /// An owner's account: its id for an account's (`acct-<id>`), else
    /// [`AccountId::here`].
    pub fn of_owner(owner: &str) -> AccountId {
        owner.parse().unwrap_or_else(|_| AccountId::here())
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A principal's permanent id, `<kind>:<id>`: the one type every side
/// names a principal with ([`link_principal`]), each kind proving itself
/// its own way, outside this crate.
pub use link_principal::{Principal as PrincipalId, PrincipalKind};

/// A kind of resource, and its verbs: the one action list. A kind joins
/// it with the slice that checks it, reviewed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// An agent, wherever it runs.
    Agent,
    /// A team: agents, people or both, and the chats it talks in.
    Team,
    /// A cloud an account connected, or a computer it accepted as one.
    Cloud,
    /// A box, on a cloud.
    Box,
    /// A bucket, or a path in it (`bucket:<id>/<path>`): an agent's Home,
    /// a team's folder.
    Bucket,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Agent,
        Kind::Team,
        Kind::Cloud,
        Kind::Box,
        Kind::Bucket,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Agent => "agent",
            Kind::Team => "team",
            Kind::Cloud => "cloud",
            Kind::Box => "box",
            Kind::Bucket => "bucket",
        }
    }

    /// The kind's verbs: what's read, what's used, what changes it.
    pub fn verbs(self) -> &'static [&'static str] {
        match self {
            Kind::Agent => &["read", "message", "admin"],
            Kind::Team => &["read", "member", "admin"],
            Kind::Cloud => &["read", "use", "admin"],
            Kind::Box => &["read", "ssh", "admin"],
            Kind::Bucket => &["read", "write", "share"],
        }
    }

    fn named(text: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == text)
    }

    fn kinds() -> String {
        let kinds: Vec<_> = Kind::ALL.iter().map(|k| k.as_str()).collect();
        kinds.join(", ")
    }
}

/// One action: `<kind>.<verb>`, from the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Action {
    kind: Kind,
    verb: &'static str,
}

impl Action {
    pub fn kind(self) -> Kind {
        self.kind
    }

    pub fn verb(self) -> &'static str {
        self.verb
    }

    /// Every action there is, kind by kind.
    pub fn all() -> impl Iterator<Item = Action> {
        Kind::ALL
            .into_iter()
            .flat_map(|kind| kind.verbs().iter().map(move |&verb| Action { kind, verb }))
    }

    /// `kind`'s `verb`, if it has one by that name.
    pub fn of(kind: Kind, verb: &str) -> Option<Action> {
        let verb = kind.verbs().iter().find(|v| **v == verb)?;
        Some(Action { kind, verb })
    }
}

impl FromStr for Action {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let (kind, verb) = text
            .split_once('.')
            .ok_or_else(|| format!("{} isn't an action: <kind>.<verb>", said(text)))?;
        let kind = Kind::named(kind).ok_or_else(|| {
            format!(
                "{} isn't an action: no kind {}; the kinds are {}",
                said(text),
                said(kind),
                Kind::kinds()
            )
        })?;
        Action::of(kind, verb).ok_or_else(|| {
            format!(
                "{} isn't an action: a {}'s verbs are {}",
                said(text),
                kind.as_str(),
                kind.verbs().join(", ")
            )
        })
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.kind.as_str(), self.verb)
    }
}

/// The actions a grant names: `*`, every action; `<kind>.*`, every verb
/// of a kind; or one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Actions {
    All,
    Kind(Kind),
    One(Action),
}

impl Actions {
    pub fn covers(self, action: Action) -> bool {
        match self {
            Actions::All => true,
            Actions::Kind(kind) => action.kind == kind,
            Actions::One(one) => one == action,
        }
    }

    /// The one kind these are of, unless every kind.
    pub fn kind(self) -> Option<Kind> {
        match self {
            Actions::All => None,
            Actions::Kind(kind) => Some(kind),
            Actions::One(action) => Some(action.kind),
        }
    }
}

impl FromStr for Actions {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        if text == "*" {
            return Ok(Actions::All);
        }
        match text.strip_suffix(".*") {
            Some(kind) => Kind::named(kind).map(Actions::Kind).ok_or_else(|| {
                format!(
                    "{} isn't an action: no kind {}; the kinds are {}",
                    said(text),
                    said(kind),
                    Kind::kinds()
                )
            }),
            None => text.parse().map(Actions::One),
        }
    }
}

impl fmt::Display for Actions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Actions::All => f.write_str("*"),
            Actions::Kind(kind) => write!(f, "{}.*", kind.as_str()),
            Actions::One(action) => action.fmt(f),
        }
    }
}

/// A resource, by its id: `[<account>/]<kind>:<id>`. Without an account,
/// it's the asking principal's own.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ResourceName {
    account: Option<AccountId>,
    kind: Kind,
    id: String,
}

impl ResourceName {
    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn account(&self) -> Option<&AccountId> {
        self.account.as_ref()
    }
}

/// `text`'s account, if it names one before a `/`, and the rest.
fn account_and(text: &str) -> (Option<&str>, &str) {
    match text.split_once('/') {
        Some((account, rest)) if account == "*" || account.starts_with("acct-") => {
            (Some(account), rest)
        }
        _ => (None, text),
    }
}

/// `<kind>:<id>`'s kind and id, the id checked by `id_ok`.
fn kind_and_id<'a>(
    text: &str,
    rest: &'a str,
    id_ok: impl Fn(&str) -> bool,
) -> Result<(Kind, &'a str), String> {
    let (kind, id) = rest
        .split_once(':')
        .ok_or_else(|| format!("{} isn't a resource: <kind>:<id>", said(text)))?;
    let kind = Kind::named(kind).ok_or_else(|| {
        format!(
            "{} isn't a resource: no kind {}; the kinds are {}",
            said(text),
            said(kind),
            Kind::kinds()
        )
    })?;
    if !id_ok(id) {
        return Err(format!("{} isn't a resource: its id isn't one", said(text)));
    }
    Ok((kind, id))
}

impl FromStr for ResourceName {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let (account, rest) = account_and(text);
        let account = match account {
            Some("*") => {
                return Err(format!("{} isn't a resource: * is a pattern's", said(text)));
            }
            Some(account) => Some(account.parse()?),
            None => None,
        };
        let (kind, id) = kind_and_id(text, rest, |id| opaque(id) && !id.contains('*'))?;
        Ok(ResourceName {
            account,
            kind,
            id: id.into(),
        })
    }
}

impl fmt::Display for ResourceName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(account) = &self.account {
            write!(f, "{account}/")?;
        }
        write!(f, "{}:{}", self.kind.as_str(), self.id)
    }
}

/// Whose resources a pattern reaches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Accounts {
    /// The principal's own: a pattern that names none.
    Own,
    /// Any account's: `*/`.
    Any,
    /// One account's.
    One(AccountId),
}

/// The resources a grant names: `*`, every one of the principal's own
/// account; or `[<account>|*/]<kind>:<id>`, its id whole or ending in
/// `*`, every id it begins. Nothing else is a pattern, so a grant reads
/// as what it reaches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Resources {
    All,
    Of {
        accounts: Accounts,
        kind: Kind,
        id: String,
    },
}

impl Resources {
    /// Whether these reach `name`, for a principal of `own` account.
    pub fn covers(&self, name: &ResourceName, own: &AccountId) -> bool {
        let account = name.account.as_ref().unwrap_or(own);
        match self {
            Resources::All => account == own,
            Resources::Of { accounts, kind, id } => {
                let accounts = match accounts {
                    Accounts::Own => account == own,
                    Accounts::Any => true,
                    Accounts::One(one) => account == one,
                };
                let id = match id.strip_suffix('*') {
                    Some(begins) => name.id.starts_with(begins),
                    None => name.id == *id,
                };
                accounts && *kind == name.kind && id
            }
        }
    }

    /// Whether these reach every resource `other` reaches, for a
    /// principal of `own` account: a pattern held to a pattern, never to
    /// one name it happens to cover.
    pub fn reaches_all(&self, other: &Resources, own: &AccountId) -> bool {
        let (
            Resources::Of { accounts, kind, id },
            Resources::Of {
                accounts: theirs,
                kind: of,
                id: their_id,
            },
        ) = (self, other)
        else {
            // Every resource of the account reaches all of its own alone.
            return match (self, other) {
                (Resources::All, Resources::All) => true,
                (Resources::All, Resources::Of { accounts, .. }) => match accounts {
                    Accounts::Own => true,
                    Accounts::One(one) => one == own,
                    Accounts::Any => false,
                },
                _ => false,
            };
        };
        let accounts = match (accounts, theirs) {
            (Accounts::Any, _) => true,
            (Accounts::Own, Accounts::Own) => true,
            (Accounts::Own, Accounts::One(one)) | (Accounts::One(one), Accounts::Own) => one == own,
            (Accounts::One(a), Accounts::One(b)) => a == b,
            (_, Accounts::Any) => false,
        };
        let ids = match id.strip_suffix('*') {
            Some(begins) => their_id.trim_end_matches('*').starts_with(begins),
            None => !their_id.ends_with('*') && id == their_id,
        };
        accounts && kind == of && ids
    }

    /// Whether its `*`, if it has one, ends a segment: `*` alone, or after
    /// a `/` (`bucket:team-r7/*`), so it never reaches an id that only
    /// begins as another does (`team:r7*`, `team:r70`).
    pub fn ends_segments(&self) -> bool {
        match self {
            Resources::All => true,
            Resources::Of { id, .. } => match id.strip_suffix('*') {
                Some(begins) => begins.is_empty() || begins.ends_with('/'),
                None => true,
            },
        }
    }

    /// The one kind these are of, unless every kind.
    pub fn kind(&self) -> Option<Kind> {
        match self {
            Resources::All => None,
            Resources::Of { kind, .. } => Some(*kind),
        }
    }
}

impl FromStr for Resources {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        if text == "*" {
            return Ok(Resources::All);
        }
        let (account, rest) = account_and(text);
        let accounts = match account {
            None => Accounts::Own,
            Some("*") => Accounts::Any,
            Some(account) => Accounts::One(account.parse()?),
        };
        let (kind, id) = kind_and_id(text, rest, |id| {
            let whole = id.strip_suffix('*').unwrap_or(id);
            !whole.contains('*') && (whole.is_empty() || opaque(whole))
        })?;
        Ok(Resources::Of {
            accounts,
            kind,
            id: id.into(),
        })
    }
}

impl fmt::Display for Resources {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Resources::All => f.write_str("*"),
            Resources::Of { accounts, kind, id } => {
                match accounts {
                    Accounts::Own => {}
                    Accounts::Any => f.write_str("*/")?,
                    Accounts::One(account) => write!(f, "{account}/")?,
                }
                write!(f, "{}:{id}", kind.as_str())
            }
        }
    }
}

strings!(AccountId, Action, Actions, ResourceName, Resources);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_back_as_written() {
        for text in ["acct-7f3a", "acct-a.b_c"] {
            assert_eq!(text.parse::<AccountId>().unwrap().to_string(), text);
        }
        for text in ["user:0d1eh9w5ecd7", "service:scoutsimjo7n"] {
            assert_eq!(text.parse::<PrincipalId>().unwrap().to_string(), text);
        }
        for text in ["cloud:aws", "acct-b/box:b1"] {
            assert_eq!(text.parse::<ResourceName>().unwrap().to_string(), text);
        }
        for text in ["*", "cloud:*", "*/cloud:*", "acct-b/cloud:ci-*", "box:b1"] {
            assert_eq!(text.parse::<Resources>().unwrap().to_string(), text);
        }
        for text in ["*", "cloud.*", "box.ssh"] {
            assert_eq!(text.parse::<Actions>().unwrap().to_string(), text);
        }
    }

    #[test]
    fn what_isnt_a_name_is_refused() {
        for text in ["", "acct-", "acct-a/b", "7f3a"] {
            assert!(text.parse::<AccountId>().is_err(), "{text}");
        }
        for text in [
            "scout",
            "service:",
            "service:a b",
            "service:s/1",
            "service:scout",
            "robot:x",
            "agent:scout",
            "installation:m1",
        ] {
            assert!(text.parse::<PrincipalId>().is_err(), "{text}");
        }
        for text in ["cloud", "cloud:a*", "*/cloud:a", "tunnel:b", "cloud:"] {
            assert!(text.parse::<ResourceName>().is_err(), "{text}");
        }
        for text in ["cloud:a*b", "cloud:**", "**", "cloud", "tunnel:*"] {
            assert!(text.parse::<Resources>().is_err(), "{text}");
        }
        for text in [
            "cloud",
            "cloud.ssh",
            "models.invoke",
            "*.read",
            ".*",
            "bucket.use",
        ] {
            assert!(text.parse::<Actions>().is_err(), "{text}");
        }
    }

    #[test]
    fn every_action_is_listed_once() {
        let all: Vec<_> = Action::all().map(|a| a.to_string()).collect();
        assert_eq!(
            all,
            [
                "agent.read",
                "agent.message",
                "agent.admin",
                "team.read",
                "team.member",
                "team.admin",
                "cloud.read",
                "cloud.use",
                "cloud.admin",
                "box.read",
                "box.ssh",
                "box.admin",
                "bucket.read",
                "bucket.write",
                "bucket.share"
            ]
        );
    }
}

//! Who acts, or may: a principal, `user:<id>` or `service:<id>`, as the
//! account service's API names it (`docs/cloud/api/openapi.yaml`'s
//! `Principal` and `PrincipalKind`). One definition for an access token's
//! `sub` (`link-api`, whose generated types are these), a role's holder
//! and a teammate's sender; link-api's test holds its grammar to the
//! spec's pattern. A machine, a chat, a webhook's caller and a guest are
//! never principals: a machine's signer signs for a user, a chat is a
//! group, a webhook acts as a service of its own and a guest holds a
//! capability.

use std::fmt;
use std::str::FromStr;

use link_id::{Id, Kind};
use serde::{Deserialize, Serialize};

/// The kinds of principal, as they're written before the first `:`.
pub const KINDS: &[&str] = &["user", "service"];

pub use link_id::NotOne;

/// A principal's kind: what comes before its first `:`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrincipalKind {
    /// One human: you, or someone a connection vouches for.
    User,
    /// A program acting on its own: an agent's, a box's, CI's. Each has
    /// an id of its own, minted apart from what it acts for: an agent's
    /// service is `service:<id>`, never its agent ID.
    Service,
}

impl PrincipalKind {
    const ALL: [Self; 2] = [Self::User, Self::Service];

    /// As it's written ([`KINDS`]).
    pub fn as_str(self) -> &'static str {
        KINDS[self as usize]
    }
}

impl fmt::Display for PrincipalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Kind for PrincipalKind {
    const NOUN: &'static str = "principal";
    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
    fn as_str(&self) -> &str {
        PrincipalKind::as_str(*self)
    }
}

impl FromStr for PrincipalKind {
    type Err = NotOne;
    fn from_str(text: &str) -> Result<Self, NotOne> {
        link_id::kind(text)
    }
}

impl TryFrom<&str> for PrincipalKind {
    type Error = NotOne;
    fn try_from(text: &str) -> Result<Self, NotOne> {
        text.parse()
    }
}

impl TryFrom<String> for PrincipalKind {
    type Error = NotOne;
    fn try_from(text: String) -> Result<Self, NotOne> {
        text.parse()
    }
}

/// A principal, `<kind>:<id>`: a kind of [`KINDS`], then an id Link
/// minted (`link_id::is_id`): `link-id`'s one generic.
pub type Principal = Id<PrincipalKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_principal_is_a_user_or_a_service() {
        for kind in KINDS {
            let principal: Principal = format!("{kind}:k3m4n5p6q7r8").parse().unwrap();
            assert_eq!(principal.kind().as_str(), *kind);
            assert_eq!(principal.kind().to_string(), *kind);
        }
        let agent: Principal = "service:a1b2c3d4e5f6".parse().unwrap();
        assert_eq!(agent.kind(), PrincipalKind::Service);
        assert_eq!(agent.id(), "a1b2c3d4e5f6");
        for not in [
            "installation:a1b2c3d4e5f6",
            "agent:a1b2c3d4e5f6",
            "api-key:a1b2c3d4e5f6",
            "chat:slack:C0TEAM",
            "webhook:a1b2c3d4e5f6",
            "guest:a1b2c3d4e5f6",
            "user:",
            "user:a b",
            "user:alice",
            "user:0aa1b2c3-0000-4000-8000-0000000000aa",
            "service:app",
            "user:slack:T0ACME/U0ALICE",
            "User:a1b2c3d4e5f6",
            "",
        ] {
            assert!(not.parse::<Principal>().is_err(), "{not}");
        }
    }

    #[test]
    fn it_is_written_as_a_string() {
        let user: Principal = "user:k3m4n5p6q7r8".parse().unwrap();
        assert_eq!(
            serde_json::to_string(&user).unwrap(),
            "\"user:k3m4n5p6q7r8\""
        );
        assert_eq!(
            serde_json::from_str::<Principal>("\"user:k3m4n5p6q7r8\"").unwrap(),
            user
        );
        assert!(serde_json::from_str::<Principal>("\"k3m4n5p6q7r8\"").is_err());
        assert_eq!(
            serde_json::to_string(&PrincipalKind::Service).unwrap(),
            "\"service\""
        );
        assert_eq!("service".parse(), Ok(PrincipalKind::Service));
    }
}

#[cfg(test)]
link_test_kit::program!(unit);

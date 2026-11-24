//! Who outside may do what: a spec's `[access]`, each verb a list
//! (`docs/architecture/specs/agent.md`, "Who may talk to it"). One type
//! every spec shares: the agent spec takes `message`, a team's file
//! `member`, and each spec says which verbs it takes ([`Access::only`]),
//! refusing the rest.
//!
//! A principal is one type (`link_principal::Principal`, `user:<id>` or
//! `service:<id>`), the same in `[access] member`, in a teammate's
//! sender and in an access token's `sub` (link-api's generated types are
//! it). A chat a connection keeps is a group, never a principal
//! ([`Chat`]), and so is someone in one: the connection's own user, by
//! its own ID ([`ChatMember`]), whom no identity source vouches for.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub use link_principal::{Principal, PrincipalKind};

use crate::constants::{CHAT_ID_MAX, CHAT_ID_PUNCTUATION};
pub use crate::constants::{STANDING_META, THREAD_META};

/// `[access]`: for each thing someone outside may do, who may.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    /// Who may message the agent, besides its owner: chats on its
    /// connections, whoever is in them, each `chat:<connection>:<chat
    /// id>` (`chat:slack:C0TEAM`). An agent spec takes it, a team's file
    /// doesn't.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message: Vec<Chat>,
    /// Which agents of other accounts may be on a team, each the service
    /// it acts as, `service:<agent id>` ([`teammate`]): the team owner's
    /// yes; the agent's owner says the other, by joining. A team's file
    /// takes it, an agent spec doesn't.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub member: Vec<Principal>,
}

/// An agent spec's `[access]`: who besides its owner may message it.
/// It takes chats alone, so a team's `member` isn't one of its keys.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MessageAccess {
    /// Who may message the agent, besides its owner: chats on its
    /// connections, whoever is in them, each `chat:<connection>:<chat
    /// id>` (`chat:slack:C0TEAM`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message: Vec<Chat>,
}

impl MessageAccess {
    /// The ids of the chats on `connection` that may message the agent
    /// ([`Access::chats`]).
    pub fn chats<'a>(&'a self, connection: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.message
            .iter()
            .filter(move |chat| chat.connection() == connection)
            .map(Chat::id)
    }
}

impl From<MessageAccess> for Access {
    fn from(access: MessageAccess) -> Self {
        Access {
            message: access.message,
            member: Vec::new(),
        }
    }
}

/// A team file's `[access]`: which agents of other accounts may be on
/// it. It takes members alone, so an agent's `message` isn't one of its
/// keys.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberAccess {
    /// Which agents of other accounts may be on the team, each the
    /// service it acts as, `service:<agent id>`: the team owner's yes; the
    /// agent's owner says the other, by joining.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub member: Vec<Principal>,
}

impl MemberAccess {
    /// Whether it names no agent: a file then holds no `[access]`.
    pub fn is_empty(&self) -> bool {
        self.member.is_empty()
    }
}

impl From<MemberAccess> for Access {
    fn from(access: MemberAccess) -> Self {
        Access {
            message: Vec::new(),
            member: access.member,
        }
    }
}

/// A chat on a connection, whoever is in it, `chat:<connection>:<id>`:
/// a group the connection keeps, which a spec lets message its agent,
/// never a principal. Its id is the connection's own (Slack's `C0TEAM`,
/// Telegram's `-1001234`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Chat(String);

impl Chat {
    /// The chat `id` on `connection`; an error for a connection's name
    /// that isn't one ([`is_name`]) or an id a chat can't hold.
    pub fn new(connection: &str, id: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            is_name(connection),
            "{:?} isn't a connection's name",
            link_format::clean(connection)
        );
        anyhow::ensure!(
            is_outside_id(id),
            "{:?} isn't a chat's id",
            link_format::clean(id)
        );
        Ok(Self(format!("chat:{connection}:{id}")))
    }

    /// The connection it's on.
    pub fn connection(&self) -> &str {
        self.parts().0
    }

    /// Its id, as the connection names it.
    pub fn id(&self) -> &str {
        self.parts().1
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parts(&self) -> (&str, &str) {
        self.0
            .strip_prefix("chat:")
            .and_then(|rest| rest.split_once(':'))
            .expect("made by Chat::new")
    }
}

impl FromStr for Chat {
    type Err = anyhow::Error;
    fn from_str(text: &str) -> anyhow::Result<Self> {
        let (connection, id) = text
            .strip_prefix("chat:")
            .and_then(|rest| rest.split_once(':'))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{:?} isn't a chat: chat:<connection>:<chat id>",
                    link_format::clean(text)
                )
            })?;
        Self::new(connection, id)
    }
}

impl TryFrom<String> for Chat {
    type Error = anyhow::Error;
    fn try_from(text: String) -> anyhow::Result<Self> {
        text.parse()
    }
}

impl From<Chat> for String {
    fn from(chat: Chat) -> Self {
        chat.0
    }
}

impl fmt::Display for Chat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Someone in a chat on a connection, `member:<connection>:<id>`, by the
/// connection's own ID for its user (Slack's `T0ACME/U0ALICE`, its
/// workspace's and its member's; a number on Telegram and Discord): what
/// a message from a chat the agent is shared with says it's from. Never a
/// principal: Link mints no ID for a person outside, whom only the
/// connection names.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ChatMember(String);

/// What a [`ChatMember`] is written with, before its connection.
pub const MEMBER_KIND: &str = "member";

impl ChatMember {
    /// Someone on `connection` whose ID there is `id`; an error for a
    /// connection's name that isn't one ([`is_name`]) or an id a user
    /// can't have.
    pub fn new(connection: &str, id: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            is_name(connection),
            "{:?} isn't a connection's name",
            link_format::clean(connection)
        );
        anyhow::ensure!(
            is_outside_id(id),
            "{:?} isn't a user's id",
            link_format::clean(id)
        );
        Ok(Self(format!("{MEMBER_KIND}:{connection}:{id}")))
    }

    /// The connection it's on.
    pub fn connection(&self) -> &str {
        self.parts().0
    }

    /// Its id, as the connection names it.
    pub fn id(&self) -> &str {
        self.parts().1
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parts(&self) -> (&str, &str) {
        self.0
            .strip_prefix(MEMBER_KIND)
            .and_then(|rest| rest.strip_prefix(':'))
            .and_then(|rest| rest.split_once(':'))
            .expect("made by ChatMember::new")
    }
}

impl FromStr for ChatMember {
    type Err = anyhow::Error;
    fn from_str(text: &str) -> anyhow::Result<Self> {
        let (connection, id) = text
            .strip_prefix(MEMBER_KIND)
            .and_then(|rest| rest.strip_prefix(':'))
            .and_then(|rest| rest.split_once(':'))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{:?} isn't someone in a chat: member:<connection>:<user id>",
                    link_format::clean(text)
                )
            })?;
        Self::new(connection, id)
    }
}

impl TryFrom<String> for ChatMember {
    type Error = anyhow::Error;
    fn try_from(text: String) -> anyhow::Result<Self> {
        text.parse()
    }
}

impl From<ChatMember> for String {
    fn from(member: ChatMember) -> Self {
        member.0
    }
}

impl fmt::Display for ChatMember {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether `id` is an id a connection gives its chats and users: 1 to
/// [`CHAT_ID_MAX`] ASCII letters, digits and [`CHAT_ID_PUNCTUATION`].
fn is_outside_id(id: &str) -> bool {
    (1..=CHAT_ID_MAX).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || CHAT_ID_PUNCTUATION.as_bytes().contains(&b))
}

/// How far a message's sender is let in, from its `Said::from`
/// ([`link_connection_contract::acp::Said::from`]), at most a firing's
/// `standing` when it carries one ([`Standing::at_most`]). On the wire,
/// `owner`, `member` or `stranger`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Standing {
    /// The owner, or the agent on their behalf: no `Said::from`, or its own
    /// schedule (`schedule:`), subagent (`task:`) or background command
    /// (`run:`). All the agent can.
    Owner,
    /// Let in by its owner: someone in a chat it's shared with
    /// (`member:`), or a teammate (`agent:`, whom the teams' files let
    /// in). All the agent can, never as its owner: the agent is the
    /// cordon.
    Member,
    /// Anyone else (`webhook:<name>`, or a sender Link doesn't know): only
    /// the tools its spec gives strangers (`[tools] strangers`, none by
    /// default), its conversation narrowed to them.
    Stranger,
}

impl Standing {
    /// The standing of a message whose `Said::from` is `from` (none: the
    /// owner's).
    pub fn of(from: Option<&str>) -> Self {
        let Some(from) = from else {
            return Self::Owner;
        };
        // Only a sender a message may be from stands above a stranger.
        if !crate::access::is_from(from) {
            return Self::Stranger;
        }
        match from.split_once(':').map(|(kind, _)| kind) {
            Some("schedule" | "task" | "run") => Self::Owner,
            Some(MEMBER_KIND | "agent") => Self::Member,
            _ => Self::Stranger,
        }
    }

    /// The lesser of this standing and `cap`: a schedule or subagent
    /// fires as no more than the conversation that made it stood.
    pub fn at_most(self, cap: Standing) -> Self {
        let rank = |standing: Standing| match standing {
            Self::Stranger => 0,
            Self::Member => 1,
            Self::Owner => 2,
        };
        match rank(cap) < rank(self) {
            true => cap,
            false => self,
        }
    }
}

/// A verb of [`Access`], as its key is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Message,
    Member,
}

impl Verb {
    /// Its key in `[access]`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Member => "member",
        }
    }
}

impl Access {
    /// An error naming the first entry a spec doesn't take: a verb it
    /// doesn't list in `takes`, a member that isn't an agent's service,
    /// `service:<agent id>` ([`teammate`]), or one named twice.
    pub fn only(&self, takes: &[Verb]) -> anyhow::Result<()> {
        let verbs: [(Verb, Vec<&str>); 2] = [
            (
                Verb::Message,
                self.message.iter().map(Chat::as_str).collect(),
            ),
            (
                Verb::Member,
                self.member.iter().map(|m| m.as_str()).collect(),
            ),
        ];
        for (verb, who) in verbs {
            if !takes.contains(&verb) {
                anyhow::ensure!(who.is_empty(), "[access] {} isn't taken here", verb.key());
                continue;
            }
            let mut seen = std::collections::BTreeSet::new();
            for named in who {
                anyhow::ensure!(
                    seen.insert(named),
                    "[access] {}: {} is named twice",
                    verb.key(),
                    link_format::clean(named)
                );
            }
        }
        for principal in &self.member {
            anyhow::ensure!(
                principal.kind() == PrincipalKind::Service
                    && link_machine::names::check_id(principal.id()).is_ok(),
                "[access] member: {} isn't an agent's service, service:<agent id>",
                link_format::clean(principal)
            );
        }
        Ok(())
    }

    /// The ids of the chats on `connection` that may message the agent, as
    /// the connection names them (Slack's `C0TEAM`, Telegram's `-1001234`).
    pub fn chats<'a>(&'a self, connection: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.message
            .iter()
            .filter(move |chat| chat.connection() == connection)
            .map(Chat::id)
    }
}

/// A chat on `connection`, whoever is in it: `chat:<connection>:<id>`
/// ([`Chat::new`]).
pub fn chat(connection: &str, id: &str) -> anyhow::Result<Chat> {
    Chat::new(connection, id)
}

/// Someone on `connection`, as it names its users: `member:<connection>:<id>`
/// ([`ChatMember::new`]).
pub fn chat_member(connection: &str, id: &str) -> anyhow::Result<ChatMember> {
    ChatMember::new(connection, id)
}

/// The service another account's agent acts as, a team's member, by the
/// account's ID (`acct-<id>`) and the agent's: `service:<agent id>`, the
/// agent's own ID; an error for an owner that isn't an account's
/// ([`link_machine::owner::is_local`]) or an ID that isn't one.
pub fn teammate(account: &str, id: &str) -> anyhow::Result<Principal> {
    link_machine::owner::check_owner(account)?;
    anyhow::ensure!(
        !link_machine::owner::is_local(account),
        "{:?} isn't an account's ID",
        link_format::clean(account)
    );
    link_machine::names::check_id(id)?;
    Principal::new(&PrincipalKind::Service, id)
        .map_err(|_| anyhow::anyhow!("{:?} isn't an agent's ID", link_format::clean(id)))
}

pub fn is_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether the conversation named `thread` is one of the owner's: never a
/// teammate's (`agent-<id>`), a team's (`team-<id>`), a subagent's
/// (`task-<id>`, which can't show its parent was the owner's) or a
/// stranger's (named as its sender, `webhook:<name>`), nor one with no
/// name. What only the owner's conversations reach (the agent's notes,
/// written and read) asks this.
pub fn is_owners_thread(thread: &str) -> bool {
    !thread.is_empty()
        && !["agent-", "team-", "task-"]
            .iter()
            .any(|kind| thread.starts_with(kind))
        && !is_from(thread)
}

pub fn is_from(from: &str) -> bool {
    match from.split_once(':') {
        Some(("schedule" | "task" | "agent" | "run", id)) => {
            link_machine::names::check_id(id).is_ok()
        }
        Some((MEMBER_KIND, _)) => from.parse::<ChatMember>().is_ok(),
        Some(("webhook", name)) => link_machine::names::check_name(name).is_ok(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A firing stands no higher than its cap, and its cap never raises
    /// it; on the wire a standing is its lowercase word.
    #[test]
    fn a_standing_is_at_most_its_cap() {
        use Standing::*;
        for (from, cap, is) in [
            (Owner, Owner, Owner),
            (Owner, Member, Member),
            (Owner, Stranger, Stranger),
            (Member, Owner, Member),
            (Member, Stranger, Stranger),
            (Stranger, Owner, Stranger),
        ] {
            assert_eq!(from.at_most(cap), is, "{from:?} at most {cap:?}");
        }
        assert_eq!(
            Standing::of(Some("schedule:aaaaaaaaaaa1")).at_most(Member),
            Member
        );
        assert_eq!(serde_json::to_string(&Member).unwrap(), "\"member\"");
        assert_eq!(
            serde_json::from_str::<Standing>("\"stranger\"").unwrap(),
            Stranger
        );
        assert!(serde_json::from_str::<Standing>("\"Owner\"").is_err());
    }

    fn access(text: &str) -> anyhow::Result<Access> {
        toml::from_str(text).map_err(|e| anyhow::anyhow!("{e}"))
    }

    const CHATS: &[Verb] = &[Verb::Message];

    #[test]
    fn a_senders_standing() {
        assert_eq!(Standing::of(None), Standing::Owner);
        for own in [
            "schedule:a1b2c3d4e5f6",
            "task:a1b2c3d4e5f6",
            "run:a1b2c3d4e5f6",
        ] {
            assert_eq!(Standing::of(Some(own)), Standing::Owner, "{own}");
        }
        for member in ["member:slack:T0ACME/U0ALICE", "agent:a1b2c3d4e5f6"] {
            assert_eq!(Standing::of(Some(member)), Standing::Member, "{member}");
        }
        for stranger in [
            "webhook:github",
            "chat:slack:C0",
            "owner:me",
            "",
            "x",
            "schedule:../x",
            "user:0a1b2c3d",
            "user:slack:T0ACME/U0ALICE",
            "user:k3m4n5p6q7r8",
        ] {
            assert_eq!(
                Standing::of(Some(stranger)),
                Standing::Stranger,
                "{stranger}"
            );
        }
    }

    #[test]
    fn a_chat_is_taken_and_found_by_its_connection() {
        let shared = access(
            r#"message = ["chat:slack:C0TEAM", "chat:telegram:-1001234", "chat:slack:C0TWO"]"#,
        )
        .unwrap();
        shared.only(CHATS).unwrap();
        assert_eq!(
            shared.chats("slack").collect::<Vec<_>>(),
            ["C0TEAM", "C0TWO"]
        );
        assert_eq!(shared.chats("telegram").collect::<Vec<_>>(), ["-1001234"]);
        assert_eq!(shared.chats("discord").count(), 0);
        assert_eq!(access("").unwrap(), Access::default());
    }

    #[test]
    fn what_a_spec_doesnt_take_is_refused() {
        // A chat where only members are taken, a verb not taken at all.
        let chats = access(r#"message = ["chat:slack:C0TEAM"]"#).unwrap();
        assert!(chats.only(MEMBERS).is_err());
        assert!(chats.only(&[]).is_err());
        assert!(Access::default().only(&[]).is_ok());
        // A chat with no connection, whose connection isn't one's name, or
        // a person: a chat is a group, and a principal is no chat.
        for bad in [
            "chat:C0TEAM",
            "chat:Slack:C0TEAM",
            "chat:slack:",
            "member:slack:T0ACME/U0ALICE",
            "service:k3m4n5p6q7r8",
        ] {
            assert!(access(&format!("message = [{bad:?}]")).is_err(), "{bad}");
        }
        // Twice.
        let twice = access(r#"message = ["chat:slack:C0TEAM", "chat:slack:C0TEAM"]"#).unwrap();
        assert!(twice.only(CHATS).is_err());
        // Not a chat, or a verb [access] doesn't have.
        assert!(access(r#"message = ["slack:C0TEAM"]"#).is_err());
        assert!(access(r#"message = ["chat:slack:C0 TEAM"]"#).is_err());
        assert!(access(r#"guest = ["chat:slack:C0TEAM"]"#).is_err());
        // An agent spec takes no member, and a chat is no member.
        let member = access(r#"member = ["service:k3m4n5p6q7r8"]"#).unwrap();
        assert!(member.only(CHATS).is_err());
        assert!(access(r#"member = ["chat:slack:C0TEAM"]"#).is_err());
    }

    const MEMBERS: &[Verb] = &[Verb::Member];

    #[test]
    fn a_member_is_another_accounts_agents_service() {
        let members =
            access(r#"member = ["service:k3m4n5p6q7r8", "service:a1b2c3d4e5f6"]"#).unwrap();
        members.only(MEMBERS).unwrap();
        // A user, the agent itself, a service that is no agent's, or twice.
        for bad in [
            "user:k3m4n5p6q7r8",
            "agent:k3m4n5p6q7r8",
            "agent:acct-b7/k3m4n5p6q7r8",
            "service:Reviewer",
            "service:k3m4",
        ] {
            let bad = access(&format!("member = [{bad:?}]"));
            assert!(bad.is_err() || bad.unwrap().only(MEMBERS).is_err());
        }
        let twice = access(r#"member = ["service:k3m4n5p6q7r8", "service:k3m4n5p6q7r8"]"#).unwrap();
        assert!(twice.only(MEMBERS).is_err());
        let made = teammate("acct-b7", "k3m4n5p6q7r8").unwrap();
        assert_eq!(made.as_str(), "service:k3m4n5p6q7r8");
        assert_eq!(made.kind(), PrincipalKind::Service);
        assert!(teammate("me@laptop", "k3m4n5p6q7r8").is_err());
        assert!(teammate("acct-b7", "nope").is_err());
    }

    #[test]
    fn chats_and_people_are_made_and_read_apart() {
        let team = chat("slack", "C0TEAM").unwrap();
        assert_eq!(team.as_str(), "chat:slack:C0TEAM");
        assert_eq!((team.connection(), team.id()), ("slack", "C0TEAM"));
        assert_eq!(team.as_str().parse::<Chat>().unwrap(), team);
        assert!(team.as_str().parse::<Principal>().is_err());
        let alice = chat_member("slack", "T0ACME/U0ALICE").unwrap();
        assert_eq!(alice.as_str(), "member:slack:T0ACME/U0ALICE");
        assert_eq!(
            (alice.connection(), alice.id()),
            ("slack", "T0ACME/U0ALICE")
        );
        assert_eq!(alice.as_str().parse::<ChatMember>().unwrap(), alice);
        // Someone in a chat is no principal, and no chat.
        assert!(alice.as_str().parse::<Principal>().is_err());
        assert!(alice.as_str().parse::<Chat>().is_err());
        assert!(team.as_str().parse::<ChatMember>().is_err());
        assert!("user:slack:T0ACME/U0ALICE".parse::<ChatMember>().is_err());
        assert!(chat("Slack", "C0TEAM").is_err());
        assert!(chat("slack", "C0 TEAM").is_err());
        assert!(chat_member("telegram", &"9".repeat(200)).is_err());
    }
}

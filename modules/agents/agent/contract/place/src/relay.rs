//! A team's relay: how a member that runs on a machine of yours talks
//! with its teammates wherever they run, through the account that hosts
//! its team. The member signs in as itself, with its own agent token
//! and a `team.member` grant for that team,
//! never as its machine, and the relay takes who sent a message from that
//! token alone.
//!
//! ```text
//! POST /v1/teams/<owner>/<team>/messages   TeamSend   -> Sent
//! GET  /v1/teams/<owner>/<team>/messages?after=<seq> -> Mailbox
//! ```
//!
//! A member sends [`TeamSend`]: what it says, never who it is. The relay
//! checks it's on the team now, at each message, and hands
//! [`TeamMessage`] to the member [`TeamSend::to`] names, or to each other
//! member: one running where the relay's account runs it is woken with
//! it, and one running on a machine of yours finds it in its
//! [`Mailbox`] at its next pull. Teammates hear each other through the
//! relay alone: a connection such as Slack is where people read the
//! conversation, never a way between agents.

use link_principal::{Principal, PrincipalKind};
use serde::{Deserialize, Serialize};

/// The most messages a [`Mailbox`] holds at one pull.
pub const MAIL_PAGE: usize = 64;

/// A team as the relay names it: its owner's account and its permanent
/// ID, never its name, which its owner may change.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamIndex {
    /// The team owner's account, `acct-<32 hex>`.
    pub owner: String,
    /// The team's ID.
    pub team: String,
}

impl TeamIndex {
    /// An error unless it's a team: its owner an account, its ID twelve
    /// lowercase letters and digits.
    pub fn check(&self) -> anyhow::Result<()> {
        check_account(&self.owner).map_err(|_| anyhow::anyhow!("not a team owner's account"))?;
        link_machine::names::check_id(&self.team)
            .map_err(|_| anyhow::anyhow!("not a team's ID"))?;
        Ok(())
    }

    /// Its messages' route on the relay, from the relay's address.
    pub fn messages_path(&self) -> String {
        format!("/v1/teams/{}/{}/messages", self.owner, self.team)
    }
}

/// An error unless `account` is an account's ID, `acct-<32 hex>`.
pub fn check_account(account: &str) -> anyhow::Result<()> {
    let hex = account.strip_prefix("acct-").unwrap_or_default();
    anyhow::ensure!(
        hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "{:?} isn't an account (acct-<32 hex>)",
        link_format::clean(account)
    );
    Ok(())
}

/// Who sent a teammate's message: an agent, as the service its agent
/// token names, with the account whose agent it is. Never a user, and
/// never the message's own word.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SenderFields", deny_unknown_fields)]
pub struct Sender {
    /// The agent's account, `acct-<id>`.
    account: String,
    /// The service the agent acts as, `service:<id>`: its own, minted
    /// apart from its agent ID.
    agent: Principal,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SenderFields {
    account: String,
    agent: Principal,
}

impl TryFrom<SenderFields> for Sender {
    type Error = String;
    fn try_from(fields: SenderFields) -> Result<Self, String> {
        Sender::new(&fields.account, fields.agent).map_err(|err| format!("{err:#}"))
    }
}

impl Sender {
    /// The sender an agent token names: `agent`, the service an agent of
    /// `account` acts as.
    pub fn new(account: &str, agent: Principal) -> anyhow::Result<Self> {
        check_account(account).map_err(|_| anyhow::anyhow!("a sender's account is acct-<id>"))?;
        anyhow::ensure!(
            agent.kind() == PrincipalKind::Service,
            "a teammate's message is sent by an agent's service, not a {}",
            agent.kind()
        );
        link_machine::names::check_id(agent.id())
            .map_err(|_| anyhow::anyhow!("a sender is service:<id>, its agent's service"))?;
        Ok(Self {
            account: account.into(),
            agent,
        })
    }

    pub fn account(&self) -> &str {
        &self.account
    }

    pub fn agent(&self) -> &Principal {
        &self.agent
    }
}

/// A team's conversation, `team-<id>`: the one a team's messages are in,
/// read by people on a connection such as Slack, and by each member.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Conversation(String);

impl Conversation {
    /// `team`'s conversation.
    pub fn of(team: &TeamIndex) -> Self {
        Self(format!("team-{}", team.team))
    }

    /// Whether it's `team`'s.
    pub fn is_of(&self, team: &TeamIndex) -> bool {
        *self == Self::of(team)
    }

    /// The team ID it's the conversation of.
    pub fn team(&self) -> &str {
        &self.0["team-".len()..]
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Conversation {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> {
        match text.strip_prefix("team-") {
            Some(id) if link_machine::names::check_id(id).is_ok() => Ok(Self(text)),
            _ => Err("a team's conversation is team-<its ID>".into()),
        }
    }
}

impl From<Conversation> for String {
    fn from(conversation: Conversation) -> String {
        conversation.0
    }
}

/// A member's message to its team, as it hands it to the relay: the
/// team, its conversation, the member it's for or none for each, how far
/// down a chain it is, and its text. Who sent it is the token's, never
/// the body's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamSend {
    pub team: TeamIndex,
    pub conversation: Conversation,
    /// The member it's for, by its agent ID; none: each other member. A
    /// member's scheduler sends a team's message to each teammate it
    /// doesn't reach itself, so each is handed it once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// How many messages led to it, at least one: the conversation's
    /// caps hold a loop.
    pub hops: u32,
    pub text: String,
}

impl TeamSend {
    /// An error unless it holds together: a team, its own conversation, a
    /// member's agent ID if it names one, at least one hop, and a text of
    /// 1 byte to a chat message's most.
    pub fn check(&self) -> anyhow::Result<()> {
        self.team.check()?;
        anyhow::ensure!(
            self.conversation.is_of(&self.team),
            "a teammate's message is in its team's conversation"
        );
        if let Some(to) = &self.to {
            link_machine::names::check_id(to)
                .map_err(|_| anyhow::anyhow!("a message is for a member's agent ID"))?;
        }
        anyhow::ensure!(
            self.hops > 0,
            "a teammate's message is at least one down a chain"
        );
        anyhow::ensure!(
            (1..=crate::CHAT_MESSAGE_MAX).contains(&self.text.len()),
            "a message is 1 byte to {} MiB",
            crate::CHAT_MESSAGE_MAX >> 20
        );
        Ok(())
    }
}

/// What the relay answers a [`TeamSend`] with: how many members it
/// reached, woken or kept for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sent {
    pub reached: u32,
}

/// A teammate's message as the relay hands it to a member: who sent it,
/// as its token said, and what it sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamMessage {
    pub from: Sender,
    pub team: TeamIndex,
    pub conversation: Conversation,
    pub hops: u32,
    pub text: String,
}

impl TeamMessage {
    /// `sent`, checked, as from `from`, the sender its token names.
    pub fn of(from: Sender, sent: TeamSend) -> anyhow::Result<Self> {
        sent.check()?;
        Ok(Self {
            from,
            team: sent.team,
            conversation: sent.conversation,
            hops: sent.hops,
            text: sent.text,
        })
    }
}

/// One message kept for a member, by its place in the member's mail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mail {
    /// Its place: each later one's is greater. A pull `after` it gets
    /// those after it.
    pub seq: u64,
    pub message: TeamMessage,
}

/// What a member's pull answers: the team's messages kept for it after
/// the `after` it asked from, oldest first, at most [`MAIL_PAGE`]. Those
/// at or before `after` are let go: the member had them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mailbox {
    pub mail: Vec<Mail>,
}

/// A pull's query: the last `seq` the member had, or none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct After {
    #[serde(default)]
    pub after: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: &str = "acct-0b7c5e2d4f6a8b9c0d1e2f3a4b5c6d7e";

    fn team() -> TeamIndex {
        TeamIndex {
            owner: ACCOUNT.into(),
            team: "t4m9q2x7k1ab".into(),
        }
    }

    fn sent() -> TeamSend {
        TeamSend {
            team: team(),
            conversation: Conversation::of(&team()),
            to: None,
            hops: 1,
            text: "Draft the summary?".into(),
        }
    }

    /// A sender is an agent's service of an account, as its token names
    /// it: never a person or the body's word; and one is refused in what
    /// a member sends.
    #[test]
    fn a_sender_is_an_agent_its_token_names() {
        let agent: Principal = "service:a1b2c3d4e5f6".parse().unwrap();
        let from = Sender::new(ACCOUNT, agent.clone()).unwrap();
        assert_eq!((from.account(), from.agent()), (ACCOUNT, &agent));
        // A person is no sender; neither a name nor an outside ID is a
        // principal at all.
        assert!(Sender::new(ACCOUNT, "user:a1b2c3d4e5f6".parse().unwrap()).is_err());
        for refused in ["member:slack:T0ACME/U0ALICE", "service:Writer"] {
            assert!(refused.parse::<Principal>().is_err(), "{refused}");
        }
        assert!(Sender::new("me@box", agent.clone()).is_err());
        assert!(Sender::new("acct-b7", agent).is_err());
        let text = serde_json::json!({"account": ACCOUNT, "agent": "installation:a1b2c3d4e5f6"});
        assert!(serde_json::from_value::<Sender>(text).is_err());
        let mut sending = serde_json::to_value(sent()).unwrap();
        sending["from"] = serde_json::json!({"account": ACCOUNT, "agent": "service:a1b2c3d4e5f6"});
        assert!(serde_json::from_value::<TeamSend>(sending).is_err());
    }

    /// What a member sends holds together, and the message handed on is
    /// it, as from the token's sender.
    #[test]
    fn a_teammates_message_holds_together() {
        let from = Sender::new(ACCOUNT, "service:a1b2c3d4e5f6".parse().unwrap()).unwrap();
        let message = TeamMessage::of(from.clone(), sent()).unwrap();
        assert_eq!(message.conversation.as_str(), "team-t4m9q2x7k1ab");
        assert_eq!(message.conversation.team(), "t4m9q2x7k1ab");
        let round: TeamMessage =
            serde_json::from_value(serde_json::to_value(&message).unwrap()).unwrap();
        assert_eq!(round, message);
        assert_eq!(
            team().messages_path(),
            format!("/v1/teams/{ACCOUNT}/t4m9q2x7k1ab/messages")
        );
        let other = TeamIndex {
            team: "z9y8x7w6v5u4".into(),
            ..team()
        };
        for bad in [
            TeamSend {
                conversation: Conversation::of(&other),
                ..sent()
            },
            TeamSend { hops: 0, ..sent() },
            TeamSend {
                text: String::new(),
                ..sent()
            },
            TeamSend {
                to: Some("../x".into()),
                ..sent()
            },
            TeamSend {
                team: TeamIndex {
                    team: "../x".into(),
                    ..team()
                },
                ..sent()
            },
        ] {
            assert!(TeamMessage::of(from.clone(), bad).is_err());
        }
        for bad in ["agent-a1b2c3d4e5f6", "team-", "team-../x", "telegram:42"] {
            assert!(Conversation::try_from(bad.to_string()).is_err(), "{bad}");
        }
    }
}

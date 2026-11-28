//! The contract between the harness module (`lnk-agent`) and a connection
//! module (`lnk-telegram`, `lnk-slack`, `lnk-discord`, ...): where you
//! talk to your agent.
//!
//! A connection module is a program answering:
//!
//! ```text
//! lnk-<connection> connect --agent <name> [--taken <json>]
//!         asks for what the connection needs (its token, from its LNK_<CONNECTION>_...
//!         variables, else on the terminal: questions on stderr), checks it,
//!         refuses a bot in `--taken` (a JSON list of [`Taken`]), and pairs the
//!         owner: whoever sends the bot the one-time code it prints. Prints
//!         [`Connected`] as JSON on stdout.
//! lnk-<connection> describe --json
//!         prints [`Describe`] as JSON: its title, which of its settings are
//!         secret, whether it's linked as a device, whether it carries
//!         messages (`bridge`), its hosts, and what disconnecting leaves.
//! lnk-<connection> cli install --home <dir> --json
//!         a connection whose `describe` says `cli` only: reads its section of
//!         [`crate::harness::Settings`] as JSON on stdin and installs the
//!         program a harness speaking it runs into that harness's home, `<dir>`;
//!         prints `{"program"}`, its path, which the harness is handed in the
//!         section. `lnk-agent` runs it in the harness's own sandbox.
//! lnk-<connection> cli link --home <dir> [--again]
//!         the same connections only: links the account in `<dir>` with that
//!         program, as `link` does; exits 0 once linked, and
//!         [`crate::harness::NOT_LINKED`] when it isn't and there's no terminal.
//! lnk-<connection> rule --json
//!         prints [`Rule`] as JSON, or `null`: how a harness that speaks the
//!         connection reaches it with a placeholder in place of its secret, which
//!         the harness's proxy adds outside its sandbox
//!         ([`crate::sandbox::Secret`]). `null`: the harness holds the secret
//!         (a connection whose secret goes over a connection the proxy can't read).
//! lnk-<connection> bridge
//!         a connection that carries messages ([`Describe::bridged`]) only:
//!         reads [`Bridge`] as one JSON line on stdin, then carries messages until stdin closes: each one from the
//!         owner goes to the harness's endpoint ([`ask`]), and the answer goes
//!         back, shown while it's written. The owner's mention of the bot
//!         in a group (a connection, a server) goes too, without the mention,
//!         each group a thread of its own, and is answered there; anyone
//!         else's gets nothing.
//! lnk-<connection> link [--again]
//!         a linked connection ([`Describe::linked`]) only: reads [`Bridge`] as
//!         one JSON line on stdin and links the connection's account as a device, into [`Bridge::state`], from a QR code
//!         on the terminal (stdout). Exits 0 at once when it's linked already
//!         (`--again`: links it anew), and [`crate::harness::NOT_LINKED`] when
//!         it isn't and stdout is no terminal to show the code on.
//! lnk-<connection> state
//!         a linked connection ([`Describe::linked`]) only: prints
//!         [`crate::harness::State`] as JSON, what of [`Bridge::state`] is the link, paths `~/`-relative to that
//!         folder. `lnk agent move` carries it to the other machine, so the
//!         connection answers there without linking again; what else is in the
//!         folder (a program it installed) stays, and the other machine
//!         makes its own.
//! ```
//!
//! A connection is the harness's own when its adapter speaks it: the module
//! only makes and pairs it. For a harness that speaks none, and answers a
//! chat completions endpoint instead (`lnk-<name> endpoint`), the module
//! carries the messages too, outside the harness's sandbox, so the
//! harness never holds the connection's token. `lnk-agent` keeps the
//! connection's settings (its section of [`crate::harness::Settings`]),
//! stops the agent while it pairs (the agent could answer the code
//! itself), and hands them to the adapters that speak it, or to
//! `bridge`.

use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

pub mod acp;
pub mod addressing;
mod asking;
mod endpoint;
mod pairing;
mod parts;
mod protocol;
pub mod pushed;
pub mod questions;
mod received;
pub mod rich;
mod run;
mod sends;
mod sharing;
mod terms;
mod threads;
pub mod webhook;

use asking::accepted;
pub use asking::{
    Accepted, AnswerTo, Asked, Fired, Firing, NAME_MAX, Recover, Recovered, Speaker, arriving, ask,
    ask_accepted, ask_for, ask_from, ask_with, env_prefix, fire, is_from, is_model_name, is_name,
    is_person_name, name_header, name_of, recovered, recovered_done, teammates_thread,
    teams_thread, way_in,
};
pub use endpoint::UNIX_ENDPOINT;

/// The harness's ACP, opened as `bridge` reaches it: proved first with
/// [`Bridge::proof`], then opened with the agent's headers and key on that
/// same connection. For a relay that carries the socket to a connection
/// module elsewhere, so neither the key nor the proof leaves the machine
/// the harness is on.
pub async fn harness_acp(bridge: &Bridge) -> anyhow::Result<acp::Socket> {
    endpoint::upgraded(bridge).await
}
use endpoint::{MAX_WHOLE, Sent, started};
pub use pairing::{
    Expired, couldnt_answer, has_code, is_code, is_number, normalize_number, note, open,
    pairing_code, qr, steps_aside, warn,
};
pub use parts::*;
pub use protocol::{PROTOCOL, Stated, VERSION_KEY, read, stated};
pub use received::Received;
pub use run::{
    Operation, REPLY_REFUSED, ReplyRefused, Request, TOKEN_REFUSED, TokenRefused, WEBHOOK_REFUSED,
    WebhookRefused, connect_request, document, failed, main, main_linked, main_with_bridge,
    reply_refused, token_refused, webhook_refused,
};
pub use sends::{SENDS, knows_group, remember_group, send_through, sends_at, take_sends};
use sharing::take_share;
pub use sharing::{SHARE_WAIT, Shared, Sharing};
pub use terms::Terms;
pub use threads::{
    MAX_ANSWERING, SseLines, check_taken, per_thread, per_thread_interacting, per_thread_replying,
};

/// What `connect` made.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connected {
    /// The connection's section of [`crate::harness::Settings`]
    /// (`{"bot_token", "allowed_users"}` for Telegram, ...): the owner
    /// alone allowed.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub settings: link_secretive::Secret<link_secretive::Json>,
    pub bot: Bot,
}

/// What a connection module is, for [`main`]: its name (`telegram`, as in
/// `lnk-telegram`), what `describe` says of it, and its `rule`.
#[derive(Clone, Debug, Default)]
pub struct About {
    pub name: &'static str,
    pub describe: Describe,
    pub rule: Option<Rule>,
    /// How it sends a message nobody asked for (`send`): an agent's
    /// answer to a schedule or a subagent. None: it can't.
    pub send: Option<Sender>,
    /// How its messages are pushed to a machine that packs agents
    /// (`hook`, `unhook`, `sift`, `take`, `hold`: [`pushed`]). None: they
    /// aren't, and only `bridge` carries them.
    pub pushed: Option<pushed::Pushed>,
    /// How it takes a share's code in a chat (`share`, [`Sharing`]): the
    /// chat a paired user mentioned the bot in with the code. None: no
    /// chat of it can be shared, and its `describe` says so
    /// ([`Describe::share`]).
    pub share: Option<Sharer>,
    /// The owner's direct chats, as its threads name them after the
    /// connection (`42` of `telegram:42`), from a bridge's settings (`owner`):
    /// where the owner is asked what only they can answer. None: it has
    /// none Link can ask on.
    pub owner: Option<Owners>,
}

/// The owner's direct chats on a connection whose settings `Bridge` holds,
/// as `lnk-<connection> owner` prints them.
pub type Owners = fn(&Bridge) -> anyhow::Result<Vec<String>>;

/// Waits for a share's code in a chat of the connection, as `lnk-<connection>
/// share` does ([`Sharing`]), and says which chat it came in. Handed its
/// wait already capped at [`SHARE_WAIT`], and stopped when it's over.
pub type Sharer =
    fn(
        Sharing,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<Shared>> + Send>>;

/// Sends `text` to chat `chat` of a connection whose settings `Bridge`
/// holds, as `lnk-<connection> send` does: only to a chat of the owner's
/// alone (a direct chat), or a group the owner talks to the agent in
/// ([`knows_group`]) while the agent's spec lets it answer in groups;
/// never another group's or a stranger's.
pub type Sender = fn(
    Bridge,
    String,
    String,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = anyhow::Result<SentMessages>> + Send>,
>;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SentMessages {
    pub messages: Vec<String>,
}

/// What `lnk-<connection> send` reads on stdin, one JSON line.
#[derive(Duplicate, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sending {
    #[duplicate]
    pub bridge: Bridge,
    /// The chat, as its thread names it after the connection
    /// (`telegram:42`: `42`).
    pub chat: String,
    pub text: String,
}

/// What `describe --json` prints: what the harness module needs to keep,
/// show and carry the connection without knowing it by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Describe {
    /// Its name as people write it (`Telegram`).
    pub title: String,
    /// The fields of its settings ([`Connected::settings`]) that are
    /// secret: a Mac keeps them in the Keychain, never in the settings
    /// file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_fields: Vec<String>,
    /// Whether its account is linked as a device (`link`, `state`), not
    /// given by a token.
    #[serde(default)]
    pub linked: bool,
    /// Whether it carries messages (`bridge`) for a harness that speaks
    /// none.
    #[serde(default)]
    pub bridged: bool,
    /// Whether it installs a program into the home of a harness that
    /// speaks it, and links the account there (`cli install`, `cli
    /// link`), as `lnk-signal` does `signal-cli`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cli: bool,
    /// Whether its messages can be pushed over HTTPS to a machine that
    /// packs agents (`hook`, `unhook`, `sift`, `take`: [`pushed`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pushed: bool,
    /// Whether its messages come only on a connection held open, which
    /// `hold` holds for many bots at once ([`pushed`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
    /// The hosts a harness speaking it reaches (`api.telegram.org`,
    /// `*.slack.com`), for `lnk agent permission add host`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    /// What `lnk agent disconnect` says of what stays the user's (the
    /// bot, the app, the linked device) and where to remove it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub disconnect_hint: String,
    /// Whether a chat of it can be shared with the agent: `share` takes a
    /// share's code there ([`Sharing`]), and its bridge answers whoever
    /// is in a chat it's handed ([`Bridge::shared`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub share: bool,
}

/// How the proxy adds a connection's secret for a harness that speaks it
/// (`rule --json`): the harness hands its adapter the connection's settings
/// with a placeholder in `field` and the proxy's address in `root`, and
/// hands the proxy the secret with `wire`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// The field of the connection's settings holding the secret
    /// (`bot_token`).
    pub field: String,
    /// The field of the connection's settings its adapter reads the
    /// service's root from, in place of the service's own
    /// (`api_root`).
    pub root: String,
    /// Where the proxy sends the secret, and what it refuses.
    #[serde(flatten)]
    pub wire: crate::secrets::route::SecretRule,
}

/// What `rule --json` answers: the connection's [`Rule`], left out when
/// the harness holds the secret itself, so the answer is always an object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ruled {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<Rule>,
}

/// A connection's bot: none of it secret.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bot {
    /// Its id on the connection, the same whichever token names it.
    pub id: String,
    /// Its name (a Telegram bot's username).
    pub name: String,
    /// Its workspace, where the connection has them (Slack).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
}

/// A bot another agent has: `connect` refuses it, since two agents on one
/// bot would each miss messages or both answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Taken {
    /// Its [`Bot::id`].
    pub id: String,
    pub agent: String,
    /// The machine the agent runs on; none: this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
}

pub use link_harness_wire::message::{
    LINK, LINK_MAIN, check_link_thread, is_link_thread, link_thread,
};

/// The header naming the agent a message is for: `Link-Agent: <agent>`
/// (`main`, `work`), on every request a bridge sends a harness's
/// endpoint, its proof's too. A harness whose contract says
/// [`crate::harness::Contract::agent`] answers it as that agent's, and
/// refuses one naming an agent it doesn't run; any other answers every
/// request as its own agent's.
pub const AGENT_HEADER: &str = "Link-Agent";

/// The agent's ID ([`link_id::candidate`]), beside its name in
/// [`AGENT_HEADER`], on every request a bridge sends, its proof's too,
/// when the agent has one. Names are unique only to an owner, so a
/// harness serving more than one owner's agents keys on this; one that
/// serves one agent may ignore it.
pub const AGENT_ID_HEADER: &str = "Link-Agent-Id";

/// What `bridge` is handed on stdin: where to carry the messages, and
/// the connection's settings (they hold its token, so never on a command
/// line).
#[derive(Duplicate, Default, PartialEq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Bridge {
    /// The agent's name: each request's [`AGENT_HEADER`], and its log
    /// lines.
    pub agent: String,
    /// Who the agent is ([`crate::bridge::Identity`]): each request's
    /// [`AGENT_ID_HEADER`] is its ID, and every call this bridge takes
    /// (what `send` hands it) must be proved as this agent's, with `key`.
    #[serde(default, skip_serializing_if = "is_nobody")]
    pub identity: crate::bridge::Identity,
    /// The harness's ACP ([`crate::acp`]), a WebSocket at its address's
    /// [`crate::acp::ACP_PATH`] (`http://127.0.0.1:18800/acp`), or a Unix
    /// socket it's reached through ([`UNIX_ENDPOINT`]): each message a
    /// prompt, its session the thread.
    pub endpoint: String,
    /// Sent as `Authorization: Bearer <key>`; empty, not sent.
    #[serde(default)]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub key: link_secretive::Secret,
    /// Whether the endpoint proves it's the harness before it's sent the
    /// key and a message ([`crate::harness::Contract::proof`]): then
    /// whatever else answers on its port gets neither.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub proof: bool,
    /// Whether the endpoint takes a message's images and documents
    /// ([`ask_holding`]); no, it reads text only.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parts: bool,
    /// The connection's section of [`crate::harness::Settings`]: its token
    /// and who may talk to it.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub settings: link_secretive::Secret<link_secretive::Json>,
    /// A folder of the agent's for this connection alone, readable only by
    /// the user and never by the harness: what links a linked connection (a
    /// device's keys) is kept there.
    #[serde(default)]
    pub state: std::path::PathBuf,
    /// The agent's opinions for this connection (its spec's
    /// `[connection.<name>]` over `[connections]`): a reaction while it
    /// answers, whether an answer shows as it's written, and how often.
    /// None: a bridge's own way, with no spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<crate::policy::Connection>,
    /// The chats on this connection the agent is shared with, by the
    /// connection's own ids (its spec's `[access] message`,
    /// [`crate::access::Access::chats`]): whoever is in one may talk to it
    /// there, as `groups` lets its owner, each message
    /// [`crate::acp::Said::from`]'s `user:`. Empty: its owner alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<String>,
}

fn is_nobody(identity: &crate::bridge::Identity) -> bool {
    *identity == crate::bridge::Identity::default()
}

impl Bridge {
    /// Who the agent is.
    pub fn identity(&self) -> crate::bridge::Identity {
        self.identity.clone()
    }

    /// What the agent proves its calls to this bridge with: its identity
    /// and its key for this start.
    pub fn binding(&self) -> crate::bridge::Binding {
        crate::bridge::Binding {
            identity: self.identity(),
            key: self.key.duplicate(),
        }
    }

    /// Whose calls its `send.sock` takes ([`take_sends`]): its agent's,
    /// proved with its key for this start, and only while this machine's
    /// keyring holds that key: once the agent stops, or its key is bound
    /// anew, a bridge still running takes nothing proved with it.
    pub fn sends_keys(
        &self,
    ) -> crate::bridge::Both<crate::bridge::Binding, crate::bridge::Keyring> {
        crate::bridge::Both(self.binding(), crate::bridge::Keyring::here())
    }

    /// Whether it answers the owner in a group, as the agent's spec says
    /// (`[connection.<name>] groups`): yes, to a mention, unless `never`.
    pub fn answers_groups(&self) -> bool {
        self.spec.as_ref().and_then(|spec| spec.groups) != Some(crate::policy::Groups::Never)
    }
}

/// How often an answer being written is shown again on `bridge`'s
/// connection: its spec's `pace`, never more often than `floor`, the
/// platform's documented limit on a bot's messages (a fact, so the
/// bridge's; zero where the platform documents none). None when its spec
/// says not to stream: the answer is shown once it's done.
pub fn pace(bridge: &Bridge, floor: std::time::Duration) -> Option<std::time::Duration> {
    let spec = bridge.spec.clone().unwrap_or_default();
    if spec.stream == Some(false) {
        return None;
    }
    let paced = spec
        .pace
        .map(link_actions::value::Duration::get)
        .unwrap_or(floor);
    Some(paced.max(floor))
}

pub use link_connection_policy::{TOOLS_MAX, is_tool_name, tools_header, tools_of};

pub use asking::ask_occurrence;

#[cfg(test)]
mod tests {
    use super::*;

    /// In a group, a bridge answers a mention unless the spec says never;
    /// one with no spec, as it did.
    #[test]
    fn groups_are_answered_as_the_spec_says() {
        use crate::agent_spec::{Connection, Groups};
        let bridge = |groups| Bridge {
            spec: Some(Connection {
                groups,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(bridge(Some(Groups::Mention)).answers_groups());
        assert!(bridge(None).answers_groups());
        assert!(Bridge::default().answers_groups());
        assert!(!bridge(Some(Groups::Never)).answers_groups());
        let spec =
            crate::agent_spec::parse("schema = 1\n[connection.telegram]\ngroups = \"never\"\n")
                .unwrap();
        assert_eq!(spec.connection("telegram").groups, Some(Groups::Never));
        assert_eq!(
            crate::agent_spec::link().connection("slack").groups,
            Some(Groups::Mention)
        );
        assert!(crate::agent_spec::parse("[connections]\ngroups = \"all\"\n").is_err());
    }

    #[test]
    fn connected_round_trips() {
        let connected = Connected {
            settings: serde_json::json!({"bot_token": "123:x", "allowed_users": [42]}).into(),
            bot: Bot {
                id: "123".into(),
                name: "my_ai_bot".into(),
                team: None,
            },
        };
        let json = serde_json::to_string(&connected).unwrap();
        assert!(!json.contains("team"));
        assert_eq!(serde_json::from_str::<Connected>(&json).unwrap(), connected);
    }
}

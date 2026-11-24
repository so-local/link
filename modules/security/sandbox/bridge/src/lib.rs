//! What every bridge is. A bridge is a way out of an agent's sandbox
//! other than its proxy: a program outside the sandbox that holds what
//! the sandbox never sees (a connection's token, a store's credential) and
//! does one kind of thing for the agent with it. Every call to one is the
//! same three things, whichever bridge it is:
//!
//! - **the identity** ([`Identity`]): the agent's ID and owner, as Link
//!   defines them, never a process's, a socket's or the system's word;
//! - **the action**: what the bridge is asked to do, its own type (a
//!   message for the agent, a message to send, a store's call);
//! - **the proof** ([`proof`]): HMAC-SHA256, with the agent's own key, of
//!   a random challenge the bridge sends on that connection and the
//!   identity named.
//!
//! On the wire, each connection is one call: the bridge writes a
//! [`Challenge`] as a JSON line; the caller writes its [`Proven`] (who it
//! is, and the proof) as a short JSON line, then its action as another;
//! the bridge checks the first before it reads the second, and writes its
//! answer, one JSON line: the bridge's own, or the one error envelope
//! every bridge answers with ([`Failure`]). A call whose identity has no
//! key here ([`Keys`]), or whose proof isn't that identity's, is refused
//! before its action is read at all: a key proves only its own agent, so
//! a harness, however it's steered, acts only as itself, and a proof
//! gives the key away to no one. Who called is its ID and owner as
//! proved, never teams it names: a bridge reads those itself.
//!
//! **The server** ([`serve`], [`serve_with`]) is the same for every
//! bridge: a long-lived process taking calls, each on its own
//! connection, with one deadline before the proof ([`PROOF_WAIT`]),
//! places of their own for connections not yet proved, a few of each
//! peer's ([`PEER_PROVING`]), and caps on the proved ones, all of them,
//! one agent's and one owner's ([`Limits`]), past which a call is
//! answered that the bridge is busy. A bridge says
//! only what it does with a call. What it holds for its calls (a
//! driver's connection, a connection's device) lives in it, shared by every
//! call it takes.
//!
//! **What a call reaches of another's** ([`grants`]), a teammate or a
//! team's folder, is `link_policy::allows`'s to say: a bridge gives it the
//! proved agent, holding the roles it reads at that call.
//!
//! **A stream call** ([`stream`]) is the same, but for its answer: after
//! its action the caller sends what the call carries as it comes, and
//! the bridge answers one line, its verdict, then what it answers, as
//! it comes ([`serve_streams`]): a model's answer, a page's.
//!
//! **The test** every bridge passes is [`conformance::check`] (the
//! `conformance` feature): a wrong key, a proof replayed on a new
//! connection, a malformed or overlong [`Proven`], a caller that never
//! answers or sends its action too slowly, a peer's flood, busy, and the
//! error envelope.

pub mod call;
#[cfg(any(test, feature = "conformance"))]
pub mod conformance;
pub mod grants;
pub mod serve;
pub mod stream;
pub mod take;

use std::time::Duration;

use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

pub use link_secretive::Secret;

pub use call::{
    BUSY_TRIES, answer_of, call, call_async, call_at, read_answer, read_answer_async,
    read_line_async, retrying, write_line_async,
};
pub use serve::{Limits, serve, serve_streams, serve_with};
pub use take::{take, take_async};

/// How long a connection has, from its challenge, to prove who's
/// calling: the whole exchange up to its [`Proven`], however it's sent.
pub const PROOF_WAIT: Duration = Duration::from_secs(5);

/// The most connections one peer (the user at a connection's other end,
/// as the system names it) holds proving who's calling, apart from the
/// proved calls a bridge's [`Limits`] count. Its next closes its own
/// oldest, answered that the bridge is busy, never another peer's.
pub const PEER_PROVING: usize = 4;

/// The most peers a bridge keeps room for, proving at once: each of them
/// holding all of its [`PEER_PROVING`] leaves the last its own. A
/// machine whose bridges more users reach (a box's slots, each a user)
/// is set up for no more than these.
pub const PEERS: usize = 16;

/// The most connections a bridge holds that haven't proved who's calling
/// yet, of every peer: past these, a new peer's connection is answered
/// that the bridge is busy.
pub const PROVING: usize = PEERS * PEER_PROVING;

// Every one of the most peers holding its share leaves the last its own.
const _: () = assert!(PEERS * PEER_PROVING <= PROVING);

/// How long a call's action may take to arrive, all of it, where a
/// program takes its calls itself ([`take_async`]).
pub const CALL_WAIT: Duration = Duration::from_secs(30);

/// The most a call's [`Proven`] line is: read, and checked, before
/// anything else of the call.
pub const PROVEN_MAX: u64 = 4096;

/// The most a call's action is, as JSON, whichever bridge it's for: a
/// store's event at its most twice over (33 MiB), or a files bridge's
/// object at its most as base64 (about 43 MiB). Read only once its proof
/// is checked.
pub const CALL_MAX: u64 = 48 << 20;

/// How long a caller waits for a bridge's challenge.
pub const CHALLENGE_WAIT: Duration = Duration::from_secs(10);

/// Who an agent is: its ID, the service it acts as, its owner, and its
/// teams' IDs.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Identity {
    pub agent: String,
    /// The service the agent acts as, its bare ID, minted apart from the
    /// agent's: who a grant or a role names. Empty, it acts as none, and
    /// [`grants::principal`] refuses it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub service: String,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<String>,
}

/// The longest an owner's ID is.
pub const OWNER_MAX: usize = 64;

/// An error unless `owner` is an owner's ID: an account's
/// (`acct-<id>`) or a local one (`<user>@<machine>`), letters,
/// digits, `.`, `_` and `-`, with at most one `@`, at most
/// [`OWNER_MAX`] long.
pub fn check_owner(owner: &str) -> anyhow::Result<()> {
    let plain = (1..=OWNER_MAX).contains(&owner.len())
        && owner.starts_with(|c: char| c.is_ascii_alphanumeric())
        && owner.matches('@').count() <= 1
        && !owner.ends_with('@')
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b));
    match plain {
        true => Ok(()),
        false => anyhow::bail!(
            "{:?} isn't an owner (an account's ID, or <user>@<machine>)",
            link_format::clean(owner)
        ),
    }
}

/// The bridge protocol, as `protocol.toml` declares it: the version each
/// side states when it connects, a bridge in its [`Challenge`] and a
/// caller in its [`Proven`], and the oldest each accepts of the other.
pub static PROTOCOL: std::sync::LazyLock<link_version::Protocol> =
    std::sync::LazyLock::new(|| link_version::Protocol::shipped(include_str!("protocol.toml")));

/// What a bridge writes first on each connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    /// The bridge protocol's version the bridge speaks ([`PROTOCOL`]).
    pub protocol: u32,
    /// Random hex, new for each connection.
    pub challenge: String,
}

impl Challenge {
    /// A challenge of this release's protocol.
    pub fn new(challenge: String) -> Self {
        Challenge {
            protocol: PROTOCOL.current,
            challenge,
        }
    }
}

/// Who calls a bridge, and the proof that it's who it says: a call's
/// first line, before its action.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proven {
    /// The bridge protocol's version the caller speaks ([`PROTOCOL`]).
    pub protocol: u32,
    pub identity: Identity,
    /// [`proof`] of the connection's challenge and `identity`.
    pub proof: String,
}

/// The one error envelope: what every bridge answers a call that failed
/// with, as one JSON line, `{"error": "<why, for people>"}`, and `"why"`
/// when the failure is one a caller acts on ([`Why`]). A bridge's own
/// answers never hold `error`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub error: String,
    #[serde(default, skip_serializing_if = "Why::is_failed")]
    pub why: Why,
}

/// What kind of failure a [`Failure`] is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Why {
    /// The call was taken, and what it asked failed.
    #[default]
    Failed,
    /// The call wasn't taken: who's calling isn't proved, or what it sent
    /// isn't a call this bridge reads. Nothing of it was done.
    Refused,
    /// The bridge holds as many calls as it takes: nothing was done, and
    /// the same call may be made again.
    Busy,
    /// What the call expected of what it changes isn't so any more (a
    /// file written since it was read): nothing was done; read it again,
    /// then call.
    Conflict,
}

impl Why {
    fn is_failed(&self) -> bool {
        *self == Why::Failed
    }
}

impl Failure {
    /// A failure of kind `why`, saying `error`.
    pub fn new(why: Why, error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            why,
        }
    }

    /// `err` as a bridge answers it: the failure it is, if it's one, else
    /// a call that failed, saying why.
    pub fn of(err: anyhow::Error) -> Self {
        match err.downcast::<Failure>() {
            Ok(failure) => failure,
            Err(err) => Failure::new(Why::Failed, format!("{err:#}")),
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&clean(&self.error))
    }
}

impl std::error::Error for Failure {}

/// What only the holder of `key` can answer to `challenge`, for
/// `identity`: HMAC-SHA256 of the two, after a label of its own, in hex.
pub fn proof(key: &Secret, challenge: &str, identity: &Identity) -> String {
    link_secretive::hmac::hex(&link_secretive::hmac::sign(
        key.expose().as_bytes(),
        &signed(challenge, identity),
    ))
}

/// Whether `proof` is [`proof`] of `challenge` and `identity` with `key`,
/// compared in constant time; never with an empty key.
pub fn proof_matches(key: &Secret, challenge: &str, identity: &Identity, proof: &str) -> bool {
    let Some(proof) = link_secretive::hmac::unhex(proof) else {
        return false;
    };
    link_secretive::hmac::verify(
        key.expose().as_bytes(),
        &signed(challenge, identity),
        &proof,
    )
}

/// What's signed: the challenge and who's calling, each after a NUL,
/// after a label no other proof Link makes has.
fn signed(challenge: &str, identity: &Identity) -> Vec<u8> {
    [
        b"link bridge proof\0".as_slice(),
        challenge.as_bytes(),
        b"\0",
        identity.agent.as_bytes(),
        b"\0",
        identity.service.as_bytes(),
        b"\0",
        identity.owner.as_bytes(),
    ]
    .concat()
}

/// An agent bound here: who it is, and its key for its bridge calls. Its
/// key is never printed.
#[derive(Duplicate, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Binding {
    pub identity: Identity,
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub key: Secret,
}

impl Binding {
    /// A second copy, key and all, said so.
    pub fn duplicate(&self) -> Self {
        Self {
            identity: self.identity.clone(),
            key: self.key.duplicate(),
        }
    }

    /// Its proof on the connection whose challenge is `challenge`.
    pub fn prove(&self, challenge: &str) -> Proven {
        Proven {
            protocol: PROTOCOL.current,
            identity: self.identity.clone(),
            proof: proof(&self.key, challenge, &self.identity),
        }
    }
}

/// Where a bridge looks up the key of the agent a call names: none for
/// an agent not bound here, or bound as another owner's.
pub trait Keys: Send + Sync {
    fn key_of(&self, identity: &Identity) -> Option<Secret>;

    /// A key additionally scoped by the kernel's socket peer. Bridges
    /// shared between Linux users override this; ordinary keyrings keep
    /// their existing identity boundary.
    fn key_of_peer(&self, identity: &Identity, _peer: u32) -> Option<Secret> {
        self.key_of(identity)
    }
}

struct PeerKeys {
    keys: std::sync::Arc<dyn Keys>,
    peer: u32,
}
impl Keys for PeerKeys {
    fn key_of(&self, identity: &Identity) -> Option<Secret> {
        self.keys.key_of_peer(identity, self.peer)
    }
}

/// A bridge for one agent alone (a connection's, handed the agent's key as
/// it starts): only that agent's calls.
impl Keys for Binding {
    fn key_of(&self, identity: &Identity) -> Option<Secret> {
        same_agent(&self.identity, identity).then(|| self.key.duplicate())
    }
}

/// Keys two places agree on: an agent's key only while both hold it, the
/// same. A bridge handed its agent's key as it starts checks the
/// machine's keyring too, so a key unbound there proves nothing on it
/// any more, however long the bridge runs.
#[derive(Clone)]
pub struct Both<A, B>(pub A, pub B);

impl<A: Keys, B: Keys> Keys for Both<A, B> {
    fn key_of(&self, identity: &Identity) -> Option<Secret> {
        let key = self.0.key_of(identity)?;
        let other = self.1.key_of(identity)?;
        // Compared whole, in constant time (`Secret`'s `==`).
        (key == other).then_some(key)
    }

    fn key_of_peer(&self, identity: &Identity, peer: u32) -> Option<Secret> {
        let key = self.0.key_of_peer(identity, peer)?;
        let other = self.1.key_of_peer(identity, peer)?;
        (key == other).then_some(key)
    }
}

impl<K: Keys + ?Sized> Keys for std::sync::Arc<K> {
    fn key_of(&self, identity: &Identity) -> Option<Secret> {
        self.as_ref().key_of(identity)
    }

    fn key_of_peer(&self, identity: &Identity, peer: u32) -> Option<Secret> {
        self.as_ref().key_of_peer(identity, peer)
    }
}

/// Whether `a` and `b` are one agent: its ID and its owner, and its
/// service where both name one. A call naming no service acts as none,
/// so it's never one that names another's.
pub fn same_agent(a: &Identity, b: &Identity) -> bool {
    !a.agent.is_empty()
        && a.agent == b.agent
        && a.owner == b.owner
        && (a.service.is_empty() || b.service.is_empty() || a.service == b.service)
}

pub use link_format::clean;

/// A new challenge: 64 random hex characters.
fn challenge() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
        .map_err(|_| anyhow::anyhow!("no random bytes for a challenge"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether `text` is an ID or an owner as a call may name one: 1 to 64
/// letters, digits, `.`, `_`, `-` and `@`. What a [`Proven`] names is
/// refused unless it is, before any key is looked up, so whatever a
/// refusal says of it is safe to show.
fn plain(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b))
}

/// Who `call` is from, once its proof of `challenge` is checked against
/// `keys`: its ID and owner, no teams (a proof doesn't cover them).
fn checked(challenge: &str, call: &Proven, keys: &dyn Keys) -> Result<Identity, Failure> {
    let refused = |error: String| Failure::new(Why::Refused, error);
    if let Err(older) = PROTOCOL.accept("the caller", call.protocol) {
        return Err(refused(format!("{older}: the call isn't taken")));
    }
    let who = &call.identity;
    if !plain(&who.agent) || !plain(&who.owner) || !(who.service.is_empty() || plain(&who.service))
    {
        return Err(refused(
            "who's calling isn't an agent's ID and owner: the call isn't taken".into(),
        ));
    }
    let Some(key) = keys.key_of(who) else {
        return Err(refused(format!(
            "agent {} isn't bound here: its call isn't taken",
            who.agent
        )));
    };
    if !proof_matches(&key, challenge, who, &call.proof) {
        return Err(refused(format!(
            "the call's proof isn't agent {}'s: it isn't taken",
            who.agent
        )));
    }
    Ok(Identity {
        teams: Vec::new(),
        ..who.clone()
    })
}

/// A line read as JSON of `T`: a refusal, saying `what` it was, when it
/// isn't.
fn parsed<T: serde::de::DeserializeOwned>(line: &[u8], what: &str) -> Result<T, Failure> {
    serde_json::from_slice(line).map_err(|err| {
        Failure::new(
            Why::Refused,
            format!("{what} isn't what this bridge reads: {err}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners() {
        for good in [
            "acct-0123456789abcdef0123456789abcdef",
            "github-22600052",
            "dana@danas-mbp",
            "a.b_c@x-1",
        ] {
            check_owner(good).unwrap();
        }
        for bad in [
            "",
            "@x",
            "a@b@c",
            "a@",
            "a b",
            "a/b",
            "../x",
            &"x".repeat(65),
        ] {
            assert!(check_owner(bad).is_err(), "{bad:?}");
        }
    }

    fn me() -> Identity {
        Identity {
            agent: "aaaaaaaaaaaa".into(),
            service: String::new(),
            owner: "acct-1".into(),
            teams: Vec::new(),
        }
    }

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    /// A proof is of one challenge, one identity and one key, and an
    /// empty key proves nothing.
    #[test]
    fn a_proof_is_of_its_challenge_identity_and_key() {
        let challenge = challenge().unwrap();
        assert_eq!(challenge.len(), 64);
        assert_ne!(challenge, super::challenge().unwrap());
        let made = proof(&KEY.to_string().into(), &challenge, &me());
        assert!(proof_matches(
            &KEY.to_string().into(),
            &challenge,
            &me(),
            &made
        ));
        assert!(!proof_matches(
            &KEY.to_string().into(),
            "another",
            &me(),
            &made
        ));
        assert!(!proof_matches(
            &KEY.replace('0', "1").into(),
            &challenge,
            &me(),
            &made
        ));
        let other = Identity {
            owner: "acct-2".into(),
            ..me()
        };
        assert!(!proof_matches(
            &KEY.to_string().into(),
            &challenge,
            &other,
            &made
        ));
        assert!(!proof_matches(
            &"".to_string().into(),
            &challenge,
            &me(),
            &proof(&"".to_string().into(), &challenge, &me())
        ));
        assert!(!proof_matches(
            &KEY.to_string().into(),
            &challenge,
            &me(),
            "zz"
        ));
    }

    /// Keys two places hold are a key only while they agree.
    #[test]
    fn both_hold_a_key_only_while_they_agree() {
        let started = Binding {
            identity: me(),
            key: KEY.to_string().into(),
        };
        let same = started.duplicate();
        assert_eq!(
            Both(started.duplicate(), same).key_of(&me()),
            Some(KEY.to_string().into())
        );
        let rebound = Binding {
            key: KEY.replace('0', "1").into(),
            ..started.duplicate()
        };
        assert_eq!(Both(started.duplicate(), rebound).key_of(&me()), None);
        struct Nothing;
        impl Keys for Nothing {
            fn key_of(&self, _: &Identity) -> Option<Secret> {
                None
            }
        }
        assert_eq!(Both(started, Nothing).key_of(&me()), None);
    }

    /// A failure on the wire is `{"error"}`, with `why` unless it's a
    /// plain one; a kind it doesn't know isn't read.
    #[test]
    fn the_envelope_is_error_and_why() {
        let busy = Failure::new(Why::Busy, "the store bridge is busy: try again");
        assert_eq!(
            serde_json::to_value(&busy).unwrap(),
            serde_json::json!({"error": "the store bridge is busy: try again", "why": "busy"})
        );
        let failed = Failure::new(Why::Failed, "no");
        assert_eq!(
            serde_json::to_value(&failed).unwrap(),
            serde_json::json!({"error": "no"})
        );
        assert!(
            serde_json::from_value::<Failure>(serde_json::json!({"error": "x", "why": "later"}))
                .is_err()
        );
        // An error that's a failure already stays the one it is.
        let conflict = Failure::new(Why::Conflict, "changed");
        assert_eq!(Failure::of(conflict.clone().into()), conflict);
        assert_eq!(Failure::of(anyhow::anyhow!("plain")).why, Why::Failed);
    }

    /// A refusal names who called only once it's plain: never text a
    /// caller chose to show.
    #[test]
    fn a_refusal_shows_only_a_plain_identity() {
        let call = Proven {
            protocol: PROTOCOL.current,
            identity: Identity {
                agent: "\u{1b}[31mx".into(),
                ..me()
            },
            proof: String::new(),
        };
        let refused = checked(
            "c",
            &call,
            &Binding {
                identity: me(),
                key: KEY.to_string().into(),
            },
        )
        .unwrap_err();
        assert_eq!(refused.why, Why::Refused);
        assert!(!refused.error.contains('\u{1b}'), "{}", refused.error);
    }

    /// A caller speaking a bridge protocol below the oldest accepted
    /// is refused by name, before its proof is checked; one of this
    /// release's is taken.
    #[test]
    fn an_older_caller_is_refused_by_name() {
        let binding = Binding {
            identity: me(),
            key: KEY.to_string().into(),
        };
        let mut call = binding.prove("c");
        assert_eq!(checked("c", &call, &binding).unwrap(), me());
        call.protocol = PROTOCOL.oldest - 1;
        let refused = checked("c", &call, &binding).unwrap_err();
        assert_eq!(refused.why, Why::Refused);
        assert!(
            refused
                .error
                .contains("the caller speaks the bridge protocol 0"),
            "{}",
            refused.error
        );
    }
}

#[cfg(unix)]
pub mod socket;

#[cfg(test)]
link_test_kit::program!(system);

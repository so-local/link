//! Where an agent lives, when it isn't on a machine of yours: the
//! `agent-*` commands an adapter (`lnk-<kind>`) answers when its account
//! hosts agents, declared in this package's `actions.toml`, which an
//! adapter answering them lists under `[implements]`. The agent module
//! owns this interface; a cloud adapter is reached through the cloud
//! provider's contract ([`link_cloud_contract`]), which knows nothing of
//! agents. Its callers:
//! `lnk-agent` (moving an agent to an account that hosts it, chatting
//! with it there, and listing those it hosts), `lnk-secret` (where a
//! moved agent's keys went) and the agents' bucket store (a hosted
//! agent's Home).
//!
//! ```text
//! lnk-<kind> agent-runs  {account} -> {"runs_agents": true}
//! lnk-<kind> agent-list  {account, owner} -> {"agents": [{id, name, state}]}
//! lnk-<kind> agent-in    {account, agent: {id, name, owner}, size} -> {"move", "have"}
//!                        | {"refused": {"why", "retry_at"?}}
//! lnk-<kind> agent-out   {account, agent} -> {"move", "size"} | {"refused": {...}}
//! lnk-<kind> agent-receive  {account, move, phase} stdin: a format-4 stream
//!                        -> {"have"} (copy) | {"answers": true, "machine"} (switch)
//! lnk-<kind> agent-send  {account, move, phase, have?} stdout: a format-4 stream
//!                        (switch: the agent paused there first)
//! lnk-<kind> agent-left  {account, move} -> {"held": [message]}: its copy deleted
//! lnk-<kind> agent-abort {account, move} -> {}: a move in's copy removed, or a
//!                        move out's agent answering there again
//! lnk-<kind> agent-backup  {account, agent} -> {"at"}: the agent backed up
//!                        where it's hosted, never stopped
//! lnk-<kind> agent-restore {account, agent} -> {"at"}: its last backup
//!                        restored there
//! lnk-<kind> agent-chat  {account, agent, thread, text} stdout: JSON lines
//!                        ([`Chatting`]): the answer's pieces as they come,
//!                        then its end
//! ```
//!
//! An adapter answering the `agent-*` commands hosts agents, with no
//! box: an agent moves to its account and runs there (`lnk agent move
//! <account>`). One that doesn't (`agent-runs` fails with [`NoCommand`])
//! hosts none. The adapter owns the transport (its service's API, signed
//! in as the account's own sign-in); `lnk` only pipes a move's streams
//! (`link_core::moving`) to and from it. `agent-in` and `agent-out` start
//! a move ([`Request::agent`] is the agent; its `size` the bytes of its
//! Home and memory's index), and name it ([`Reply::move_id`]); `have` is
//! what the account holds of the agent already, by path and SHA-256.
//! `refused` is shown as it is, with `retry_at` (Unix seconds) as a local
//! time: `lnk` keeps no limit of its own. `agent-receive` reads its request as
//! one line, then the stream; `agent-send` writes the stream, the agent
//! paused there first for a switch, skipping what `have` holds the same.
//! `agent-left` ends a move out, the agent answering where it went: the
//! account deletes its copy and hands over `held`, the messages
//! ([`Held`]) that came while it was paused there, which `lnk` gives the
//! agent where it went. `agent-abort` ends a move that failed before the
//! other end answered.
//!
//! `agent-chat` sends the owner's message to a hosted agent on one of
//! the `link` connection's threads (`link:<name>`, `lnk agent chat send`), and
//! writes its answer as it comes: a [`Chatting::Piece`] a line, then one
//! last line, the [`Chatting::Answer`] whole, [`Chatting::Steered`] (it
//! joined a turn already running) or [`Chatting::Refused`]. An answer
//! nobody waited for there (the chat stopped waiting, a schedule's) isn't
//! kept for the next chat: it's in the agent's conversation. It's newer
//! than the rest and optional: an adapter without it hosts agents `lnk
//! agent chat` doesn't reach.
//!
//! `agent-backup` and `agent-restore` are newer than the rest and
//! optional: a hosted agent's backup is its host's to keep, where it runs,
//! and `at` (Unix seconds) says when the backup restored was taken. An
//! adapter without them keeps no backups of the agents it hosts, and
//! the agent module says so.
//!
//! With phase `files`, `agent-receive` and `agent-send` carry a hosted
//! agent's Home alone, for `lnk storage` ([`Request::agent`] names it, and
//! no move): `agent-receive` first prints Home's `{"have"}` as one line,
//! reads a stream of what's new, then prints `{}`; `agent-send` writes
//! what differs from `have`.
//!
//!
//!
//! Every command reads one [`Request`] as JSON on stdin (`agent-receive`'s
//! as one line, its stream after it) and prints one [`Reply`] as JSON on
//! stdout, as every adapter command does.

pub mod declared;

pub mod relay;
pub mod teams;

use link_format::WithinExt;
use std::collections::BTreeMap;

use anyhow::{bail, ensure};
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

pub use link_cloud_contract::{Account, NoCommand, account};

/// One `agent-*` command's input and output, and what they hold,
/// generated from the contract's declarations (`types.toml`).
pub use declared::{Connect, Create, Held, HostedAgent, Refused, Reply, Request};

#[expect(
    clippy::derivable_impls,
    reason = "the type is generated, so derives nothing"
)]
impl Default for Request {
    fn default() -> Self {
        Self {
            account: Account::default(),
            agent: None,
            connect: None,
            create: None,
            have: BTreeMap::new(),
            r#move: None,
            owner: None,
            phase: None,
            seal_to: None,
            size: None,
            team: None,
            text: None,
            thread: None,
        }
    }
}

impl Duplicate for Request {
    fn duplicate(&self) -> Self {
        Self {
            account: self.account.duplicate(),
            agent: self.agent.clone(),
            connect: self.connect.as_ref().map(Duplicate::duplicate),
            create: self.create.clone(),
            have: self.have.clone(),
            r#move: self.r#move.clone(),
            owner: self.owner.clone(),
            phase: self.phase.clone(),
            seal_to: self.seal_to.clone(),
            size: self.size,
            team: self.team.clone(),
            text: self.text.clone(),
            thread: self.thread.clone(),
        }
    }
}

impl Duplicate for Connect {
    fn duplicate(&self) -> Self {
        Self {
            connection: self.connection.clone(),
            token: self.token.duplicate(),
            user: self.user.clone(),
        }
    }
}

impl Reply {
    /// `agent-in`: whether the host supplies the agent's models.
    pub fn manages_models(&self) -> bool {
        self.managed_models == Some(true)
    }
}

#[expect(
    clippy::derivable_impls,
    reason = "the type is generated, so derives nothing"
)]
impl Default for HostedAgent {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            owner: None,
            state: None,
        }
    }
}

impl HostedAgent {
    /// `agent-list`: what the cloud says it's doing, or nothing.
    pub fn state(&self) -> &str {
        self.state.as_deref().unwrap_or_default()
    }
}

/// Runs `agent-*` command `command` of `kind`'s adapter with `request`,
/// its errors on this terminal, and reads its reply.
pub fn call(kind: &str, command: &str, request: &Request) -> anyhow::Result<Reply> {
    link_cloud_contract::call_as(kind, command, request)
}

/// [`call`], what the adapter says on stderr kept for the caller to show
/// or not ([`link_cloud_contract::call_quietly`]).
pub fn call_quietly(kind: &str, command: &str, request: &Request) -> anyhow::Result<Reply> {
    link_cloud_contract::call_quietly_as(kind, command, request)
}

/// In an adapter: reads the [`Request`] on the first line of `input`,
/// leaving what follows it (a stream) to read.
pub fn read_request_line(input: &mut impl std::io::BufRead) -> anyhow::Result<Request> {
    link_cloud_contract::read_request_line(input)
}

/// In an adapter: prints `reply`.
pub fn reply(reply: &Reply) -> anyhow::Result<()> {
    link_cloud_contract::reply(reply)
}

/// Starts `agent-*` command `command` with `request` as one line on its
/// stdin, left open for its stream ([`link_cloud_contract::spawn`]).
pub fn spawn(kind: &str, command: &str, request: &Request) -> anyhow::Result<std::process::Child> {
    link_cloud_contract::spawn(kind, command, request)
}

/// One line `agent-chat` writes: a piece of the answer, or its end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum Chatting {
    /// A piece of the answer, as it comes, after the pieces before it.
    Piece(String),
    /// The end: the answer, whole.
    Answer(String),
    /// The end: the message joined a turn already running, whose answer
    /// goes where that turn's does.
    Steered,
    /// The end: the account won't take a message for the agent now.
    Refused(Refused),
}

/// The longest message `agent-chat` sends: a megabyte.
pub const CHAT_MESSAGE_MAX: usize = 1 << 20;

/// The longest answer `agent-chat` takes: 16 megabytes, as a connection's
/// bridge takes from a harness.
pub const CHAT_ANSWER_MAX: usize = 16 << 20;

impl Held {
    /// An error unless it's a message `lnk` gives an agent: a connection's
    /// name, a chat and a sender of printable text, and a text at most
    /// a megabyte.
    pub fn check(&self) -> anyhow::Result<()> {
        let connection = &self.connection;
        let plain = !connection.is_empty()
            && connection.len() <= 40
            && connection.starts_with(|c: char| c.is_ascii_lowercase())
            && connection
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !plain {
            bail!("{:?} isn't a connection", link_format::clean(connection));
        }
        for (what, text) in [("chat", &self.chat), ("sender", &self.from)] {
            if text.is_empty() || text.len() > 256 || text.chars().any(char::is_control) {
                bail!(
                    "a held message's {what} isn't one: {:?}",
                    link_format::clean(text)
                );
            }
        }
        if self.text.len() > 1 << 20 {
            bail!("a held message is over a megabyte: not given");
        }
        if let Some(generation) = &self.generation
            && !(generation.len() == 32
                && generation
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        {
            bail!(
                "a held message's generation isn't a move's ID: {:?}",
                link_format::clean(generation)
            );
        }
        Ok(())
    }
}

/// An error unless `agents`, as `agent-list` answered, are agents: each
/// with an ID and a name Link's own could be, and a state of plain words.
pub fn check_hosted(agents: &[HostedAgent]) -> anyhow::Result<()> {
    for agent in agents {
        link_machine::names::check_id(&agent.id)?;
        link_machine::names::check_name(&agent.name)?;
        if agent.state().len() > 32
            || !agent
                .state()
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'-')
        {
            bail!(
                "{:?} isn't a state an agent is in",
                link_format::clean(agent.state())
            );
        }
    }
    Ok(())
}

/// The agents `account` hosts for `owner` (`agent-list`), checked; none
/// when its reply doesn't say which, never taken as hosting none.
pub fn hosted_agents(
    account: &Account,
    owner: Option<&str>,
) -> anyhow::Result<Option<Vec<HostedAgent>>> {
    let request = Request {
        account: link_secretive::Duplicate::duplicate(account),
        owner: owner.map(str::to_string),
        ..Request::default()
    };
    let reply = call_quietly(&account.kind, "agent-list", &request)?;
    if let Some(agents) = &reply.agents {
        check_hosted(agents)
            .within(format!("what {} listed", link_format::clean(&account.name)))?;
    }
    Ok(reply.agents)
}

/// Sends `text` to `agent`, hosted by `account`, as its owner, on `link`
/// thread `thread` (`agent-chat`), each piece of the answer to `progress`
/// as it comes; what came of it. The adapter's errors are shown as it
/// says them. The adapter lives no longer than the chat: dropped before
/// its answer ends (Ctrl-C), it's killed.
pub async fn chat(
    account: &Account,
    agent: &HostedAgent,
    thread: &str,
    text: &str,
    progress: &tokio::sync::watch::Sender<String>,
) -> anyhow::Result<link_harness_wire::message::Asked> {
    link_harness_wire::message::check_link_thread(thread)?;
    link_machine::names::check_id(&agent.id)?;
    ensure!(
        text.len() <= CHAT_MESSAGE_MAX,
        "a message is at most {} MB",
        CHAT_MESSAGE_MAX >> 20
    );
    let request = Request {
        account: link_secretive::Duplicate::duplicate(account),
        agent: Some(agent.clone()),
        thread: Some(thread.to_string()),
        text: Some(text.to_string()),
        ..Request::default()
    };
    chat_request(&request, progress).await
}

/// Send to the team's current lead or a named member. The hosting
/// service resolves the target and checks current membership.
pub async fn team_chat(
    account: &Account,
    team: &str,
    member: Option<&str>,
    thread: &str,
    text: &str,
    progress: &tokio::sync::watch::Sender<String>,
) -> anyhow::Result<link_harness_wire::message::Asked> {
    link_harness_wire::message::check_link_thread(thread)?;
    link_machine::names::check_id(team)?;
    if let Some(member) = member {
        link_machine::machine::check_name(member)?;
    }
    ensure!(
        !text.is_empty() && text.len() <= CHAT_MESSAGE_MAX,
        "a message is 1 byte to {CHAT_MESSAGE_MAX} bytes"
    );
    let request = Request {
        account: link_secretive::Duplicate::duplicate(account),
        team: Some(teams::Action::Chat(teams::TeamActionChat {
            team: team.into(),
            member: member.map(str::to_string),
        })),
        thread: Some(thread.into()),
        text: Some(text.into()),
        ..Request::default()
    };
    chat_request(&request, progress).await
}

async fn chat_request(
    request: &Request,
    progress: &tokio::sync::watch::Sender<String>,
) -> anyhow::Result<link_harness_wire::message::Asked> {
    let account = &request.account;
    let mut child = spawn(&account.kind, "agent-chat", request)?;
    drop(child.stdin.take());
    let said = child.stdout.take().within("no output from the adapter")?;
    let mut adapter = KilledWhenDropped(child);
    let progress = progress.clone();
    let read =
        tokio::task::spawn_blocking(move || read_chat(std::io::BufReader::new(said), &progress))
            .await
            .within("reading the adapter's answer")?;
    let status = adapter.0.wait()?;
    let kind = &account.kind;
    match read {
        Ok(asked) if status.success() => Ok(asked),
        Ok(_) => bail!("lnk-{kind} agent-chat failed (above)"),
        Err(err) if status.success() => Err(err),
        Err(_) => bail!("lnk-{kind} agent-chat failed (above)"),
    }
}

/// An adapter's process, killed when dropped unless it has ended.
struct KilledWhenDropped(std::process::Child);

impl Drop for KilledWhenDropped {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// A chat refused for now ([`Refused::retry_at`]): the message wasn't
/// taken, and may be sent again at `at` (Unix seconds). It's the error
/// [`chat`] and [`team_chat`] return, for their caller to downcast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Later {
    pub why: String,
    pub at: u64,
}

impl std::fmt::Display for Later {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.why)
    }
}

impl std::error::Error for Later {}

/// What `agent-chat` wrote on `said`, read a line at a time, each piece
/// to `progress`: an error unless every line is a [`Chatting`], the
/// pieces at most [`CHAT_ANSWER_MAX`] together, and it ends with one end.
pub fn read_chat(
    said: impl std::io::BufRead,
    progress: &tokio::sync::watch::Sender<String>,
) -> anyhow::Result<link_harness_wire::message::Asked> {
    use link_harness_wire::message::Asked;
    use std::io::BufRead;
    // Each piece once, as JSON, with room for its escapes, then the
    // answer whole the same.
    let mut lines = said
        .take(4 * CHAT_ANSWER_MAX as u64 + (1 << 20))
        .split(b'\n');
    let mut answer = String::new();
    while let Some(line) = lines.next() {
        let line = line.within("reading the agent's answer (the connection broke?)")?;
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let ended = match serde_json::from_slice::<Chatting>(&line)
            .within("the adapter's answer isn't one agent-chat writes")?
        {
            Chatting::Piece(piece) => {
                ensure!(
                    answer.len() + piece.len() <= CHAT_ANSWER_MAX,
                    "the agent's answer is over {} MB",
                    CHAT_ANSWER_MAX >> 20
                );
                answer.push_str(&piece);
                progress.send_modify(|so_far| so_far.push_str(&piece));
                continue;
            }
            Chatting::Answer(whole) => {
                ensure!(
                    whole.len() <= CHAT_ANSWER_MAX,
                    "the agent's answer is over {} MB",
                    CHAT_ANSWER_MAX >> 20
                );
                Asked::Answer(whole)
            }
            Chatting::Steered => Asked::Steered,
            Chatting::Refused(refused) => match refused.retry_at {
                Some(at) => {
                    return Err(Later {
                        why: link_format::clean(&refused.why).to_string(),
                        at,
                    }
                    .into());
                }
                None => bail!("{}", link_format::clean(&refused.why)),
            },
        };
        ensure!(
            lines.all(|line| line.is_ok_and(|l| l.iter().all(u8::is_ascii_whitespace))),
            "the adapter wrote more after the answer's end"
        );
        return Ok(ended);
    }
    bail!("the agent's answer stopped before its end (the connection broke?)")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `read_chat` makes of `said`, and the pieces it passed on.
    fn chatted(said: &str) -> (anyhow::Result<link_harness_wire::message::Asked>, String) {
        let (progress, pieces) = tokio::sync::watch::channel(String::new());
        let asked = read_chat(said.as_bytes(), &progress);
        let pieces = pieces.borrow().clone();
        (asked, pieces)
    }

    #[test]
    fn a_chat_is_its_pieces_then_one_end() {
        use link_harness_wire::message::Asked;
        let (asked, pieces) =
            chatted("{\"piece\":\"Hel\"}\n{\"piece\":\"lo\"}\n{\"answer\":\"Hello\"}\n");
        assert_eq!(asked.unwrap(), Asked::Answer("Hello".into()));
        assert_eq!(pieces, "Hello");
        let (asked, _) = chatted("\"steered\"\n");
        assert_eq!(asked.unwrap(), Asked::Steered);
        // Refused: why, as it's said.
        let (asked, _) = chatted("{\"refused\":{\"why\":\"the agent is moving\"}}\n");
        assert_eq!(asked.unwrap_err().to_string(), "the agent is moving");
        // Refused for now: when it may be sent again, typed.
        let (asked, _) =
            chatted("{\"refused\":{\"why\":\"the agent is moving\",\"retry_at\":7}}\n");
        let later = asked.unwrap_err().downcast::<Later>().unwrap();
        assert_eq!((later.why.as_str(), later.at), ("the agent is moving", 7));
        // Cut short, more after the end, or not a line agent-chat writes:
        // refused.
        let (asked, _) = chatted("{\"piece\":\"Hel\"}\n");
        assert!(asked.unwrap_err().to_string().contains("before its end"));
        let (asked, _) = chatted("{\"answer\":\"a\"}\n{\"piece\":\"b\"}\n");
        assert!(
            asked
                .unwrap_err()
                .to_string()
                .contains("after the answer's end")
        );
        let (asked, _) = chatted("{\"answer\":\"a\",\"more\":1}\n");
        assert!(asked.is_err());
        let (asked, _) = chatted("{\"held\":[]}\n");
        assert!(asked.is_err());
        // An answer over its limit, in pieces or whole.
        let piece =
            serde_json::to_string(&Chatting::Piece("x".repeat(CHAT_ANSWER_MAX / 2 + 1))).unwrap();
        let (asked, _) = chatted(&format!("{piece}\n{piece}\n"));
        assert!(asked.unwrap_err().to_string().contains("over 16 MB"));
    }

    #[test]
    fn a_chat_goes_only_to_a_link_thread() {
        assert!(link_harness_wire::message::check_link_thread("link:main").is_ok());
        for bad in [
            "telegram:42",
            "link:",
            "link:-a",
            "link:a b",
            "main",
            "link:main:x",
        ] {
            assert!(
                link_harness_wire::message::check_link_thread(bad).is_err(),
                "{bad}"
            );
        }
    }
}

#[cfg(test)]
mod wire_tests;

#[cfg(test)]
link_test_kit::program!(unit);

use super::*;

mod lines;

/// Sends `text`, the owner's new message on `thread`, to the harness's
/// endpoint, streamed: `progress` holds the answer so far while it's
/// written (a connection shows it), and the whole answer is returned. Only
/// the new message is sent: remembering the thread is the harness's.
/// With [`Bridge::proof`], only to a connection that proved it's the
/// harness ([`send`]).
pub async fn ask(
    bridge: &Bridge,
    thread: &str,
    text: &str,
    progress: &tokio::sync::watch::Sender<String>,
) -> anyhow::Result<String> {
    match ask_for(bridge, thread, text, progress).await? {
        Asked::Answer(answer) => Ok(answer),
        Asked::Steered => Ok(String::new()),
    }
}

/// The most characters a sender's name ([`crate::acp::Said::from_name`]) keeps.
pub const NAME_MAX: usize = 64;

/// A connection's name, as `lnk agent connect <connection>` takes it and its
/// module is `lnk-<name>`: lowercase letters, digits and `-`, starting
/// with a letter, at most 32. Any connection's, Link's or another's.
pub use link_agent_identity::access::is_name;

/// The prefix of the variables connection `name`'s `connect` reads its
/// secrets from: `LNK_<NAME>_`, `-` as `_` (`LNK_TELEGRAM_`).
pub fn env_prefix(name: &str) -> String {
    format!("LNK_{}_", name.to_ascii_uppercase().replace('-', "_"))
}

/// Whether `from` names a sender a message may be from: `schedule:`,
/// `task:`, `agent:` or `run:` (a background command of the agent's,
/// telling it how it ended), then an ID; a connection's user,
/// `member:<connection>:<user id>` ([`crate::access::ChatMember`]); or
/// `webhook:` then the name of one of the agent's webhooks (`lnk agent
/// webhook`), a stranger's post.
pub use link_agent_identity::access::is_from;

/// `name` as a [`crate::acp::Said::from_name`]: made safe to show ([`crate::clean`]:
/// no control character, nothing invisible or turning text around), cut
/// to [`NAME_MAX`] characters, and each byte a header can't hold (and
/// `%`) written `%XX`, so any name in any script is sent as it is.
pub fn name_header(name: &str) -> String {
    let name: String = link_format::clean(name).chars().take(NAME_MAX).collect();
    let mut header = String::with_capacity(name.len());
    for byte in name.trim().bytes() {
        match byte {
            b' '..=b'~' if byte != b'%' => header.push(byte as char),
            _ => header.push_str(&format!("%{byte:02X}")),
        }
    }
    header
}

/// The name a [`crate::acp::Said::from_name`] holds: none for one that isn't
/// [`name_header`]'s writing (a byte it would have escaped, a `%` not
/// followed by two hex digits, bytes that aren't UTF-8), that isn't safe
/// to show as it is ([`crate::clean`]), or is empty or longer than
/// [`NAME_MAX`].
pub fn name_of(header: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(header.len());
    let mut rest = header.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        match byte {
            b'%' => {
                let hex = std::str::from_utf8(after.get(..2)?).ok()?;
                bytes.push(u8::from_str_radix(hex, 16).ok()?);
                rest = &after[2..];
            }
            b' '..=b'~' => {
                bytes.push(byte);
                rest = after;
            }
            _ => return None,
        }
    }
    let name = String::from_utf8(bytes).ok()?;
    is_person_name(&name).then_some(name)
}

/// Whether `name` may be a person's name on their connection
/// ([`crate::acp::Said::from_name`]): 1 to [`NAME_MAX`] characters, nothing a
/// terminal or the model would read as more than text, nothing around
/// it.
pub fn is_person_name(name: &str) -> bool {
    let chars = name.chars().count();
    chars > 0 && chars <= NAME_MAX && link_format::clean(name) == name && name.trim() == name
}

/// Whether `name` could be a model's name ([`crate::acp::Said::model`]),
/// or an alias for one: 1 to 128 letters, digits and `-._:/@`, as
/// providers name theirs (`claude-opus-5-5`, `qwen3:8b`,
/// `anthropic/claude-haiku-5.5`).
pub fn is_model_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._:/@".contains(c))
}

/// Who a message is from when it isn't the owner ([`crate::acp::Said::from`]), the
/// name they go by when their connection gives one ([`crate::acp::Said::from_name`]),
/// and how many messages led to it ([`crate::acp::Said::hops`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Speaker<'a> {
    pub from: &'a str,
    pub name: Option<&'a str>,
    pub hops: u32,
}

/// A message for the agent from someone who isn't its owner, handed to
/// its bridge's way in ([`way_in`]) as a bridge's call's action
/// ([`crate::bridge`], [`fire`]): proved as the agent's whose scheduler or
/// tool hands it, and answered with one line ([`Fired`]) once the answer
/// went where it goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Firing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<link_interactions_contract::TransportEventId>,
    /// Exact scheduled occurrence, preserved through delivery retries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<link_interactions_contract::ScheduledOccurrence>,
    /// A subagent's (`thread` `task-<id>`): the conversation that started
    /// it, as its scheduler keeps it ([`crate::acp::Said::parent`]). A
    /// harness hosting its work in a store of its own records it as the
    /// subagent's lineage before taking the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The conversation it's in (`telegram:42`, `task-<id>`).
    pub thread: String,
    /// Who it's from ([`crate::acp::Said::from`]).
    pub from: String,
    pub text: String,
    /// Where the answer goes.
    pub answer: AnswerTo,
    /// How many messages led to it ([`crate::acp::Said::hops`]).
    #[serde(default)]
    pub hops: u32,
    /// The tools its conversation may use from now on ([`crate::acp::Said::tools`]):
    /// a subagent's, its parent's or fewer. None: it keeps what it has.
    /// Refused for a harness that doesn't keep a conversation's tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// The most its message stands ([`crate::acp::Said::standing`]): the
    /// standing of the conversation that made its schedule or subagent,
    /// which the scheduler records, or its sender's own. Every firing has
    /// one, so none fires as more than the conversation that made it.
    pub standing: link_agent_identity::access::Standing,
    /// The model its conversation runs on from now on
    /// ([`crate::acp::Said::model`]): a subagent's, as it was started
    /// with, a model or an alias. None: it keeps its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// For a test: a firing with nothing said, standing as a stranger, the
/// least. No program builds one this way: a firing says its standing.
#[cfg(any(test, feature = "test-support"))]
impl Default for Firing {
    fn default() -> Self {
        Firing {
            delivery: None,
            occurrence: None,
            parent: None,
            thread: String::new(),
            from: String::new(),
            text: String::new(),
            answer: AnswerTo::default(),
            hops: 0,
            tools: None,
            standing: link_agent_identity::access::Standing::Stranger,
            model: None,
        }
    }
}

/// Where an answer nobody waits for goes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnswerTo {
    /// The chat its thread is (`telegram:42`: Telegram chat 42).
    #[default]
    Chat,
    /// Deliver directly to this owner chat while work retains its own conversation.
    OwnerThread(String),
    /// Another conversation of the agent's, as a message from the same
    /// sender: a subagent's answer to its parent's.
    Thread(String),
    /// The owner's direct chat on a connected service, or the CLI inbox.
    Connection(String),
    /// Nowhere: it's kept in its conversation only.
    Nowhere,
}

/// What a bridge's way in answers a [`Firing`] with, once it went where
/// it goes; the error envelope ([`crate::bridge::Failure`]) when it
/// didn't.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Fired {
    /// The agent's answer, as it went.
    pub answer: String,
}

/// Hands `firing` to the way in at `socket` as `agent`'s, proved
/// ([`crate::bridge`]), and waits for its answer, once it went where it
/// goes: an error when it didn't, the way in's [`crate::bridge::Failure`]
/// when it said why, so a refusal ([`crate::bridge::Why::Refused`]) is
/// told from a failure worth trying again.
pub fn fire(
    socket: &std::path::Path,
    agent: &crate::bridge::Binding,
    firing: &Firing,
) -> anyhow::Result<Fired> {
    crate::bridge::call_at(socket, agent, firing, 16 << 20).map_err(|err| {
        match err.downcast::<crate::bridge::Failure>() {
            // The way in took it, and says why it didn't go.
            Ok(failure) => anyhow::Error::new(failure),
            Err(err) => anyhow::anyhow!("handing the message to the agent's bridge: {err:#}"),
        }
    })
}

/// The conversation of an agent's with teammate `agent` (its ID), where
/// that teammate's messages land: `agent-<id>`.
pub fn teammates_thread(agent: &str) -> String {
    format!("agent-{agent}")
}

/// The conversation of an agent's with its team `team` (the team's ID),
/// where a message to the whole team lands: `team-<id>`.
pub fn teams_thread(team: &str) -> String {
    format!("team-{team}")
}

/// Where agent `agent`'s bridge listens for messages from others: a
/// Unix socket with its settings (`~/.lnk/safe/agents/<agent>/way-in.sock`),
/// which only the user reaches.
pub fn way_in(agent: &str) -> std::path::PathBuf {
    link_files::layout::Hidden::Agents
        .path()
        .join(agent)
        .join("way-in.sock")
}

/// Where `lnk agent harness set` marks that conversations from elsewhere are on
/// their way into agent `agent`'s new harness
/// (`~/.lnk/safe/agents/<agent>/arriving`): a message waits while it's there
/// ([`after_arriving`]), so it lands after them, in the thread they hold.
pub fn arriving(agent: &str) -> std::path::PathBuf {
    link_files::layout::Hidden::Agents
        .path()
        .join(agent)
        .join("arriving")
}

/// Waits while conversations from elsewhere are arriving into agent
/// `agent`'s harness: at most [`crate::constants::ARRIVING`] from when
/// their mark was made. An expired or unreadable mark refuses delivery.
pub(crate) async fn after_arriving(agent: &str) -> anyhow::Result<()> {
    while_marked(&arriving(agent)).await
}

/// Waits while `mark` is there and younger than [`crate::constants::ARRIVING`].
#[expect(
    clippy::disallowed_methods,
    reason = "polls a mark another process removes, which nothing announces"
)]
async fn while_marked(mark: &std::path::Path) -> anyhow::Result<()> {
    let started = tokio::time::Instant::now();
    loop {
        let meta = match std::fs::symlink_metadata(mark) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            meta.is_file(),
            "conversation handoff marker is not a regular file"
        );
        let made = meta.modified()?;
        anyhow::ensure!(
            made.elapsed().unwrap_or_default() < crate::constants::ARRIVING
                && started.elapsed() < crate::constants::ARRIVING,
            "conversation handoff did not finish; retained history is safe and delivery is refused until the harness switch is retried"
        );
        tokio::time::sleep(crate::constants::ARRIVING_LOOK).await;
    }
}

pub use link_harness_wire::message::Asked;

/// [`ask`], telling an answer from a message that joined a running
/// turn.
pub async fn ask_for(
    bridge: &Bridge,
    thread: &str,
    text: &str,
    progress: &tokio::sync::watch::Sender<String>,
) -> anyhow::Result<Asked> {
    ask_from(bridge, thread, text, None, progress).await
}

/// [`ask_for`], the message from `from` ([`Speaker`]) when it isn't the
/// owner's.
pub async fn ask_from(
    bridge: &Bridge,
    thread: &str,
    text: &str,
    from: Option<Speaker<'_>>,
    progress: &tokio::sync::watch::Sender<String>,
) -> anyhow::Result<Asked> {
    ask_with(bridge, thread, text, from, None, progress).await
}

/// [`ask_from`], narrowing the conversation to `tools`
/// ([`crate::acp::Said::tools`]) when some are given: only for a harness whose
/// contract says it keeps them.
pub async fn ask_with(
    bridge: &Bridge,
    thread: &str,
    text: &str,
    from: Option<Speaker<'_>>,
    tools: Option<&[String]>,
    progress: &tokio::sync::watch::Sender<String>,
) -> anyhow::Result<Asked> {
    ask_accepted(bridge, thread, text, from, tools, None)
        .await?
        .answer(progress)
        .await
}

/// [`ask_with`] up to the moment the endpoint has taken the message: its
/// answer's headers came back a success. Its answer, still to come, is
/// read with [`Accepted::answer`]. A refusal (a status that isn't a
/// success), or an endpoint that can't be reached, is an error here,
/// before anything was taken. For a caller that answers its own sender
/// once the agent has a message, not once its turn is done (a webhook's
/// post). `model`, the model its conversation runs on from now on: a
/// subagent's ([`Firing::model`]).
pub async fn ask_accepted(
    bridge: &Bridge,
    thread: &str,
    text: &str,
    from: Option<Speaker<'_>>,
    tools: Option<&[String]>,
    model: Option<&str>,
) -> anyhow::Result<Accepted> {
    accepted(
        bridge,
        thread,
        text,
        &[],
        Sent {
            from,
            tools,
            model,
            ..Default::default()
        },
    )
    .await
}

pub async fn ask_occurrence(bridge: &Bridge, firing: &Firing) -> anyhow::Result<Accepted> {
    accepted(
        bridge,
        &firing.thread,
        &firing.text,
        &[],
        Sent {
            from: Some(Speaker {
                from: &firing.from,
                name: None,
                hops: firing.hops,
            }),
            tools: firing.tools.as_deref(),
            model: firing.model.as_deref(),
            standing: Some(firing.standing),
            occurrence: firing.occurrence.as_ref(),
            parent: firing.parent.as_deref(),
            delivery: firing.delivery.as_ref(),
            receipts: true,
        },
    )
    .await
}

/// [`ask_accepted`], the message's `text` with `parts`, its images and
/// documents ([`ask_holding`]), as a prompt on the harness's ACP.
pub(super) async fn accepted(
    bridge: &Bridge,
    thread: &str,
    text: &str,
    parts: &[Part],
    sent: Sent<'_>,
) -> anyhow::Result<Accepted> {
    // The owner's reply with a question's code answers it, on a chat it
    // was asked on, and never reaches the agent.
    let owner = sent.from.is_none();
    if owner
        && parts.is_empty()
        && let Some(said) = super::questions::caught(&bridge.agent, thread, text)
    {
        return Ok(Accepted {
            taken: Taken::Answered(said),
        });
    }
    // The owner addressing one of its agent's teams, `@<team>` or
    // `@<team>/<member>`: through the owner's team chat, never this
    // agent's own turn.
    if owner
        && parts.is_empty()
        && !bridge.identity.teams.is_empty()
        && let Some(said) = super::addressing::said(text).await
    {
        return Ok(Accepted {
            taken: Taken::Answered(said),
        });
    }
    started(bridge).await;
    anyhow::ensure!(
        parts.is_empty() || bridge.parts,
        "the agent reads text only"
    );
    // While a turn a person started on a chat is under way, the owner's
    // or a member's, its questions are asked on that connection.
    let person = owner
        || sent
            .from
            .is_some_and(|from| from.from.parse::<crate::access::ChatMember>().is_ok());
    let turn = person
        .then(|| super::questions::turn(&bridge.agent, thread))
        .flatten();
    // A person spoke: the agents' chain in this conversation starts again.
    if person && !bridge.identity.agent.is_empty() {
        let ended = link_interactions::chains::Chains::machine()
            .and_then(|chains| chains.person_spoke(&bridge.identity.agent, thread));
        if let Err(err) = ended {
            link_format::daemon::warn!("ending a conversation's chain: {err:#}");
        }
    }
    let taking = acp::client::asked(bridge, thread, text, parts, sent).await?;
    Ok(Accepted {
        taken: Taken::Taking(taking, turn),
    })
}

/// A message the harness has taken ([`ask_accepted`]), its answer still
/// to read; or the owner's answer to a question, which it never saw.
pub struct Accepted {
    taken: Taken,
}

enum Taken {
    Taking(
        acp::client::Taking,
        Option<link_interactions::presence::Presence>,
    ),
    Answered(String),
}

impl Accepted {
    /// Its answer, read to its end, each piece told `progress` as it
    /// comes: whole, or that it joined a turn already running.
    pub async fn answer(
        self,
        progress: &tokio::sync::watch::Sender<String>,
    ) -> anyhow::Result<Asked> {
        self.answer_received(progress)
            .await
            .map(|(answer, _)| answer)
    }

    /// A host-kept response is acknowledged only after its destination accepted delivery.
    pub async fn answer_received(
        self,
        progress: &tokio::sync::watch::Sender<String>,
    ) -> anyhow::Result<(Asked, Option<String>)> {
        match self.taken {
            Taken::Taking(taking, _turn) => answered(taking, progress).await,
            Taken::Answered(said) => Ok((Asked::Answer(said), None)),
        }
    }
}

/// A message's answer on ACP, each piece told `progress` as it comes,
/// at most [`endpoint::ANSWER_TIMEOUT`] between anything of its turn.
async fn answered(
    mut taking: acp::client::Taking,
    progress: &tokio::sync::watch::Sender<String>,
) -> anyhow::Result<(Asked, Option<String>)> {
    let (mut answer, mut called) = (String::new(), Vec::new());
    let look = taking.look.clone();
    loop {
        let streamed = tokio::time::timeout(endpoint::ANSWER_TIMEOUT, taking.streamed.recv())
            .await
            .map_err(|_| anyhow::anyhow!("the agent didn't answer"))?;
        match streamed {
            None => break,
            Some(acp::client::Streamed::Moved) => {}
            Some(acp::client::Streamed::Called(tool)) => {
                called.push(tool);
                progress.send_replace(lines::activity(&called, &answer, &look));
            }
            Some(acp::client::Streamed::Piece(piece)) => {
                anyhow::ensure!(
                    answer.len() + piece.len() <= MAX_WHOLE,
                    "the agent's answer is over {} MB",
                    MAX_WHOLE >> 20
                );
                // Below its calls, if it made any.
                let first = answer.is_empty() && !called.is_empty();
                answer.push_str(&piece);
                progress.send_modify(|so_far| {
                    if first {
                        so_far.push_str("\n\n");
                    }
                    so_far.push_str(&piece);
                });
            }
        }
    }
    let ended = taking
        .ended
        .await
        .map_err(|_| anyhow::anyhow!("the agent's connection ended before its answer"))??;
    match ended.steered {
        true => Ok((Asked::Steered, ended.receipt)),
        false if taking.keep => Ok((
            Asked::Answer(lines::kept(&called, &answer, &look)),
            ended.receipt,
        )),
        false => Ok((Asked::Answer(answer), ended.receipt)),
    }
}

/// What a bridge `POST`s to [`crate::acp::RECOVER`], or asks as
/// [`crate::acp::RECOVER`].
#[derive(
    Clone, Debug, Default, PartialEq, Serialize, Deserialize, agent_client_protocol::JsonRpcRequest,
)]
#[request(method = "_link/recover", response = crate::acp::RecoveredAnswers)]
pub struct Recover {
    /// Each answer kept, with its `id`, until it's said done.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub two_phase: bool,
    /// The answers that went where they go, by `id`: forgotten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub done: Vec<String>,
}

/// An answer of a turn finished after a crash: nobody waited for it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Recovered {
    /// Its conversation (`telegram:42`).
    pub thread: String,
    pub text: String,
    /// What to say it went by ([`Recover::done`]), when it was asked for
    /// with [`Recover::two_phase`]; none, it was handed over once, from
    /// a harness that keeps nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// The answers `bridge`'s harness finished after a crash and hasn't been
/// told went where they go ([`crate::acp::RECOVER`]), asked for in two phases:
/// say each one that went with [`recovered_done`].
pub async fn recovered(bridge: &Bridge) -> anyhow::Result<Vec<Recovered>> {
    let asked = Recover {
        two_phase: true,
        ..Recover::default()
    };
    let body = recover(bridge, &asked).await?;
    Ok(serde_json::from_value(body["answers"].clone()).unwrap_or_default())
}

/// Tells `bridge`'s harness the recovered answers `done` (their ids) went
/// where they go: it forgets them.
pub async fn recovered_done(bridge: &Bridge, done: Vec<String>) -> anyhow::Result<()> {
    if done.is_empty() {
        return Ok(());
    }
    let said = Recover {
        done,
        ..Recover::default()
    };
    recover(bridge, &said).await.map(drop)
}

/// Asks `body` as [`crate::acp::RECOVER`]; its answer.
async fn recover(bridge: &Bridge, body: &Recover) -> anyhow::Result<serde_json::Value> {
    let answers = acp::client::recover(bridge, body.clone()).await?;
    Ok(serde_json::to_value(answers)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A message waits while conversations are arriving, goes once
    /// they're in, and never waits on a mark past the time they may take.
    #[tokio::test]
    async fn a_message_waits_while_conversations_arrive() {
        let dir = link_test_kit::TempDir::new("arriving");
        let mark = dir.join("arriving");
        std::fs::write(&mark, b"").unwrap();
        let held =
            tokio::time::timeout(crate::constants::ARRIVING_LOOK * 2, while_marked(&mark)).await;
        assert!(held.is_err(), "it went while they were arriving");
        std::fs::remove_file(&mark).unwrap();
        while_marked(&mark).await.unwrap();
        let old = std::time::SystemTime::now() - crate::constants::ARRIVING;
        std::fs::File::create(&mark)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(
            while_marked(&mark).await.is_err(),
            "an expired handoff must never authorize an empty continuation"
        );
    }

    /// A sender is a kind Link knows and an ID: a background command's
    /// `run:` among them, never the owner's, never anything else; or one
    /// of the agent's webhooks, by a name Link takes.
    #[test]
    fn who_a_message_may_be_from() {
        for good in ["schedule:", "task:", "agent:", "run:", "webhook:"] {
            assert!(is_from(&format!("{good}a1b2c3d4e5f6")), "{good}");
        }
        assert!(is_from("webhook:ci"));
        for good in [
            "member:slack:T0ACME/U0ALICE",
            "member:telegram:42",
            "member:discord:200000000000000002",
        ] {
            assert!(is_from(good), "{good}");
        }
        for bad in [
            // An account's person, a chat, or a connection's user without
            // their connection or id: not who a message is from.
            "user:0a1b2c3d",
            "member:slack:",
            "user:Slack:U0ALICE",
            "member:slack:U0 ALICE",
            "chat:slack:C0TEAM",
            "owner:a1b2c3d4e5f6",
            "run:",
            "run:../x",
            "run",
            "a1b2c3d4e5f6",
            "webhook:",
            "webhook:../x",
            "webhook:a b",
        ] {
            assert!(!is_from(bad), "{bad}");
        }
    }

    /// A sender's name, as a header and back: any script, `%` itself, and
    /// nothing a header can't hold or a model would read as a line.
    #[test]
    fn a_senders_name() {
        for name in ["Alice", "Zoë Ångström", "李雷", "50% off", "a\"b"] {
            let header = name_header(name);
            assert!(
                header.bytes().all(|b| (b' '..=b'~').contains(&b)),
                "{header}"
            );
            assert_eq!(name_of(&header).as_deref(), Some(name), "{header}");
        }
        assert_eq!(
            name_of(&name_header("Eve\nOwner: yes")).unwrap(),
            "EveOwner: yes"
        );
        assert_eq!(
            name_of(&name_header(&"x".repeat(100))).unwrap().len(),
            NAME_MAX
        );
        assert_eq!(name_header("  \u{7}  "), "");
        // Nothing invisible, or turning the text around.
        assert_eq!(
            name_of(&name_header("Eve\u{202E}renwo\u{200B}")).unwrap(),
            "Everenwo"
        );
        assert_eq!(name_of("Eve%E2%80%AEx"), None);
        for bad in [
            "",
            "%",
            "%4",
            "%ZZ",
            "%0A",
            "%FF",
            "é",
            " x",
            &"x".repeat(NAME_MAX + 1),
        ] {
            assert_eq!(name_of(bad), None, "{bad}");
        }
    }

    /// A conversation's tools, as a header and back: none is a list too,
    /// and a list holding anything but tools' names is none at all.
    #[test]
    fn a_conversations_tools() {
        let tools = vec!["memory__search".to_string(), "kit__stamp".to_string()];
        let header = tools_header(&tools).unwrap();
        assert_eq!(header, "memory__search,kit__stamp");
        assert_eq!(tools_of(&header), Some(tools));
        assert_eq!(tools_of(""), Some(Vec::new()));
        assert_eq!(tools_of(" "), Some(Vec::new()));
        assert_eq!(tools_of("a, b,a"), Some(vec!["a".into(), "b".into()]));
        for bad in ["a,,b", "a b", "../x", "a;b", &"x".repeat(129)] {
            assert_eq!(tools_of(bad), None, "{bad}");
        }
        let many: Vec<String> = (0..=TOOLS_MAX).map(|n| format!("t{n}")).collect();
        assert_eq!(tools_of(&many.join(",")), None);
        assert!(tools_header(&many).is_err());
        assert!(tools_header(&["a\r\nX-Evil: 1".into()]).is_err());
        assert_eq!(tools_header(&[]).unwrap(), "");
    }

    /// A harness's ACP on a port of its own: each prompt told taken, then
    /// answered `taken` only once `release` says; one whose text is
    /// `refuse` refused before it's taken.
    async fn holding() -> (u16, tokio::sync::oneshot::Sender<()>) {
        use agent_client_protocol::schema::ProtocolVersion;
        use agent_client_protocol::schema::v1::{
            ContentBlock, ContentChunk, InitializeRequest, InitializeResponse, PromptRequest,
            PromptResponse, ResumeSessionRequest, ResumeSessionResponse, SessionNotification,
            SessionUpdate, StopReason, TextContent,
        };
        use agent_client_protocol::{self as sdk, Lines};
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release, released) = tokio::sync::oneshot::channel::<()>();
        let released = std::sync::Arc::new(tokio::sync::Mutex::new(Some(released)));
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                // A look whether it's up is answered, and let go.
                let mut head = [0u8; 4];
                if stream.peek(&mut head).await.is_ok() && &head == b"HEAD" {
                    use tokio::io::AsyncWriteExt;
                    let _ = stream.write_all(b"HTTP/1.0 200 OK\r\n\r\n").await;
                    continue;
                }
                let Ok(socket) = tokio_tungstenite::accept_async(stream).await else {
                    continue;
                };
                let released = released.clone();
                tokio::spawn(async move {
                    let (sink, stream) = socket.split();
                    let outgoing =
                        sink.sink_map_err(std::io::Error::other)
                            .with(|line: String| async move {
                                Ok::<_, std::io::Error>(Message::Text(line.into()))
                            });
                    let incoming = stream.filter_map(|frame| async move {
                        match frame {
                            Ok(Message::Text(text)) => Some(Ok(text.to_string())),
                            Ok(_) => None,
                            Err(err) => Some(Err(std::io::Error::other(err))),
                        }
                    });
                    let _ = sdk::Agent
                        .builder()
                        .on_receive_request(
                            async |_: InitializeRequest, responder, _cx| {
                                responder.respond(InitializeResponse::new(ProtocolVersion::V1))
                            },
                            sdk::on_receive_request!(),
                        )
                        .on_receive_request(
                            async |_: ResumeSessionRequest, responder, _cx| {
                                responder.respond(ResumeSessionResponse::new())
                            },
                            sdk::on_receive_request!(),
                        )
                        .on_receive_request(
                            async move |asked: PromptRequest, responder, cx| {
                                let refused = matches!(&asked.prompt[..],
                                    [ContentBlock::Text(t)] if t.text == "refuse");
                                if refused {
                                    return responder.respond_with_error(
                                        sdk::Error::invalid_params().data("not now"),
                                    );
                                }
                                let session = asked.session_id;
                                cx.send_notification(crate::acp::Taken {
                                    session_id: session.clone(),
                                })?;
                                let released = released.clone();
                                tokio::spawn(async move {
                                    if let Some(released) = released.lock().await.take() {
                                        let _ = released.await;
                                    }
                                    let chunk = ContentChunk::new(ContentBlock::Text(
                                        TextContent::new("taken"),
                                    ));
                                    let _ = cx.send_notification(SessionNotification::new(
                                        session,
                                        SessionUpdate::AgentMessageChunk(chunk),
                                    ));
                                    let _ =
                                        responder.respond(PromptResponse::new(StopReason::EndTurn));
                                });
                                Ok(())
                            },
                            sdk::on_receive_request!(),
                        )
                        .connect_to(Lines::new(outgoing, incoming))
                        .await;
                });
            }
        });
        (port, release)
    }

    fn at(port: u16) -> Bridge {
        Bridge {
            agent: "main".into(),
            endpoint: format!("http://127.0.0.1:{port}/acp"),
            ..Default::default()
        }
    }

    /// A message is accepted once the harness says it's taken, before any
    /// of the answer: the answer is read after.
    #[tokio::test]
    async fn a_message_is_accepted_before_its_answer() {
        let (port, release) = holding().await;
        let bridge = at(port);
        let accepted = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            ask_accepted(&bridge, "webhook:deploys", "hello", None, Some(&[]), None),
        )
        .await
        .expect("accepted with its answer held back")
        .unwrap();
        release.send(()).unwrap();
        let (progress, _) = tokio::sync::watch::channel(String::new());
        assert_eq!(
            accepted.answer(&progress).await.unwrap(),
            Asked::Answer("taken".into())
        );
        assert_eq!(*progress.borrow(), "taken");
    }

    /// A refusal is no acceptance: an error, before anything is taken.
    #[tokio::test]
    async fn a_refused_message_is_never_accepted() {
        let (port, _release) = holding().await;
        let err = ask_accepted(&at(port), "webhook:deploys", "refuse", None, None, None)
            .await
            .err()
            .expect("refused");
        assert!(format!("{err:#}").contains("not now"), "{err:#}");
        // Nothing listening at all: no acceptance either.
        let held = link_test_kit::HeldNumber::port();
        let gone = held.port_number();
        assert!(
            ask_accepted(&at(gone), "webhook:deploys", "hello", None, None, None)
                .await
                .is_err()
        );
    }

    /// Two tools called in one turn each show their line, in the order
    /// called: above the finished answer where they're kept, gone from it
    /// where they're replaced, and never where activity is off.
    #[tokio::test]
    async fn each_call_of_a_turn_shows_as_its_spec_says() {
        use crate::acp::fake::{self, Turn};
        use crate::policy::{Activity, Connection};
        let harness = fake::serve_script(
            |_| {
                Turn::Working(
                    vec![
                        ("file__read".into(), "file read notes/plan.md".into()),
                        ("run__command".into(), "run command cargo test".into()),
                    ],
                    Box::pin(futures_util::stream::once(std::future::ready(
                        "Done.".to_string(),
                    ))),
                )
            },
            |_| Vec::new(),
            |_| {},
        )
        .await;
        let lines = "› file read: notes/plan.md\n› run command: cargo test";
        for (activity, answer, shown) in [
            (
                Activity::Keep,
                format!("{lines}\n\nDone."),
                format!("{lines}\n\nDone."),
            ),
            (
                Activity::Replace,
                "Done.".to_string(),
                format!("{lines}\n\nDone."),
            ),
            (Activity::Off, "Done.".to_string(), "Done.".to_string()),
        ] {
            let bridge = Bridge {
                agent: "main".into(),
                endpoint: harness.endpoint.clone(),
                spec: Some(Connection {
                    activity: Some(activity),
                    activity_mark: Some("›".into()),
                    activity_separator: Some(": ".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let (progress, _) = tokio::sync::watch::channel(String::new());
            let asked = ask_accepted(&bridge, "telegram:42", "hello", None, None, None)
                .await
                .unwrap()
                .answer(&progress)
                .await
                .unwrap();
            assert_eq!(asked, Asked::Answer(answer), "{activity:?}");
            assert_eq!(*progress.borrow(), shown, "{activity:?}");
        }
    }
}

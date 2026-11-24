//! `lnk-signal link` and `lnk-signal bridge`: Link linking the agent's
//! number and carrying its messages for a harness that speaks no Signal
//! (`link_connection_contract`).
//!
//! Both run `signal-cli`, linked as a device into the connection's own
//! folder ([`Bridge::state`]), which the harness never reaches: the one
//! `crate::native` installs there, or the program in
//! `LNK_SIGNAL_CLI`. The bridge speaks JSON-RPC with it over its stdin and
//! stdout (`signal-cli jsonRpc`), never its HTTP daemon: a port on the
//! loopback would let anyone on the machine send as the agent. Each direct
//! text message from an allowed number goes to the endpoint alone, its
//! thread `signal:<the number's digits>`. Signal limits how often a
//! message can be edited, so the answer isn't shown as it's written:
//! "typing" is, and then the whole answer, its formatting as Signal's
//! text styles ([`crate::format`]), split into more messages past its
//! limit. An allowed number's mention of the agent's in a group goes
//! too, without the mention, its thread `signal:g-<the group's id in
//! base64url>`; the answer is sent to the group the same way.
//!
//! `signal-cli` holds the account while the bridge runs, so `lnk-signal
//! send` (an answer nobody waits for) hands its message to the running
//! bridge ([`connection::send_through`]), which sends it through `signal-cli`.

use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use link_connection_contract as connection;
use link_connection_contract::{Bridge, rich};
use link_format::Out;
use link_harness_contract::harness::Signal;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::ChildStdin,
    sync::{Mutex, mpsc, oneshot},
};

/// How often "typing" is sent while an answer is written: Signal shows
/// it for 15 seconds.
const TYPING: Duration = Duration::from_secs(10);

/// The longest wait for `signal-cli` to answer a request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// The variable naming a `signal-cli` of the user's own (or a test's)
/// to run instead of the one Link installs.
pub const PROGRAM_ENV: &str = "LNK_SIGNAL_CLI";

fn settings(bridge: &Bridge) -> anyhow::Result<Signal> {
    let settings: Signal = bridge
        .settings
        .expose()
        .read()
        .context("reading the Signal settings on stdin")?;
    anyhow::ensure!(
        connection::is_number(&settings.account),
        "the agent's Signal number {:?} isn't a phone number",
        settings.account
    );
    anyhow::ensure!(
        bridge.state.is_absolute(),
        "no folder to keep Signal's account in"
    );
    Ok(settings)
}

/// The command running `signal-cli` with its account in `state`: the
/// program in [`PROGRAM_ENV`], or the one Link installs there (the first
/// time, a download), run with `HOME` set to `state`.
fn signal_cli(state: &Path, account: &str) -> anyhow::Result<Command> {
    match std::env::var_os(PROGRAM_ENV) {
        Some(program) => {
            std::fs::create_dir_all(crate::native::data_in(state))?;
            let mut command = link_secretive::process::command(program);
            command.arg("--config").arg(crate::native::data_in(state));
            Ok(command)
        }
        None => {
            let program = crate::native::install_in(state, account)?;
            let mut command = link_secretive::process::command(program);
            command.env("HOME", state);
            Ok(command)
        }
    }
}

/// Links the agent's number, as `lnk-signal link [--again]`: whether it's
/// linked. Its code is shown on stdout, since stdin carries `bridge`.
pub fn link(bridge: Bridge, again: bool, out: &mut Out) -> anyhow::Result<bool> {
    let settings = settings(&bridge)?;
    let terminal = link_format::streams::stdout_is_terminal();
    let state = &bridge.state;
    crate::native::link_in(state, &settings.account, again, terminal, out, || {
        signal_cli(state, &settings.account)
    })
}

/// The owner's own chats: each allowed number's, its digits
/// (`lnk-signal owner`).
pub fn owner(bridge: &Bridge) -> anyhow::Result<Vec<String>> {
    let settings: Signal = bridge
        .settings
        .expose()
        .read()
        .context("reading the Signal settings")?;
    Ok(settings
        .allowed_users
        .iter()
        .filter_map(|number| number.strip_prefix('+'))
        .map(str::to_string)
        .collect())
}

/// Sends `text` to chat `chat` (as its thread names it: the number's
/// digits, or `g-<the group's id in base64url>`), a message nobody asked
/// for (an answer to a schedule or a subagent), through the running
/// bridge: only to an allowed number's own chat, or a group an allowed
/// number talked to the agent in while its spec lets it answer in groups;
/// never another group or anyone else.
pub async fn send(
    bridge: Bridge,
    chat: String,
    text: String,
) -> anyhow::Result<connection::SentMessages> {
    let settings = settings(&bridge)?;
    anyhow::ensure!(
        may_send(&settings, &bridge, &chat),
        "Signal chat {chat} isn't an allowed number's own, nor a group they talk to the agent \
         in: not sending there"
    );
    connection::send_through(&bridge.state, &bridge.binding(), &chat, &text).await?;
    Ok(connection::SentMessages::default())
}

/// Whether `send` may send to `chat`: an allowed number's own chat, or a
/// group they talked to the agent in.
fn may_send(settings: &Signal, bridge: &Bridge, chat: &str) -> bool {
    match chat.strip_prefix("g-") {
        Some(_) => bridge.answers_groups() && connection::knows_group(&bridge.state, chat),
        None => settings.allowed_users.contains(&format!("+{chat}")),
    }
}

/// Whom `signal-cli` sends to for `chat`, as its thread names it: a
/// number's digits (`{"recipient": ["+..."]}`), or `g-<id in base64url>`
/// (`{"groupId": "<id in base64>"}`).
fn recipient(chat: &str) -> Option<Value> {
    match chat.strip_prefix("g-") {
        Some(url) => {
            let mut id: String = url
                .chars()
                .map(|c| match c {
                    '-' => '+',
                    '_' => '/',
                    c => c,
                })
                .collect();
            while !id.len().is_multiple_of(4) {
                id.push('=');
            }
            (group_thread(&id)?.strip_prefix("signal:")? == chat).then(|| json!({"groupId": id}))
        }
        None => {
            let number = format!("+{chat}");
            connection::is_number(&number).then(|| json!({"recipient": [number]}))
        }
    }
}

/// Carries messages until it fails for good; `signal-cli` is started
/// again whenever it stops.
pub async fn carry(bridge: Bridge) -> anyhow::Result<()> {
    let settings = settings(&bridge)?;
    if crate::native::linked_in(&bridge.state).as_deref() != Some(settings.account.as_str()) {
        bail!(
            "Signal isn't linked to {}: lnk agent connect signal in a terminal links it",
            settings.account
        );
    }
    link_connection_contract::note(&format!("Carrying {}'s Signal messages", bridge.agent));
    let rpc = Rpc::default();
    // signal-cli is read while answers are written: one at a time and in
    // order in each thread, threads side by side.
    let (queue, messages) = mpsc::unbounded_channel::<Message>();
    let answering =
        connection::per_thread_replying(messages, Message::thread, Message::text, |message| {
            let (rpc, bridge) = (&rpc, &bridge);
            async move { message.answer(rpc, bridge).await }
        });
    // What `send` hands it, sent in its chat.
    let sends = async {
        let sent = connection::take_sends(&bridge.state, bridge.sends_keys(), |chat, text| {
            let rpc = &rpc;
            async move {
                let to =
                    recipient(&chat).with_context(|| format!("{chat:?} isn't a Signal chat"))?;
                if !rpc.say(&to, &text).await {
                    bail!("signal-cli didn't send it");
                }
                Ok(())
            }
        });
        if let Err(err) = sent.await {
            link_connection_contract::warn(&format!("Signal: not taking what's sent: {err:#}"));
        }
        std::future::pending::<()>().await
    };
    tokio::select! {
        () = listen(&bridge.state, &settings, &rpc, queue) => {}
        () = answering => {}
        () = sends => {}
    }
    Ok(())
}

/// Runs `signal-cli`, starting it again whenever it stops.
#[expect(
    clippy::disallowed_methods,
    reason = "backs off before starting signal-cli's connection again"
)]
async fn listen(state: &Path, settings: &Signal, rpc: &Rpc, queue: mpsc::UnboundedSender<Message>) {
    let mut seen = Seen::default();
    let mut wait = Duration::from_secs(1);
    loop {
        let started = Instant::now();
        if let Err(err) = session(state, settings, rpc, &queue, &mut seen).await {
            link_connection_contract::warn(&format!(
                "signal-cli: {err:#}; starting it again in {}s",
                wait.as_secs()
            ));
        }
        rpc.close().await;
        if started.elapsed() > Duration::from_secs(60) {
            wait = Duration::from_secs(1);
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(60));
    }
}

/// One run of `signal-cli jsonRpc`, until it stops.
async fn session(
    state: &Path,
    settings: &Signal,
    rpc: &Rpc,
    queue: &mpsc::UnboundedSender<Message>,
    seen: &mut Seen,
) -> anyhow::Result<()> {
    let mut command = tokio::process::Command::from(signal_cli(state, &settings.account)?);
    command
        .args(["-a", &settings.account, "jsonRpc"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().context("running signal-cli")?;
    let stdin = child.stdin.take().context("no stdin for signal-cli")?;
    let stdout = child.stdout.take().context("no stdout from signal-cli")?;
    rpc.open(stdin).await;
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await.context("reading signal-cli")? {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value["method"] == "receive" {
            if let Some(message) = owners(&value["params"]["envelope"], settings)
                && seen.first((&message.from, message.timestamp))
            {
                let _ = queue.send(message);
            }
        } else if value.get("id").is_some() {
            rpc.answered(value).await;
        }
    }
    let status = child.wait().await?;
    bail!("it stopped ({status})")
}

/// Requests to the running `signal-cli`, and the answers it owes.
#[derive(Default)]
struct Rpc(Mutex<Calls>);

#[derive(Default)]
struct Calls {
    stdin: Option<ChildStdin>,
    next: u64,
    waiting: HashMap<u64, oneshot::Sender<Value>>,
}

impl Rpc {
    async fn open(&self, stdin: ChildStdin) {
        self.0.lock().await.stdin = Some(stdin);
    }

    /// Forgets the `signal-cli` that stopped: what it owed fails.
    async fn close(&self) {
        let mut calls = self.0.lock().await;
        calls.stdin = None;
        calls.waiting.clear();
    }

    /// Hands `reply` to the request it answers.
    async fn answered(&self, reply: Value) {
        let Some(id) = reply["id"].as_u64() else {
            return;
        };
        if let Some(waiting) = self.0.lock().await.waiting.remove(&id) {
            let _ = waiting.send(reply);
        }
    }

    /// Asks `method` with `params`; its result.
    async fn call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let (send, reply) = oneshot::channel();
        {
            let mut calls = self.0.lock().await;
            calls.next += 1;
            let id = calls.next;
            let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            let stdin = calls.stdin.as_mut().context("signal-cli isn't running")?;
            stdin
                .write_all(format!("{request}\n").as_bytes())
                .await
                .context("writing to signal-cli")?;
            stdin.flush().await?;
            calls.waiting.insert(id, send);
        }
        let reply = tokio::time::timeout(REPLY_TIMEOUT, reply)
            .await
            .context("signal-cli didn't answer")?
            .context("signal-cli stopped")?;
        if let Some(error) = reply.get("error") {
            bail!(
                "{}",
                link_format::clean(error["message"].as_str().unwrap_or("failed"))
            );
        }
        Ok(reply["result"].clone())
    }

    /// Says `text`, Markdown, to `to`: each of its messages sent in
    /// turn, its formatting as Signal's styles; whether all were.
    async fn say(&self, to: &Value, text: &str) -> bool {
        for piece in crate::format::pieces(text) {
            if !self.send(to, &piece).await {
                return false;
            }
        }
        true
    }

    /// Sends `message` to `to` (`{"recipient": [...]}` or `{"groupId": ...}`),
    /// trying again a few times; whether it was.
    #[expect(
        clippy::disallowed_methods,
        reason = "backs off before sending again through signal-cli"
    )]
    async fn send(&self, to: &Value, message: &rich::Styled) -> bool {
        let params = crate::format::with(to.clone(), message);
        let mut wait = Duration::from_secs(1);
        for _ in 0..3 {
            match self.call("send", params.clone()).await {
                Ok(_) => return true,
                Err(err) => link_connection_contract::warn(&format!("signal-cli send: {err:#}")),
            }
            tokio::time::sleep(wait).await;
            wait *= 2;
        }
        false
    }

    /// Shows `to` "typing", or stops it.
    async fn typing(&self, to: &Value, stop: bool) {
        let mut params = to.clone();
        params["stop"] = stop.into();
        let typing = self.call("sendTyping", params);
        if let Ok(Err(err)) = tokio::time::timeout(Duration::from_secs(5), typing).await {
            link_connection_contract::warn(&format!("signal-cli sendTyping: {err:#}"));
        }
    }
}

/// A message from someone allowed, for the agent.
#[derive(Debug, PartialEq)]
struct Message {
    /// Their number (`+15551234567`).
    from: String,
    /// When they sent it: its id, with the number.
    timestamp: u64,
    /// None: something without text (a picture, a sticker).
    text: Option<String>,
    /// Its attachments, by the ids signal-cli keeps them under.
    files: Vec<connection::Held<String>>,
    /// The group it mentions the agent in (its id, base64), where the
    /// answer goes; none for a direct message.
    group: Option<String>,
}

/// What `envelope` is to the bridge: a new direct message from an allowed
/// number, or its mention of the agent's number in a group, without the
/// mention. Anything else (a group's message not mentioning it, a mention
/// with no text left, a copy of the agent's own from another of its
/// devices, a reaction, a receipt) is left unanswered.
fn owners(envelope: &Value, settings: &Signal) -> Option<Message> {
    let from = envelope["sourceNumber"].as_str()?;
    let data = envelope["dataMessage"].as_object()?;
    let allowed = settings.allowed_users.iter().any(|a| a == from);
    let own = from == settings.account;
    if own || !allowed || !connection::is_number(from) {
        return None;
    }
    let timestamp = envelope["timestamp"].as_u64()?;
    let group = data.get("groupInfo").filter(|v| !v.is_null());
    if let Some(group) = group {
        let id = group["groupId"].as_str().filter(|id| !id.is_empty())?;
        let mentions: Vec<&Value> = data
            .get("mentions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|mention| mention["number"] == settings.account.as_str())
            .collect();
        if mentions.is_empty() {
            return None;
        }
        let text = data
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = without_mentions(text, &mentions);
        let files = files(data);
        let held = !(text.is_empty() && files.is_empty());
        return (held && group_thread(id).is_some()).then(|| Message {
            from: from.to_string(),
            timestamp,
            text: Some(text),
            files,
            group: Some(id.to_string()),
        });
    }
    let text = data
        .get("message")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string);
    let attached = data
        .get("attachments")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty())
        || data.get("sticker").is_some_and(|s| !s.is_null());
    if text.is_none() && !attached {
        return None;
    }
    Some(Message {
        from: from.to_string(),
        timestamp,
        text,
        files: files(data),
        group: None,
    })
}

/// The attachments `data` holds, each by the id signal-cli keeps it
/// under, in its account's `attachments/`; one whose id would name
/// another file is left out.
fn files(data: &serde_json::Map<String, Value>) -> Vec<connection::Held<String>> {
    let id = |id: &str| {
        !id.is_empty()
            && !id.starts_with('.')
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    };
    data.get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| {
            let at = file["id"].as_str().filter(|at| id(at))?;
            Some(connection::Held {
                media: file["contentType"].as_str().unwrap_or_default().to_string(),
                name: file["filename"].as_str().map(str::to_string),
                size: file["size"].as_u64(),
                at: at.to_string(),
            })
        })
        .collect()
}

/// The bytes of the attachment signal-cli keeps as `id` for the account
/// in `state`.
async fn attachment(state: &Path, id: &str) -> anyhow::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let path = crate::native::data_in(state).join("attachments").join(id);
    let file = tokio::fs::File::open(&path)
        .await
        .with_context(|| format!("opening {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(connection::PART_MAX + 1)
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes)
}

/// `text` without the spans `mentions` mark (each its `start` and
/// `length`, in UTF-16 units, over a placeholder), trimmed; as it is if
/// one doesn't fit it.
fn without_mentions(text: &str, mentions: &[&Value]) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut spans = Vec::new();
    for mention in mentions {
        let start = mention["start"]
            .as_u64()
            .and_then(|s| usize::try_from(s).ok());
        let length = mention["length"]
            .as_u64()
            .and_then(|l| usize::try_from(l).ok());
        match (start, length) {
            (Some(start), Some(length)) if start + length <= units.len() => {
                spans.push((start, start + length));
            }
            _ => return text.trim().to_string(),
        }
    }
    spans.sort_unstable();
    let mut kept = Vec::with_capacity(units.len());
    let mut at = 0;
    for (start, end) in spans {
        if start >= at {
            kept.extend_from_slice(&units[at..start]);
            at = end;
        }
    }
    kept.extend_from_slice(&units[at..]);
    String::from_utf16_lossy(&kept).trim().to_string()
}

/// The messages already carried, the latest 100: a message is carried
/// once however often it comes.
#[derive(Default)]
struct Seen(VecDeque<(String, u64)>);

impl Seen {
    /// Whether the message `(from, timestamp)` names is new, remembering
    /// it.
    fn first(&mut self, (from, timestamp): (&str, u64)) -> bool {
        let key = (from.to_string(), timestamp);
        if self.0.contains(&key) {
            return false;
        }
        self.0.push_back(key);
        if self.0.len() > 100 {
            self.0.pop_front();
        }
        true
    }
}

/// The thread of messages with `number`: a thread takes no `+`.
fn thread(number: &str) -> String {
    format!("signal:{}", number.trim_start_matches('+'))
}

/// The thread of group `id` (base64, `+`, `/` and `=` in it): the id in
/// base64url without padding, after `g-`, which keeps it apart from a
/// number. None for an id that isn't base64, or too long for a thread.
fn group_thread(id: &str) -> Option<String> {
    let base64 = !id.is_empty()
        && id.len() <= 160
        && id
            .trim_end_matches('=')
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/');
    let url: String = id
        .trim_end_matches('=')
        .chars()
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            c => c,
        })
        .collect();
    let thread = format!("signal:g-{url}");
    (base64 && thread.len() <= 128).then_some(thread)
}

impl Message {
    /// Where its answer goes: the group, or the sender.
    fn to(&self) -> Value {
        match &self.group {
            Some(group) => json!({"groupId": group}),
            None => json!({"recipient": [self.from]}),
        }
    }

    /// Its thread: the group's, or the sender's.
    /// Its text, if it has any.
    fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    fn thread(&self) -> String {
        match &self.group {
            Some(group) => group_thread(group).unwrap_or_default(),
            None => thread(&self.from),
        }
    }

    async fn answer(&self, rpc: &Rpc, bridge: &Bridge) {
        let to = self.to();
        // Where an answer nobody waits for may go too.
        if self.group.is_some()
            && let Some(chat) = self.thread().strip_prefix("signal:")
            && let Err(err) = connection::remember_group(&bridge.state, chat)
        {
            link_connection_contract::warn(&format!("Signal: keeping the group: {err:#}"));
        }
        let state = &bridge.state;
        let held =
            connection::holding(bridge, self.text.as_deref(), &self.files, |id| async move {
                attachment(state, &id).await
            });
        let (text, parts) = match held.await {
            connection::Holding::Ask { text, parts } => (text, parts),
            connection::Holding::Answer(answer) => {
                rpc.say(&to, &answer).await;
                return;
            }
        };
        let (progress, _) = tokio::sync::watch::channel(String::new());
        let thread = self.thread();
        // It waits for the harness to answer, which signal-cli, handing
        // over what came while it was off as soon as it starts, is often
        // ahead of.
        let asking = connection::ask_holding(bridge, &thread, &text, &parts, None, &progress);
        tokio::pin!(asking);
        let mut tick = tokio::time::interval(TYPING);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let result = loop {
            tokio::select! {
                result = &mut asking => break result,
                _ = tick.tick() => rpc.typing(&to, false).await,
            }
        };
        rpc.typing(&to, true).await;
        // Joined a turn already running: its answer answers this too.
        let result = match result {
            // Nothing to mark it with on this connection.
            Ok(connection::Asked::Steered) => return,
            Ok(connection::Asked::Answer(answer)) => Ok(answer),
            Err(err) => Err(err),
        };
        match result {
            Ok(answer) if answer.trim().is_empty() => {
                rpc.say(&to, "(Your agent answered nothing.)").await;
            }
            Ok(answer) => {
                rpc.say(&to, &answer).await;
            }
            Err(err) => {
                rpc.say(
                    &to,
                    &link_connection_contract::couldnt_answer(&bridge.agent, &err),
                )
                .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner's direct chat, as `owner` says it for Link to ask on.
    #[test]
    fn the_owners_chat_is_said() {
        let bridge = Bridge {
            settings:
                serde_json::json!({"account": "+15554445555", "allowed_users": ["+15556667777"]})
                    .into(),
            ..Bridge::default()
        };
        assert_eq!(owner(&bridge).unwrap(), ["15556667777"]);
    }

    const AGENT: &str = "+15557654321";
    const OWNER: &str = "+15551234567";

    fn settings() -> Signal {
        Signal {
            account: AGENT.into(),
            allowed_users: vec![OWNER.into()],
            program: None,
        }
    }

    fn envelope(from: &str, data: Value) -> Value {
        json!({"source": from, "sourceNumber": from, "sourceUuid": "a-b", "sourceDevice": 1,
            "timestamp": 1700000000123u64, "dataMessage": data})
    }

    #[test]
    fn only_the_owners_direct_text() {
        let hello = json!({"timestamp": 1700000000123u64, "message": "Hello"});
        assert_eq!(
            owners(&envelope(OWNER, hello.clone()), &settings()),
            Some(Message {
                from: OWNER.into(),
                timestamp: 1700000000123,
                text: Some("Hello".into()),
                files: vec![],
                group: None,
            })
        );
        // A stranger, and the agent's own number.
        assert!(owners(&envelope("+15550000000", hello.clone()), &settings()).is_none());
        assert!(owners(&envelope(AGENT, hello.clone()), &settings()).is_none());
        // In a group.
        let mut group = hello.clone();
        group["groupInfo"] = json!({"groupId": "abc=", "type": "DELIVER"});
        assert!(owners(&envelope(OWNER, group), &settings()).is_none());
        // What the agent sent from another of its devices.
        let sync = json!({"sourceNumber": AGENT, "timestamp": 1,
            "syncMessage": {"sentMessage": {"destinationNumber": OWNER, "message": "Hi"}}});
        assert!(owners(&sync, &settings()).is_none());
        // A receipt, "typing", a reaction: no message.
        let receipt = json!({"sourceNumber": OWNER, "timestamp": 1,
            "receiptMessage": {"isRead": true}});
        assert!(owners(&receipt, &settings()).is_none());
        let reaction = json!({"timestamp": 1, "reaction": {"emoji": "👍"}});
        assert!(owners(&envelope(OWNER, reaction), &settings()).is_none());
        // A picture: carried by its id, without text; an id naming
        // another file is left out.
        let picture = json!({"timestamp": 1, "message": null,
            "attachments": [{"contentType": "image/jpeg", "id": "a1b2.jpg", "size": 9},
                {"contentType": "image/jpeg", "id": "../accounts.json"}]});
        let Some(picture) = owners(&envelope(OWNER, picture), &settings()) else {
            panic!("a picture wasn't carried");
        };
        assert_eq!(picture.text, None);
        assert_eq!(
            picture.files,
            vec![connection::Held {
                media: "image/jpeg".into(),
                name: None,
                size: Some(9),
                at: "a1b2.jpg".into(),
            }]
        );
        // A number hidden (only a uuid) is no one allowed.
        let mut hidden = envelope(OWNER, hello);
        hidden.as_object_mut().unwrap().remove("sourceNumber");
        assert!(owners(&hidden, &settings()).is_none());
        assert!(owners(&json!({}), &settings()).is_none());
    }

    #[test]
    fn the_owners_mention_in_a_group_is_carried_without_it() {
        let mention = |from: &str, mentioned: &str| {
            let data = json!({"timestamp": 1700000000123u64, "message": "\u{fffc} hi",
                "groupInfo": {"groupId": "Z3JvdXA=", "groupName": "Friends", "revision": 2,
                    "type": "DELIVER"},
                "mentions": [{"name": mentioned, "number": mentioned, "uuid": "c-d",
                    "start": 0, "length": 1}]});
            envelope(from, data)
        };
        assert_eq!(
            owners(&mention(OWNER, AGENT), &settings()),
            Some(Message {
                from: OWNER.into(),
                timestamp: 1700000000123,
                text: Some("hi".into()),
                files: vec![],
                group: Some("Z3JvdXA=".into()),
            })
        );
        // Nothing left once the mention is out: skipped.
        let mut alone = mention(OWNER, AGENT);
        alone["dataMessage"]["message"] = json!("\u{fffc} ");
        assert!(owners(&alone, &settings()).is_none());
        // Someone else mentioned, a stranger's mention, the agent's own.
        assert!(owners(&mention(OWNER, "+15550000000"), &settings()).is_none());
        assert!(owners(&mention("+15550000000", AGENT), &settings()).is_none());
        assert!(owners(&mention(AGENT, AGENT), &settings()).is_none());
        // A group with no id to answer in.
        let mut nameless = mention(OWNER, AGENT);
        nameless["dataMessage"]["groupInfo"]["groupId"] = json!("");
        assert!(owners(&nameless, &settings()).is_none());
    }

    #[test]
    fn only_the_agents_own_mentions_are_taken_out() {
        let at = |start: u64, length: u64| json!({"start": start, "length": length});
        let (first, last) = (at(0, 1), at(10, 1));
        // Another's placeholder stays; an emoji is two units.
        let text = "\u{fffc} ask \u{fffc} 😀\u{fffc}";
        assert_eq!(without_mentions(text, &[&first, &last]), "ask \u{fffc} 😀");
        // A span past the text: left as it is.
        let past = at(40, 1);
        assert_eq!(without_mentions(" \u{fffc} hi", &[&past]), "\u{fffc} hi");
    }

    #[test]
    fn a_groups_thread_is_its_id_in_base64url() {
        assert_eq!(
            group_thread("AbC+d/e=").as_deref(),
            Some("signal:g-AbC-d_e")
        );
        // A group's id is 32 bytes: 44 characters in base64.
        let id = format!("{}=", "a+/".repeat(14) + "b");
        let thread = group_thread(&id).unwrap();
        assert_eq!(thread.len(), "signal:g-".len() + 43);
        assert!(!thread.contains(['+', '/', '=']));
        assert_eq!(group_thread(""), None);
        assert_eq!(group_thread("../x"), None);
        assert_eq!(group_thread(&"a".repeat(120)), None);
    }

    /// `send` sends only to an allowed number's chat, or a group an
    /// allowed number talked to the agent in, as signal-cli names each.
    #[test]
    fn send_goes_only_to_the_owner_or_their_groups() {
        let state = link_test_kit::TempDir::new("signal-send");
        let bridge = Bridge {
            state: state.to_path_buf(),
            ..Default::default()
        };
        let group = group_thread("AbC+d/e=").unwrap();
        let chat = group.strip_prefix("signal:").unwrap();
        assert!(may_send(&settings(), &bridge, "15551234567"));
        assert!(!may_send(&settings(), &bridge, "15550000000"));
        assert!(!may_send(&settings(), &bridge, chat));
        connection::remember_group(&state, chat).unwrap();
        assert!(may_send(&settings(), &bridge, chat));
        assert_eq!(recipient(chat), Some(json!({"groupId": "AbC+d/e="})));
        assert_eq!(
            recipient("15551234567"),
            Some(json!({"recipient": [OWNER]}))
        );
        for bad in ["", "g-", "g-../x", "x"] {
            assert_eq!(recipient(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_message_is_carried_once() {
        let mut seen = Seen::default();
        assert!(seen.first((OWNER, 1)));
        assert!(!seen.first((OWNER, 1)));
        assert!(seen.first((OWNER, 2)));
        assert!(seen.first(("+15550000000", 1)));
    }

    #[test]
    fn a_thread_is_the_numbers_digits() {
        assert_eq!(thread(OWNER), "signal:15551234567");
    }
}

//! `lnk-slack bridge`: carries the owner's messages to a harness that
//! speaks no Slack, and its answers back (`link_connection_contract`). It
//! listens in Socket Mode with the app-level token, outside the harness's
//! sandbox, so there's no public address and the harness never holds a
//! token. Each direct message from an allowed member goes to the endpoint
//! alone, its thread `slack:<channel id>`; the answer is posted as it's
//! written, edited in place about once a second, in Slack's mrkdwn
//! ([`crate::format`]), and split into more messages past its limit. An
//! allowed member's mention of the app in a channel goes too, without the
//! mention; the answer is shown the same way, in a Slack thread on the
//! mention. Each Slack thread in a channel
//! is a conversation of its own, `slack:<the channel's id>.<the thread's
//! ts>`: the mention's own, or the one it was made in.

use std::{collections::VecDeque, time::Duration};

use anyhow::Context;
use link_connection_contract::{self as connection, Bridge};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::{call, call_waiting, format};
mod sending;
pub use sending::send;

/// How often an answer being written may be shown anew, at most:
/// Slack's documented limit on `chat.update`, a Tier 3 method, fifty a
/// minute.
const EVERY: Duration = Duration::from_millis(1200);

/// Takes the share code once, from a paired user's mention in a channel.
pub async fn share(sharing: connection::Sharing) -> anyhow::Result<connection::Shared> {
    let settings: link_harness_contract::harness::Slack = sharing.settings.expose().read()?;
    let bot = Bot::new(settings.bot_token.duplicate());
    let (queue, mut messages) = mpsc::unbounded_channel::<Message>();
    let accepting = async {
        while let Some(message) = messages.recv().await {
            if !message.group
                || !connection::has_code(message.text.as_deref().unwrap_or_default(), &sharing.code)
            {
                continue;
            }
            let info = call(
                settings.bot_token.expose(),
                "conversations.info",
                &[("channel", &message.connection)],
            )
            .await?;
            let name = info["channel"]["name"]
                .as_str()
                .map(link_format::clean)
                .filter(|name| !name.trim().is_empty())
                .map(|name| format!("#{name}"));
            let said = format!(
                "Shared with {}: whoever is here can talk to me.",
                name.as_deref().unwrap_or("this channel")
            );
            message.say(&bot, &said).await?;
            return Ok(connection::Shared {
                chat: link_agent_identity::access::chat("slack", &message.connection)?,
                name,
            });
        }
        anyhow::bail!("Slack stopped listening for the share code")
    };
    tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(sharing.wait_secs), accepting) =>
            result.context("the code expired; run lnk agent share add slack again")?,
        () = listen(&settings, &[], queue) => anyhow::bail!("Slack stopped listening"),
    }
}

/// The owner's direct conversation with the bot, `dm_channel`, if
/// connect saved one (`lnk-slack owner`).
pub fn owner(bridge: &Bridge) -> anyhow::Result<Vec<String>> {
    let settings: link_harness_contract::harness::Slack = bridge
        .settings
        .expose()
        .read()
        .context("reading the Slack settings")?;
    Ok(settings.dm_channel.into_iter().collect())
}

/// Carries messages until it fails for good; a failure to reach Slack
/// is tried again.
pub async fn carry(bridge: Bridge) -> anyhow::Result<()> {
    let settings: link_harness_contract::harness::Slack = bridge
        .settings
        .expose()
        .read()
        .context("reading the Slack settings on stdin")?;
    let bot = Bot::new(settings.bot_token.duplicate());
    link_connection_contract::note(&format!("Carrying {}'s Slack messages", bridge.agent));
    // The socket is read (and each message acknowledged) while answers
    // are written: one at a time and in order in each thread, threads
    // side by side.
    let (queue, messages) = mpsc::unbounded_channel::<Message>();
    let answering = connection::per_thread_interacting(
        messages,
        |message| message.conversation(),
        |message| {
            connection::questions::urgent(
                message.text.as_deref(),
                !message.group && message.thread.is_some(),
            )
        },
        |message| {
            let (bot, bridge) = (&bot, &bridge);
            async move {
                if let Err(err) = message.answer(bot, bridge).await {
                    link_connection_contract::warn(&format!("Slack: {err:#}"));
                }
            }
        },
    );
    tokio::select! {
        () = listen(&settings, &bridge.shared, queue) => {}
        () = answering => {}
    }
    Ok(())
}

/// Answers one event pushed by the Events API ([`crate::pushed::take`])
/// as `carry` answers each message it hears, once the answer is sent, or
/// as its answer failed ([`Message::answer`]).
pub(crate) async fn answer_event(bridge: &Bridge, event: &Value) -> anyhow::Result<()> {
    let settings: link_harness_contract::harness::Slack = bridge
        .settings
        .expose()
        .read()
        .context("reading the Slack settings on stdin")?;
    // The app's member id, which its mentions name.
    let me = crate::bot(settings.bot_token.expose()).await?;
    let message = carried(
        event,
        &settings.allowed_users,
        &bridge.shared,
        &me.id,
        &me.team_id,
    )
    .context("the event isn't a message from someone allowed: not answering it")?;
    message
        .answer(&Bot::new(settings.bot_token.duplicate()), bridge)
        .await
}

/// Listens in Socket Mode, connecting again whenever Slack asks or the
/// connection drops.
#[expect(
    clippy::disallowed_methods,
    reason = "backs off before reconnecting to Slack"
)]
async fn listen(
    settings: &link_harness_contract::harness::Slack,
    shared: &[String],
    queue: mpsc::UnboundedSender<Message>,
) {
    let mut seen = Seen::default();
    let mut wait = Duration::from_secs(1);
    loop {
        let started = std::time::Instant::now();
        if let Err(err) = session(settings, shared, &queue, &mut seen).await {
            link_connection_contract::warn(&format!("Slack: {err:#}; connecting again"));
        }
        // Slack asks every few hours: then at once.
        if started.elapsed() > Duration::from_secs(60) {
            wait = Duration::from_secs(1);
            continue;
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(60));
    }
}

/// One Socket Mode connection, until Slack ends it.
async fn session(
    settings: &link_harness_contract::harness::Slack,
    shared: &[String],
    queue: &mpsc::UnboundedSender<Message>,
    seen: &mut Seen,
) -> anyhow::Result<()> {
    // The app's member id, which its mentions name.
    let me = crate::bot(settings.bot_token.expose()).await?;
    let mut socket = crate::socket::open(settings.app_token.expose()).await?;
    while let Some(event) = crate::socket::event(&mut socket).await? {
        let crate::socket::Event::Message(payload) = event else {
            continue;
        };
        if let Some(message) = carried(
            &payload["event"],
            &settings.allowed_users,
            shared,
            &me.id,
            &me.team_id,
        ) && seen.first((&message.connection, &message.ts))
        {
            let _ = queue.send(message);
        }
    }
    Ok(())
}

/// A message from the owner, for the agent.
#[derive(Debug, PartialEq)]
pub(crate) struct Message {
    /// A channel mention, distinct from a thread in an owner's DM.
    group: bool,
    /// The direct message's channel id (`D0...`), or the channel's it
    /// mentions the app in (`C0...`).
    pub(crate) connection: String,
    /// Its timestamp, which is its id in the channel.
    ts: String,
    text: Option<String>,
    /// Its files, by their private URLs on Slack.
    files: Vec<connection::Held<String>>,
    /// For a mention, the Slack thread the answer goes in: the mention's
    /// own, or the thread it's in.
    pub(crate) thread: Option<String>,
    pub(crate) from: Option<link_agent_identity::access::ChatMember>,
}

/// What `event` is to the bridge: a new direct message from someone
/// allowed, or their mention of the app `me` (its member id) in a channel
/// (`app_mention`), without the mention. Anything else (a bot's, an edit,
/// a message in a channel, a mention with no text left) is left
/// unanswered. Pushed messages are sifted by it too
/// ([`crate::pushed::sift`]).
pub(crate) fn carried(
    event: &Value,
    allowed: &[String],
    shared: &[String],
    me: &str,
    team: &str,
) -> Option<Message> {
    let user = event["user"].as_str()?;
    let connection = event["channel"].as_str()?;
    let new = event["bot_id"].is_null()
        && (event["subtype"].is_null() || event["subtype"] == "file_share");
    let id = |id: &str| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric());
    if !new || !id(connection) || !id(user) || user == me {
        return None;
    }
    let owner = allowed.iter().any(|a| a == user);
    let workspace = event["user_team"]
        .as_str()
        .or_else(|| event["team"].as_str());
    if [
        event["user_team"].as_str(),
        event["team"].as_str(),
        event["source_team"].as_str(),
    ]
    .into_iter()
    .flatten()
    .any(|workspace| !team.is_empty() && workspace != team)
    {
        return None;
    }
    let from = if owner {
        None
    } else if event["type"] == "app_mention"
        && shared.iter().any(|c| c == connection)
        && id(team)
        && workspace == Some(team)
    {
        Some(link_agent_identity::access::chat_member("slack", &format!("{team}/{user}")).ok()?)
    } else {
        return None;
    };
    let ts = event["ts"].as_str().unwrap_or_default().to_string();
    match event["type"].as_str() {
        Some("message") if event["channel_type"] == "im" => {
            let thread = event["thread_ts"].as_str();
            if thread.is_some_and(|thread| !stamp(thread)) {
                return None;
            }
            Some(Message {
                group: false,
                connection: connection.to_string(),
                ts,
                text: event["text"]
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
                    .map(unescape),
                files: files(event),
                thread: thread.map(str::to_string),
                from: None,
            })
        }
        Some("app_mention") if stamp(&ts) => {
            let text = without_mention(event["text"].as_str().unwrap_or_default(), me);
            let files = files(event);
            let thread = event["thread_ts"].as_str().unwrap_or(&ts);
            if (text.is_empty() && files.is_empty()) || !stamp(thread) {
                return None;
            }
            Some(Message {
                group: true,
                connection: connection.to_string(),
                thread: Some(thread.to_string()),
                ts,
                text: Some(unescape(&text)),
                files,
                from,
            })
        }
        _ => None,
    }
}

/// The files `event` holds, each at its private URL on Slack, which the
/// bot's token downloads; one anywhere else is left out.
fn files(event: &Value) -> Vec<connection::Held<String>> {
    event["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| {
            let url = file["url_private_download"]
                .as_str()
                .or(file["url_private"].as_str())
                .filter(|url| crate::on_slack(url))?;
            Some(connection::Held {
                media: file["mimetype"].as_str().unwrap_or_default().to_string(),
                name: file["name"].as_str().map(str::to_string),
                size: file["size"].as_u64(),
                at: url.to_string(),
            })
        })
        .collect()
}

/// `text` without its mentions of the app `me` (`<@U0BOT>`, or
/// `<@U0BOT|name>`), trimmed.
fn without_mention(text: &str, me: &str) -> String {
    if me.is_empty() {
        return text.trim().to_string();
    }
    let mut kept = String::new();
    let mut rest = text;
    let mention = format!("<@{me}");
    while let Some(start) = rest.find(&mention) {
        let after = &rest[start + mention.len()..];
        let end = match after.chars().next() {
            Some('>') => Some(1),
            Some('|') => after.find('>').map(|i| i + 1),
            _ => None,
        };
        match end {
            Some(end) => {
                kept.push_str(&rest[..start]);
                rest = &after[end..];
            }
            None => {
                kept.push_str(&rest[..start + mention.len()]);
                rest = after;
            }
        }
    }
    kept.push_str(rest);
    kept.trim().to_string()
}

/// The thread of messages in `connection`: a direct message's, or a
/// channel's; in a channel, of its Slack thread `in_thread` (its first
/// message's `ts`).
pub(crate) fn thread(connection: &str, in_thread: Option<&str>) -> String {
    format!("slack:{}", at(connection, in_thread))
}

/// Channel `connection`, or its Slack thread `in_thread`, as `send` takes
/// it: `<channel>.<ts>`.
pub(crate) fn at(connection: &str, in_thread: Option<&str>) -> String {
    match in_thread {
        Some(ts) => format!("{connection}.{ts}"),
        None => connection.to_string(),
    }
}

/// The channel and Slack thread `chat` names, as [`at`] writes them; none
/// for what isn't a channel's id, or a thread's `ts` after it.
pub(crate) fn place(chat: &str) -> Option<(&str, Option<&str>)> {
    let id = |id: &str| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric());
    let (channel, ts) = match chat.split_once('.') {
        Some((channel, ts)) => (channel, Some(ts)),
        None => (chat, None),
    };
    (id(channel) && ts.is_none_or(stamp)).then_some((channel, ts))
}

/// Whether `ts` is a message's timestamp, as Slack writes one
/// (`1700000000.000200`).
fn stamp(ts: &str) -> bool {
    ts.split_once('.').is_some_and(|(secs, part)| {
        [secs, part]
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// The events already carried, the latest 100: Slack sends one again
/// when it missed the acknowledgement.
#[derive(Default)]
struct Seen(VecDeque<(String, String)>);

impl Seen {
    /// Whether the event named `(channel, ts)` is new, remembering it.
    fn first(&mut self, (connection, ts): (&str, &str)) -> bool {
        let key = (connection.to_string(), ts.to_string());
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

impl Message {
    /// Answers it where it was said, as the agent at `bridge` answers it.
    /// An answer Slack didn't take is an error, one it refused (the
    /// channel gone or archived, the app removed from it)
    /// [`link_connection_contract::ReplyRefused`].
    #[expect(
        clippy::disallowed_methods,
        reason = "paces the answer's updates to Slack's rate"
    )]
    async fn answer(&self, bot: &Bot, bridge: &Bridge) -> anyhow::Result<()> {
        if !self.group
            && self.from.is_none()
            && self.files.is_empty()
            && let (Some(reply), Some(text)) = (&self.thread, &self.text)
            && let Some(answer) = connection::questions::caught_reply(
                &bridge.agent,
                &format!("slack:{}", self.connection),
                reply,
                text,
                &self.ts,
            )
        {
            return self.say(bot, &answer).await;
        }
        // In a channel, as the agent's spec says.
        if self.group && !bridge.answers_groups() {
            return Ok(());
        }
        // Where an answer nobody waits for may go too.
        if self.group
            && self.from.is_none()
            && let Err(err) = connection::remember_group(&bridge.state, &self.connection)
        {
            link_connection_contract::warn(&format!("Slack: keeping the channel: {err:#}"));
        }
        let held = connection::holding(
            bridge,
            self.text.as_deref(),
            &self.files,
            |url| async move { crate::download(&bot.token, &url).await },
        );
        let (text, parts) = match held.await {
            connection::Holding::Ask { text, parts } => (text, parts),
            connection::Holding::Answer(answer) => {
                return self.say(bot, &answer).await;
            }
        };
        // A bot shows no "typing" in Slack: a reaction on the message
        // instead, while it's answered, as the agent's spec says.
        let working = bridge.spec.as_ref().and_then(|s| s.working.clone());
        if let Some(working) = &working {
            bot.react("reactions.add", self, working).await;
        }
        let (progress, mut seen) = tokio::sync::watch::channel(String::new());
        let thread = self.conversation();
        let name = match &self.from {
            Some(person) => Some(
                bot.name(person.as_str().rsplit('/').next().unwrap_or_default())
                    .await,
            ),
            None => None,
        };
        let from = self.from.as_ref().map(|person| connection::Speaker {
            from: person.as_str(),
            name: name.as_deref(),
            hops: 0,
        });
        let asking = connection::ask_holding(bridge, &thread, &text, &parts, from, &progress);
        tokio::pin!(asking);
        let mut shown = Shown::default();
        let every = connection::pace(bridge, EVERY);
        let mut tick = tokio::time::interval(every.unwrap_or(EVERY));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let result = loop {
            tokio::select! {
                result = &mut asking => break result,
                _ = tick.tick() => {
                    // Told to wait: no edit until then, and the next
                    // one shows what's new by then.
                    let so_far = seen.borrow_and_update().clone();
                    if !so_far.is_empty() && every.is_some() && !bot.held() {
                        // On the way: what wasn't posted goes with the whole.
                        let _ = shown.show(bot, self, &so_far, false).await;
                    }
                }
            }
        };
        // Joined a turn already running: its answer answers this too.
        let result = match result {
            Ok(connection::Asked::Steered) => {
                if let Some(steered) = bridge.spec.as_ref().and_then(|s| s.steered.clone()) {
                    bot.react("reactions.add", self, &steered).await;
                }
                if let Some(working) = &working {
                    bot.react("reactions.remove", self, working).await;
                }
                return Ok(());
            }
            Ok(connection::Asked::Answer(answer)) => Ok(answer),
            Err(err) => Err(err),
        };
        let answered = match result {
            Ok(answer) if answer.trim().is_empty() => {
                self.say(bot, "(Your agent answered nothing.)").await
            }
            Ok(answer) => {
                // The whole answer, even past a rate limit; never again
                // once Slack refused it.
                let mut shown_all = Ok(false);
                for _ in 0..3 {
                    shown_all = shown.show(bot, self, &answer, true).await;
                    match &shown_all {
                        Ok(true) => break,
                        Err(err) if connection::reply_refused(err) => break,
                        _ => tokio::time::sleep(EVERY).await,
                    }
                }
                shown_all.map(drop)
            }
            Err(err) => {
                self.say(
                    bot,
                    &link_connection_contract::couldnt_answer(&bridge.agent, &err),
                )
                .await
            }
        };
        if let Some(working) = &working {
            bot.react("reactions.remove", self, working).await;
        }
        answered
    }

    /// Its conversation: a direct message's, or in a channel, its Slack
    /// thread's.
    pub(crate) fn conversation(&self) -> String {
        thread(&self.connection, self.thread.as_deref())
    }

    /// Says `text`, Markdown, where its answer goes, all at once: each
    /// piece posted.
    async fn say(&self, bot: &Bot, text: &str) -> anyhow::Result<()> {
        for piece in format::pieces(text) {
            self.post(bot, &piece).await?;
        }
        Ok(())
    }

    /// Posts `text`, mrkdwn, where its answer goes: its channel, in the
    /// Slack thread for a mention; its message's `ts`. Slack refusing it
    /// is the connection refusing the answer
    /// ([`link_connection_contract::ReplyRefused`]).
    async fn post(&self, bot: &Bot, text: &str) -> anyhow::Result<Option<String>> {
        match bot
            .post(&self.connection, self.thread.as_deref(), text)
            .await
        {
            Err(err) if err.is::<crate::Refused>() => {
                Err(anyhow::Error::from(connection::ReplyRefused).context(err.to_string()))
            }
            posted => posted,
        }
    }
}

/// An answer's messages as they're shown: each one's `ts` and text.
#[derive(Default)]
struct Shown {
    messages: Vec<(String, String)>,
}

impl Shown {
    /// Shows `answer` to `message`: each piece posted, or edited where it
    /// changed; whether all of it is shown, or why a piece couldn't be
    /// posted. Only for the `whole` answer does an edit wait when told to.
    async fn show(
        &mut self,
        bot: &Bot,
        message: &Message,
        answer: &str,
        whole: bool,
    ) -> anyhow::Result<bool> {
        let mut all = true;
        for (i, piece) in format::pieces(answer).into_iter().enumerate() {
            match self.messages.get_mut(i) {
                Some((_, text)) if *text == piece => {}
                Some((ts, text)) => {
                    if bot.update(&message.connection, ts, &piece, whole).await {
                        *text = piece;
                    } else {
                        all = false;
                    }
                }
                None => match message.post(bot, &piece).await? {
                    Some(ts) => self.messages.push((ts, piece)),
                    None => return Ok(false),
                },
            }
        }
        Ok(all)
    }
}

/// A message's text as the person typed it.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

struct Bot {
    token: link_secretive::Secret,
    /// Until when Slack said to wait, told so by an edit that didn't.
    held: std::sync::Mutex<Option<std::time::Instant>>,
    names: tokio::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl Bot {
    fn new(token: impl Into<link_secretive::Secret>) -> Self {
        Self {
            token: token.into(),
            held: std::sync::Mutex::new(None),
            names: Default::default(),
        }
    }

    async fn name(&self, user: &str) -> String {
        let mut names = self.names.lock().await;
        if let Some(name) = names.get(user) {
            return name.clone();
        }
        let name = crate::name_of(self.token.expose(), user).await;
        let name = if name == "you" {
            user.to_string()
        } else {
            name
        };
        if names.len() >= 256 {
            names.clear();
        }
        names.insert(user.to_string(), name.clone());
        name
    }

    /// Whether Slack said to wait, and that time hasn't passed.
    fn held(&self) -> bool {
        let held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        held.is_some_and(|until| std::time::Instant::now() < until)
    }

    /// Posts `text`, mrkdwn ([`crate::format`]), in `thread` if one is
    /// given; its message's `ts`, or its failure: a refused token as
    /// [`link_connection_contract::TokenRefused`].
    async fn post(
        &self,
        connection: &str,
        thread: Option<&str>,
        text: &str,
    ) -> anyhow::Result<Option<String>> {
        let mut form = vec![("channel", connection), ("text", text)];
        if let Some(thread) = thread {
            form.push(("thread_ts", thread));
        }
        let message = call(self.token.expose(), "chat.postMessage", &form).await?;
        Ok(message["ts"].as_str().map(str::to_string))
    }

    /// Edits message `ts` to `text`, mrkdwn; whether it was. Told to
    /// wait, it waits only for the `whole` answer: an edit on the way is
    /// out of date by then.
    async fn update(&self, connection: &str, ts: &str, text: &str, whole: bool) -> bool {
        let updated = call_waiting(
            self.token.expose(),
            "chat.update",
            &[("channel", connection), ("ts", ts), ("text", text)],
            (!whole).then_some(&self.held),
        )
        .await;
        if let Err(err) = &updated {
            link_connection_contract::warn(&format!("chat.update: {err:#}"));
        }
        updated.is_ok()
    }

    /// Adds or removes (`method`) 👀 on `message`.
    async fn react(&self, method: &str, message: &Message, name: &str) {
        let _ = call(
            self.token.expose(),
            method,
            &[
                ("channel", &message.connection),
                ("timestamp", &message.ts),
                ("name", name),
            ],
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The owner's direct chat, as `owner` says it for Link to ask on.
    #[test]
    fn the_owners_chat_is_said() {
        let bridge = Bridge {
            settings: serde_json::json!({"bot_token": "xoxb", "app_token": "xapp", "allowed_users": ["U1"], "dm_channel": "D1"}).into(),
            ..Bridge::default()
        };
        assert_eq!(owner(&bridge).unwrap(), ["D1"]);
    }

    /// A message nobody asked for goes to the owner's direct conversation
    /// with the bot alone: another member's, a channel, or any with an app
    /// connected before it was kept, is refused before Slack is asked.
    #[tokio::test]
    async fn send_refuses_a_chat_not_the_owners() {
        let bridge = |dm: Option<&str>| Bridge {
            settings: json!({"bot_token": "xoxb-x", "allowed_users": ["U0DANA"], "dm_channel": dm})
                .into(),
            ..Bridge::default()
        };
        for (dm, chat) in [
            (Some("D0DANA"), "D0OTHER"),
            (Some("D0DANA"), "C0GENERAL"),
            (None, "D0DANA"),
        ] {
            let err = send(bridge(dm), chat.into(), "hi".into()).await;
            let err = format!("{:#}", err.unwrap_err());
            assert!(err.contains("not sending there"), "{chat}: {err}");
        }
    }

    const BOT: &str = "U0BOT";

    #[test]
    fn shared_mentions_keep_the_sender_and_refuse_outsiders() {
        let owners = ["U0DANA".into()];
        let shared = ["C0TEAM".into()];
        let event = json!({"type": "app_mention", "user": "U0ALICE", "channel": "C0TEAM",
            "user_team": "T0ACME", "ts": "1.2", "text": "<@U0BOT> hello"});
        let read =
            |event: &Value, shared: &[String]| carried(event, &owners, shared, BOT, "T0ACME");
        let member = read(&event, &shared).unwrap();
        assert_eq!(member.from.unwrap().as_str(), "member:slack:T0ACME/U0ALICE");
        assert_eq!(member.text.as_deref(), Some("hello"));
        assert!(read(&event, &[]).is_none());
        for (field, value) in [
            ("user_team", json!("T0OTHER")),
            ("user_team", Value::Null),
            ("bot_id", json!("B01")),
            ("subtype", json!("message_changed")),
            ("channel", json!("C0OTHER")),
            ("user", json!("../bad")),
            ("type", json!("message")),
        ] {
            let mut refused = event.clone();
            refused[field] = value;
            assert!(read(&refused, &shared).is_none(), "{field}: {refused}");
        }
        let mut direct = event;
        direct["type"] = json!("message");
        direct["channel_type"] = json!("im");
        assert!(read(&direct, &shared).is_none());
    }

    #[test]
    fn only_the_owners_direct_messages() {
        let allowed = ["U0DANA".to_string()];
        let event = |user: &str, channel_type: &str| {
            json!({"type": "message", "user": user, "channel": "D0DANA",
                "channel_type": channel_type, "ts": "1.2", "text": "a &lt;b&gt; &amp;"})
        };
        let owners = |event: &Value| carried(event, &allowed, &[], BOT, "T0ACME");
        assert_eq!(
            owners(&event("U0DANA", "im")),
            Some(Message {
                group: false,
                connection: "D0DANA".into(),
                ts: "1.2".into(),
                text: Some("a <b> &".into()),
                files: vec![],
                thread: None,
                from: None,
            })
        );
        assert!(owners(&event("U0EVE", "im")).is_none());
        assert!(owners(&event("U0DANA", "channel")).is_none());
        assert!(owners(&event("U0DANA", "mpim")).is_none());
        let mut threaded = event("U0DANA", "im");
        threaded["thread_ts"] = json!("1.1");
        let first = owners(&threaded).unwrap();
        assert_eq!(first.conversation(), "slack:D0DANA.1.1");
        threaded["ts"] = json!("1.3");
        assert_eq!(
            owners(&threaded).unwrap().conversation(),
            first.conversation()
        );
        threaded["thread_ts"] = json!("2.1");
        assert_ne!(
            owners(&threaded).unwrap().conversation(),
            first.conversation()
        );
        threaded["thread_ts"] = json!("../1");
        assert!(owners(&threaded).is_none());
        let mut bot = event("U0DANA", "im");
        bot["bot_id"] = json!("B01");
        assert!(owners(&bot).is_none());
        let mut edit = event("U0DANA", "im");
        edit["subtype"] = json!("message_changed");
        assert!(owners(&edit).is_none());
        let mut odd = event("U0DANA", "im");
        odd["channel"] = json!("../D0");
        assert!(owners(&odd).is_none());
        let mut file = event("U0DANA", "im");
        file["subtype"] = json!("file_share");
        file["text"] = json!("");
        file["files"] = json!([
            {"name": "a.pdf", "mimetype": "application/pdf", "size": 9,
                "url_private_download": "https://files.slack.com/files-pri/T0-F0/download/a.pdf"},
            {"name": "b.pdf", "url_private_download": "https://elsewhere.example/b.pdf"}]);
        let file = owners(&file).unwrap();
        assert_eq!(file.text, None);
        // Only from Slack, where its token goes.
        assert_eq!(
            file.files,
            vec![connection::Held {
                media: "application/pdf".into(),
                name: Some("a.pdf".into()),
                size: Some(9),
                at: "https://files.slack.com/files-pri/T0-F0/download/a.pdf".into(),
            }]
        );
        assert!(owners(&json!({})).is_none());
    }

    #[test]
    fn the_owners_mention_in_a_channel_is_carried_without_it() {
        let allowed = ["U0DANA".to_string()];
        let mention = |user: &str, text: &str| {
            json!({"type": "app_mention", "user": user, "channel": "C0123ABC", "ts": "1.3",
                "text": text})
        };
        let owners = |event: &Value| carried(event, &allowed, &[], BOT, "T0ACME");
        let carried = |text: &str, thread: &str| {
            Some(Message {
                group: true,
                connection: "C0123ABC".into(),
                ts: "1.3".into(),
                text: Some(text.into()),
                files: vec![],
                thread: Some(thread.into()),
                from: None,
            })
        };
        let hi = "<@U0BOT> hi &lt;there&gt;";
        assert_eq!(owners(&mention("U0DANA", hi)), carried("hi <there>", "1.3"));
        // In a thread: answered in it.
        let mut threaded = mention("U0DANA", hi);
        threaded["thread_ts"] = json!("1.1");
        assert_eq!(owners(&threaded), carried("hi <there>", "1.1"));
        // Each Slack thread is a conversation of its own: the mention's
        // own, or the one it's in.
        assert_eq!(
            owners(&mention("U0DANA", hi)).unwrap().conversation(),
            "slack:C0123ABC.1.3"
        );
        assert_eq!(
            owners(&threaded).unwrap().conversation(),
            "slack:C0123ABC.1.1"
        );
        // A thread that isn't a timestamp isn't carried.
        threaded["thread_ts"] = json!("../1");
        assert!(owners(&threaded).is_none());
        // The mention alone leaves nothing to carry.
        assert!(owners(&mention("U0DANA", " <@U0BOT> ")).is_none());
        // A stranger's, a bot's, an odd channel.
        assert!(owners(&mention("U0EVE", hi)).is_none());
        let mut bot = mention("U0DANA", hi);
        bot["bot_id"] = json!("B01");
        assert!(owners(&bot).is_none());
        let mut odd = mention("U0DANA", hi);
        odd["channel"] = json!("../C0");
        assert!(owners(&odd).is_none());
    }

    #[test]
    fn only_the_apps_own_mentions_are_taken_out() {
        assert_eq!(
            without_mention("<@U0BOT> ask <@U0EVE> and <@U0BOT|link>", BOT),
            "ask <@U0EVE> and"
        );
        // Another member whose id starts with the app's.
        assert_eq!(without_mention("<@U0BOTX> hi", BOT), "<@U0BOTX> hi");
        // An unclosed mention stays.
        assert_eq!(without_mention("hi <@U0BOT|link", BOT), "hi <@U0BOT|link");
        // Without the app's id, the text as it is.
        assert_eq!(without_mention(" <@U0BOT> hi", ""), "<@U0BOT> hi");
    }

    #[test]
    fn a_thread_is_the_channels_id() {
        assert_eq!(thread("D0DANA", None), "slack:D0DANA");
        assert_eq!(thread("C0123ABC", None), "slack:C0123ABC");
        // A Slack thread in a channel is a conversation of its own, as
        // send takes it.
        assert_eq!(
            thread("C0123ABC", Some("1700000000.000200")),
            "slack:C0123ABC.1700000000.000200"
        );
        assert_eq!(
            place("C0123ABC.1700000000.000200"),
            Some(("C0123ABC", Some("1700000000.000200")))
        );
        assert_eq!(place("D0DANA"), Some(("D0DANA", None)));
        for bad in [
            "",
            "C0-1",
            "C01.",
            "C01.17",
            "C01.17.x",
            ".17.1",
            "C01.17.1.2",
        ] {
            assert_eq!(place(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_message_sent_again_is_carried_once() {
        let mut seen = Seen::default();
        assert!(seen.first(("D0DANA", "1.1")));
        assert!(!seen.first(("D0DANA", "1.1")));
        assert!(seen.first(("D0DANA", "1.2")));
    }

    #[test]
    fn a_message_reads_as_typed() {
        let escape = crate::format::escape;
        assert_eq!(unescape(&escape("a <b> & &amp;")), "a <b> & &amp;");
    }
}

//! `lnk-telegram bridge`: carries the owner's messages to a harness that
//! speaks no Telegram, and its answers back (`link_connection_contract`).
//! It long-polls the bot outside the harness's sandbox, so the harness
//! never holds the token. Each direct message from an allowed user goes
//! to the endpoint alone, its thread `telegram:<chat id>`; the answer is
//! sent as it's written, edited in place about once a second, and split
//! into more messages past Telegram's limit. An allowed user's mention of
//! the bot in a group, or reply to it there, goes too, without the
//! mention, its thread `telegram:<the group's chat id>`; the answer is a
//! reply to it, shown the same way. In a forum, each topic is a
//! conversation of its own, `telegram:<the group's chat id>.<the topic's
//! id>`, answered in the topic.

use std::time::{Duration, Instant};

use anyhow::Context;
use link_connection_contract::{self as connection, Bridge};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::api;
use crate::format::{self, Piece};

/// How often an answer being written may be shown anew, at most, in a
/// chat with one person: Telegram's documented limit on a bot's messages
/// in one chat, about one a second (the Bot API's FAQ).
const EVERY: Duration = Duration::from_secs(1);

/// The same in a group (a chat whose id is negative): Telegram's twenty
/// messages a minute.
const EVERY_IN_GROUP: Duration = Duration::from_secs(3);

/// Takes a paired user's mention with the share code once, in a group.
pub async fn share(sharing: connection::Sharing) -> anyhow::Result<connection::Shared> {
    let settings: link_harness_contract::harness::Telegram = sharing.settings.expose().read()?;
    let bot = Bot::new(&settings.bot_token);
    let accepting = async {
        let me = Me::of(&bot.call("getMe", json!({})).await?);
        anyhow::ensure!(
            me.id != 0 && !me.username.is_empty(),
            "Telegram sent no bot identity"
        );
        let mut offset = None;
        loop {
            let updates = bot
                .call(
                    "getUpdates",
                    json!({"offset": offset, "timeout": 25, "allowed_updates": ["message"]}),
                )
                .await?;
            for update in updates.as_array().into_iter().flatten() {
                if let Some(id) = update["update_id"].as_i64() {
                    offset = Some(offset.unwrap_or(0).max(id.saturating_add(1)));
                }
                let Some(message) = shared_code(
                    &update["message"],
                    &settings.allowed_users,
                    &me,
                    &sharing.code,
                ) else {
                    continue;
                };
                let name = update["message"]["chat"]["title"]
                    .as_str()
                    .map(link_format::clean)
                    .filter(|name| !name.trim().is_empty());
                message
                    .say(
                        &bot,
                        &format!(
                            "Shared with {}: whoever is here can talk to me.",
                            name.as_deref().unwrap_or("this group")
                        ),
                    )
                    .await?;
                return Ok(connection::Shared {
                    chat: link_agent_identity::access::chat("telegram", &message.chat.to_string())?,
                    name,
                });
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(sharing.wait_secs), accepting)
        .await
        .context("the code expired; run lnk agent share add telegram again")?
}

fn shared_code(
    message: &Value,
    allowed: &[i64],
    me: &Me,
    code: &link_secretive::Secret,
) -> Option<Message> {
    let carried = carried(message, allowed, &[], me)?;
    let text = message["text"].as_str()?;
    (carried.chat < 0
        && carried.reply_to.is_some()
        && !mentioned(text, &message["entities"], me).is_empty()
        && connection::has_code(text, code))
    .then_some(carried)
}

/// Telegram's limit on how often a bot writes in chat `chat`.
fn floor(chat: i64) -> Duration {
    match chat < 0 {
        true => EVERY_IN_GROUP,
        false => EVERY,
    }
}

/// The owner's direct chats with the bot: each allowed user's own, whose
/// id is theirs (`lnk-telegram owner`).
pub fn owner(bridge: &Bridge) -> anyhow::Result<Vec<String>> {
    let settings: link_harness_contract::harness::Telegram = bridge
        .settings
        .expose()
        .read()
        .context("reading the Telegram settings")?;
    Ok(settings.allowed_users.iter().map(i64::to_string).collect())
}

/// Sends `text` to chat `chat`, a message nobody asked for (an answer
/// to a schedule or a subagent): only to an allowed user's own chat with
/// the bot, or a group an allowed user talked to the agent in while its
/// spec lets it answer in groups; never another group or anyone else.
pub async fn send(
    bridge: Bridge,
    chat: String,
    text: String,
) -> anyhow::Result<connection::SentMessages> {
    let settings: link_harness_contract::harness::Telegram = bridge
        .settings
        .expose()
        .read()
        .context("reading the Telegram settings on stdin")?;
    anyhow::ensure!(
        may_send(&settings, &bridge, &chat),
        "Telegram chat {chat} isn't an allowed user's own, nor a group they talk to the agent \
         in: not sending there"
    );
    let (chat, topic) = place(&chat).context("a Telegram chat is a number")?;
    let bot = Bot::new(&settings.bot_token);
    let mut messages = Vec::new();
    for piece in format::pieces(&text) {
        let message = bot
            .formatted(
                "sendMessage",
                in_topic(json!({"chat_id": chat}), topic),
                &piece,
                true,
            )
            .await?;
        if let Some(id) = message["result"]["message_id"]
            .as_i64()
            .or_else(|| message["message_id"].as_i64())
        {
            messages.push(id.to_string());
        }
    }
    Ok(connection::SentMessages { messages })
}

/// The chat and topic `chat` names: a chat's id, or a forum's and its
/// topic's, `<chat>.<topic>`, as [`thread`] writes them.
pub(crate) fn place(chat: &str) -> Option<(i64, Option<i64>)> {
    match chat.split_once('.') {
        Some((chat, topic)) => {
            let topic = topic.parse().ok().filter(|topic: &i64| *topic > 0)?;
            Some((chat.parse().ok()?, Some(topic)))
        }
        None => Some((chat.parse().ok()?, None)),
    }
}

/// `body`, a call to a chat, sent into topic `topic` of its forum.
pub(crate) fn in_topic(mut body: Value, topic: Option<i64>) -> Value {
    if let Some(topic) = topic {
        body["message_thread_id"] = topic.into();
    }
    body
}

/// Whether `send` may send to `chat`: an allowed user's own chat, or a
/// group (a negative id) they talked to the agent in, any of its topics.
fn may_send(
    settings: &link_harness_contract::harness::Telegram,
    bridge: &Bridge,
    chat: &str,
) -> bool {
    match place(chat) {
        Some((id, _)) if id < 0 => {
            let group = id.to_string();
            bridge.answers_groups()
                && (connection::knows_group(&bridge.state, &group)
                    || bridge.shared.contains(&group))
        }
        Some((id, None)) => settings.allowed_users.contains(&id),
        _ => false,
    }
}

/// Carries messages until it fails for good; a failure to reach
/// Telegram is tried again.
pub async fn carry(bridge: Bridge) -> anyhow::Result<()> {
    let settings: link_harness_contract::harness::Telegram = bridge
        .settings
        .expose()
        .read()
        .context("reading the Telegram settings on stdin")?;
    let bot = Bot::new(&settings.bot_token);
    link_connection_contract::note(&format!("Carrying {}'s Telegram messages", bridge.agent));
    let me = bot.me().await;
    // Telegram is polled while answers are written: one at a time and in
    // order in each chat, chats side by side.
    let (queue, messages) = mpsc::unbounded_channel::<Message>();
    let answering = connection::per_thread_interacting(
        messages,
        |message| thread(message.chat, message.topic),
        |message| {
            connection::questions::urgent(message.text.as_deref(), message.question_reply.is_some())
        },
        |message| {
            let (bot, bridge) = (&bot, &bridge);
            async move {
                if let Err(err) = message.answer(bot, bridge).await {
                    link_connection_contract::warn(&format!("Telegram: {err:#}"));
                }
            }
        },
    );
    tokio::select! {
        () = poll(&bot, &settings, &bridge.shared, &me, queue) => {}
        () = answering => {}
    }
    Ok(())
}

/// Polls the bot for updates, each message from the owner put in
/// `queue`; a failure to reach Telegram is tried again.
#[expect(
    clippy::disallowed_methods,
    reason = "backs off before polling Telegram again"
)]
async fn poll(
    bot: &Bot,
    settings: &link_harness_contract::harness::Telegram,
    shared: &[String],
    me: &Me,
    queue: mpsc::UnboundedSender<Message>,
) {
    let mut offset: Option<i64> = None;
    let mut wait = Duration::from_secs(1);
    loop {
        let mut query = json!({"timeout": 25, "allowed_updates": ["message"]});
        if let Some(offset) = offset {
            query["offset"] = offset.into();
        }
        let updates = match bot.call("getUpdates", query).await {
            Ok(updates) => {
                wait = Duration::from_secs(1);
                updates
            }
            Err(err) => {
                // Pushed to a webhook instead: said so, not Telegram's
                // own words.
                match crate::webhook_set(&format!("{err:#}")) {
                    true => link_connection_contract::warn(&format!(
                        "Telegram: {}; trying again in {}s",
                        crate::HOOKED,
                        wait.as_secs()
                    )),
                    false => link_connection_contract::warn(&format!(
                        "Telegram: {err:#}; trying again in {}s",
                        wait.as_secs()
                    )),
                }
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        for update in updates.as_array().into_iter().flatten() {
            let id = update["update_id"].as_i64().unwrap_or(0);
            offset = Some(offset.unwrap_or(0).max(id + 1));
            if let Some(message) = carried(&update["message"], &settings.allowed_users, shared, me)
            {
                let _ = queue.send(message);
            }
        }
    }
}

/// The bot, as `getMe` says.
#[derive(Default)]
pub(crate) struct Me {
    pub(crate) id: i64,
    pub(crate) username: String,
}

impl Me {
    /// The bot `getMe` answered `me` of.
    pub(crate) fn of(me: &Value) -> Self {
        Self {
            id: me["id"].as_i64().unwrap_or(0),
            username: me["username"].as_str().unwrap_or_default().to_string(),
        }
    }
}

/// A message from the owner, for the agent.
#[derive(Debug, PartialEq)]
pub(crate) struct Message {
    question_reply: Option<String>,
    event_id: String,
    /// The direct chat's id, or the group's.
    pub(crate) chat: i64,
    /// In a forum, the topic's id.
    pub(crate) topic: Option<i64>,
    text: Option<String>,
    /// Its photo, document or voice note, by its file's id.
    files: Vec<connection::Held<String>>,
    /// In a group, the message the answer replies to: the mention.
    reply_to: Option<i64>,
    pub(crate) from: Option<Member>,
}

/// The photo, document or voice note `message` holds: a photo at its
/// largest, by its file's id; a voice note Telegram's Ogg unless it says.
fn files(message: &Value) -> Vec<connection::Held<String>> {
    let size = |file: &Value| file["file_size"].as_u64();
    let photo = message["photo"].as_array().and_then(|sizes| sizes.last());
    let photo = photo.and_then(|photo| {
        Some(connection::Held {
            media: "image/jpeg".into(),
            name: None,
            size: size(photo),
            at: photo["file_id"].as_str()?.to_string(),
        })
    });
    let document = &message["document"];
    let document = document["file_id"].as_str().map(|id| connection::Held {
        media: document["mime_type"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        name: document["file_name"].as_str().map(str::to_string),
        size: size(document),
        at: id.to_string(),
    });
    let voice = ["voice", "audio"].into_iter().find_map(|kind| {
        let voice = &message[kind];
        Some(connection::Held {
            media: voice["mime_type"]
                .as_str()
                .unwrap_or("audio/ogg")
                .to_string(),
            name: voice["file_name"].as_str().map(str::to_string),
            size: size(voice),
            at: voice["file_id"].as_str()?.to_string(),
        })
    });
    photo.into_iter().chain(document).chain(voice).collect()
}

#[derive(Debug, PartialEq)]
pub(crate) struct Member {
    pub(crate) person: link_agent_identity::access::ChatMember,
    name: Option<String>,
}

/// What `message` is to the bridge: a direct message from someone
/// allowed, or their mention of the bot in a group, without the mention.
/// Anything else, and a mention with no text left, is left unanswered.
#[cfg(test)]
fn owners(message: &Value, allowed: &[i64], me: &Me) -> Option<Message> {
    carried(message, allowed, &[], me)
}

pub(crate) fn carried(
    message: &Value,
    allowed: &[i64],
    shared: &[String],
    me: &Me,
) -> Option<Message> {
    let from = message["from"]["id"].as_i64()?;
    let human = message["from"]["is_bot"] != true;
    if !human || !message["sender_chat"].is_null() || from <= 0 {
        return None;
    }
    let owner = allowed.contains(&from);
    let group = matches!(
        message["chat"]["type"].as_str(),
        Some("group" | "supergroup")
    );
    let chat = message["chat"]["id"].as_i64()?;
    let from = if owner {
        None
    } else if group && chat < 0 && shared.contains(&chat.to_string()) {
        let name = [
            message["from"]["first_name"].as_str(),
            message["from"]["last_name"].as_str(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        let name = if name.trim().is_empty() {
            message["from"]["username"].as_str().map(str::to_string)
        } else {
            Some(name)
        };
        Some(Member {
            person: link_agent_identity::access::chat_member("telegram", &from.to_string()).ok()?,
            name,
        })
    } else {
        return None;
    };
    match message["chat"]["type"].as_str() {
        Some("private") => Some(Message {
            question_reply: message["reply_to_message"]["message_id"]
                .as_i64()
                .map(|id| id.to_string()),
            event_id: message["message_id"]
                .as_i64()
                .map(|id| id.to_string())
                .unwrap_or_default(),
            chat,
            topic: None,
            text: message["text"]
                .as_str()
                .or(message["caption"].as_str())
                .map(str::to_string),
            files: files(message),
            reply_to: None,
            from: None,
        }),
        Some("group" | "supergroup") if addressed(message, me) => {
            let (text, entities) = match message["text"].as_str() {
                Some(text) => (text, &message["entities"]),
                None => (
                    message["caption"].as_str().unwrap_or_default(),
                    &message["caption_entities"],
                ),
            };
            let text = without_mention(text, entities, me);
            let files = files(message);
            if text.is_empty() && files.is_empty() {
                return None;
            }
            // A forum's topic, never a reply's thread in a group.
            let topic = match message["is_topic_message"] == true {
                true => message["message_thread_id"].as_i64(),
                false => None,
            };
            Some(Message {
                question_reply: None,
                event_id: message["message_id"].as_i64()?.to_string(),
                chat: message["chat"]["id"].as_i64()?,
                topic,
                text: Some(text),
                files,
                reply_to: Some(message["message_id"].as_i64()?),
                from,
            })
        }
        _ => None,
    }
}

/// Whether `message` is addressed to the bot: it mentions `@<username>`
/// in its text or caption, or answers one of the bot's messages.
fn addressed(message: &Value, me: &Me) -> bool {
    let mentions = |text: &Value, entities: &Value| {
        !mentioned(text.as_str().unwrap_or_default(), entities, me).is_empty()
    };
    let named = !me.username.is_empty()
        && (mentions(&message["text"], &message["entities"])
            || mentions(&message["caption"], &message["caption_entities"]));
    let replied = me.id != 0 && message["reply_to_message"]["from"]["id"].as_i64() == Some(me.id);
    named || replied
}

/// Where `entities` mention the bot in `text`: each span's start and end,
/// in UTF-16 units.
fn mentioned(text: &str, entities: &Value, me: &Me) -> Vec<(usize, usize)> {
    let name = format!("@{}", me.username);
    entities
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entity| {
            entity["type"] == "mention"
                && marked(text, entity).is_some_and(|m| m.eq_ignore_ascii_case(&name))
        })
        .filter_map(|entity| {
            let offset = usize::try_from(entity["offset"].as_u64()?).ok()?;
            let length = usize::try_from(entity["length"].as_u64()?).ok()?;
            Some((offset, offset + length))
        })
        .collect()
}

/// The part of `text` that `entity` marks: its offset and length count
/// UTF-16 units.
fn marked(text: &str, entity: &Value) -> Option<String> {
    let offset = usize::try_from(entity["offset"].as_u64()?).ok()?;
    let length = usize::try_from(entity["length"].as_u64()?).ok()?;
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16(units.get(offset..offset.checked_add(length)?)?).ok()
}

/// `text` without its mentions of the bot, which `entities` mark, trimmed.
fn without_mention(text: &str, entities: &Value, me: &Me) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut spans = mentioned(text, entities, me);
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

/// The thread of the chat `chat`: a person's, or a group's (negative);
/// in a forum, of its topic `topic`.
pub(crate) fn thread(chat: i64, topic: Option<i64>) -> String {
    format!("telegram:{}", at(chat, topic))
}

/// Chat `chat`, or topic `topic` of it, as `send` takes it.
pub(crate) fn at(chat: i64, topic: Option<i64>) -> String {
    match topic {
        Some(topic) => format!("{chat}.{topic}"),
        None => chat.to_string(),
    }
}

impl Message {
    /// Answers it in its chat, as the agent at `bridge` answers it: shown
    /// typing, the answer edited in as it's written, split at Telegram's
    /// limit. What `bridge` does with each message it polls, and `take`
    /// with each pushed. An answer Telegram didn't take is an error, one it
    /// refused (the chat gone, the bot blocked)
    /// [`link_connection_contract::ReplyRefused`].
    #[expect(
        clippy::disallowed_methods,
        reason = "paces the answer's updates and typing to Telegram's rate"
    )]
    pub(crate) async fn answer(&self, bot: &Bot, bridge: &Bridge) -> anyhow::Result<()> {
        if self.from.is_none()
            && self.files.is_empty()
            && let (Some(reply), Some(text)) = (&self.question_reply, &self.text)
            && let Some(answer) = connection::questions::caught_reply(
                &bridge.agent,
                &thread(self.chat, self.topic),
                reply,
                text,
                &self.event_id,
            )
        {
            return self.say(bot, &answer).await;
        }
        // In a group, as the agent's spec says.
        if self.reply_to.is_some() && !bridge.answers_groups() {
            return Ok(());
        }
        // Where an answer nobody waits for may go too.
        if self.reply_to.is_some()
            && self.from.is_none()
            && let Err(err) = connection::remember_group(&bridge.state, &self.chat.to_string())
        {
            link_connection_contract::warn(&format!("Telegram: keeping the group: {err:#}"));
        }
        let held =
            connection::holding(bridge, self.text.as_deref(), &self.files, |id| async move {
                bot.file(&id).await
            });
        let (text, parts) = match held.await {
            connection::Holding::Ask { text, parts } => (text, parts),
            connection::Holding::Answer(answer) => {
                return self.say(bot, &answer).await;
            }
        };
        let (progress, mut seen) = tokio::sync::watch::channel(String::new());
        let thread = thread(self.chat, self.topic);
        let from = self.from.as_ref().map(|member| connection::Speaker {
            from: member.person.as_str(),
            name: member.name.as_deref(),
            hops: 0,
        });
        let asking = connection::ask_holding(bridge, &thread, &text, &parts, from, &progress);
        tokio::pin!(asking);
        let mut shown = Shown::default();
        // Telegram shows "typing" for 5 seconds at most.
        let mut typing: Option<Instant> = None;
        let mut last: Option<Instant> = None;
        let every = connection::pace(bridge, floor(self.chat));
        let step = every.unwrap_or(floor(self.chat));
        let result = loop {
            if (seen.borrow().is_empty() || every.is_none())
                && typing.is_none_or(|t| t.elapsed() >= Duration::from_secs(4))
            {
                bot.act(self.chat, self.topic).await;
                typing = Some(Instant::now());
            }
            let tick = tokio::time::sleep(step.saturating_sub(last.map_or(step, |l| l.elapsed())));
            tokio::select! {
                result = &mut asking => break result,
                _ = tick => {
                    // Told to wait: no edit until then, and the next
                    // one shows what's new by then.
                    let so_far = seen.borrow_and_update().clone();
                    if !so_far.is_empty() && every.is_some() && !bot.held() {
                        // On the way: what wasn't sent goes with the whole.
                        let _ = shown.show(bot, self, &so_far, false).await;
                        last = Some(Instant::now());
                    }
                }
            }
        };
        // Joined a turn already running: its answer answers this too.
        let result = match result {
            // Nothing to mark it with on this connection.
            Ok(connection::Asked::Steered) => return Ok(()),
            Ok(connection::Asked::Answer(answer)) => Ok(answer),
            Err(err) => Err(err),
        };
        match result {
            Ok(answer) if answer.trim().is_empty() => {
                self.say(bot, "(Your agent answered nothing.)").await
            }
            Ok(answer) => shown.show(bot, self, &answer, true).await,
            Err(err) => {
                self.say(
                    bot,
                    &link_connection_contract::couldnt_answer(&bridge.agent, &err),
                )
                .await
            }
        }
    }

    /// Says `text` in its chat, all at once: [`Shown::show`] of the whole.
    async fn say(&self, bot: &Bot, text: &str) -> anyhow::Result<()> {
        Shown::default().show(bot, self, text, true).await
    }

    /// Sends `piece` in its chat, as a reply to the mention in a group, in
    /// its topic in a forum; its message's id ([`sent`]).
    async fn send(&self, bot: &Bot, piece: &Piece) -> anyhow::Result<Option<i64>> {
        let mut body = in_topic(json!({"chat_id": self.chat}), self.topic);
        if let Some(message) = self.reply_to {
            body["reply_parameters"] =
                json!({"message_id": message, "allow_sending_without_reply": true});
        }
        sent(bot.formatted("sendMessage", body, piece, true).await)
    }
}

/// An answer's messages as they're shown: each one's id and HTML.
#[derive(Default)]
struct Shown {
    messages: Vec<(i64, String)>,
}

impl Shown {
    /// Shows `answer` to `message`: each piece sent (the first as a reply
    /// to it, in a group), or edited where it changed. An edit told to
    /// wait waits and is tried again only for the `whole` answer: one on
    /// the way is out of date by then. A piece that couldn't be sent is
    /// the first such failure, once every piece was tried; an edit that
    /// failed isn't one, the answer being in the chat already.
    async fn show(
        &mut self,
        bot: &Bot,
        message: &Message,
        answer: &str,
        whole: bool,
    ) -> anyhow::Result<()> {
        let chat = message.chat;
        let mut failed = None;
        for (i, piece) in format::pieces(answer).into_iter().enumerate() {
            match self.messages.get_mut(i) {
                Some((_, text)) if *text == piece.out => {}
                Some((id, text)) => {
                    let edited = bot
                        .formatted(
                            "editMessageText",
                            json!({"chat_id": chat, "message_id": *id}),
                            &piece,
                            whole,
                        )
                        .await;
                    if edited.is_ok() {
                        *text = piece.out;
                    }
                }
                None => {
                    let sent = match i {
                        0 => message.send(bot, &piece).await,
                        _ => bot.send(chat, message.topic, &piece).await,
                    };
                    match sent {
                        Ok(Some(id)) => self.messages.push((id, piece.out)),
                        Ok(None) => {}
                        Err(err) => {
                            failed.get_or_insert(err);
                        }
                    }
                }
            }
        }
        failed.map_or(Ok(()), Err)
    }
}

/// Telegram answered a call and said no (`"ok": false`), saying why: never
/// its token refused, which is [`link_connection_contract::TokenRefused`], nor
/// told to wait.
#[derive(Debug)]
struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

/// The id of the message `sendMessage` sent; Telegram refusing it, the
/// connection refusing the answer ([`link_connection_contract::ReplyRefused`]).
fn sent(sent: anyhow::Result<Value>) -> anyhow::Result<Option<i64>> {
    match sent {
        Ok(message) => Ok(message["message_id"].as_i64()),
        Err(err) if err.is::<Refused>() => {
            Err(anyhow::Error::from(connection::ReplyRefused).context(err.to_string()))
        }
        Err(err) => Err(err),
    }
}

pub(crate) struct Bot {
    token: link_secretive::Secret,
    /// Until when Telegram said to wait, told so by an edit that didn't.
    held: std::sync::Mutex<Option<Instant>>,
}

impl Bot {
    pub(crate) fn new(token: &link_secretive::Secret) -> Self {
        Self {
            token: token.duplicate(),
            held: std::sync::Mutex::new(None),
        }
    }

    /// Whether Telegram said to wait, and that time hasn't passed.
    fn held(&self) -> bool {
        let held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        held.is_some_and(|until| Instant::now() < until)
    }

    /// Telegram's `method`. Told to wait (too many messages or edits),
    /// it waits as long as it's told, up to a minute, and tries again,
    /// a few times: an answer's last edit isn't lost to a rate limit.
    pub(crate) async fn call(&self, method: &str, body: Value) -> anyhow::Result<Value> {
        self.call_waiting(method, body, true).await
    }

    /// [`Bot::call`], or with `wait` false, failing at once when told to
    /// wait, and noting until when ([`Bot::held`]).
    #[expect(
        clippy::disallowed_methods,
        reason = "waits out Telegram's rate limit, as its retry_after says"
    )]
    async fn call_waiting(&self, method: &str, body: Value, wait: bool) -> anyhow::Result<Value> {
        let mut tries = 0;
        let response = loop {
            let url = self
                .token
                .expose_map(|token| format!("{}/bot{token}/{method}", api()));
            let response =
                crate::said(link_fetch::Method::POST, &url, method, |r| r.json(&body)).await?;
            tries += 1;
            match response["parameters"]["retry_after"].as_u64() {
                Some(after) if response["ok"] != true => {
                    let after = Duration::from_secs(after.max(1));
                    *self.held.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(Instant::now() + after);
                    if !wait || after > Duration::from_secs(60) || tries >= 4 {
                        break response;
                    }
                    tokio::time::sleep(after).await;
                }
                _ => break response,
            }
        };
        if response["ok"] != true {
            let why = response["description"]
                .as_str()
                .unwrap_or("no reason given");
            let refused = format!("Telegram refused {method}: {}", link_format::clean(why));
            // Told to wait past what's waited: not a refusal.
            if response["parameters"]["retry_after"].is_u64() {
                anyhow::bail!(refused);
            }
            return Err(Refused(refused).into());
        }
        Ok(response["result"].clone())
    }

    /// The bytes of the file `id`, as Telegram keeps it for the bot.
    async fn file(&self, id: &str) -> anyhow::Result<Vec<u8>> {
        let file = self.call("getFile", json!({"file_id": id})).await?;
        let path = file["file_path"]
            .as_str()
            .context("Telegram named no path of the file")?;
        let url = self
            .token
            .expose_map(|token| format!("{}/file/bot{token}/{path}", api()));
        let client = crate::client();
        let request = client.secret_url(link_fetch::Method::GET, &url, "a Telegram file");
        Ok(request.limit(connection::PART_MAX).bytes().await?)
    }

    /// Sends `piece`, in topic `topic` of a forum; its message's id
    /// ([`sent`]).
    async fn send(
        &self,
        chat: i64,
        topic: Option<i64>,
        piece: &Piece,
    ) -> anyhow::Result<Option<i64>> {
        let body = in_topic(json!({"chat_id": chat}), topic);
        sent(self.formatted("sendMessage", body, piece, true).await)
    }

    /// `method` (sending or editing a message), `body` with `piece` as
    /// its text, in Telegram's HTML. Refused for HTML Telegram couldn't
    /// read, it's sent once more as the piece's plain text.
    pub(crate) async fn formatted(
        &self,
        method: &str,
        body: Value,
        piece: &Piece,
        wait: bool,
    ) -> anyhow::Result<Value> {
        let formatting = &*format::FORMATTING;
        let mut html = body.clone();
        html["text"] = piece.out.clone().into();
        html["parse_mode"] = formatting.parse_mode.clone().into();
        match self.call_waiting(method, html, wait).await {
            Err(err) if err.is::<Refused>() && err.to_string().contains(&formatting.unparsed) => {
                let mut plain = body;
                plain["text"] = format::plain(piece).into();
                self.call_waiting(method, plain, wait).await
            }
            done => done,
        }
    }

    /// The bot's id and username, asked until Telegram answers.
    #[expect(
        clippy::disallowed_methods,
        reason = "backs off before asking Telegram again"
    )]
    async fn me(&self) -> Me {
        let mut wait = Duration::from_secs(1);
        loop {
            match self.call("getMe", json!({})).await {
                Ok(me) => return Me::of(&me),
                Err(err) => {
                    link_connection_contract::warn(&format!(
                        "Telegram: {err:#}; trying again in {}s",
                        wait.as_secs()
                    ));
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(60));
                }
            }
        }
    }

    /// Shows the agent typing, in topic `topic` of a forum.
    async fn act(&self, chat: i64, topic: Option<i64>) {
        // Only a hint: never waited for.
        if self.held() {
            return;
        }
        let _ = self
            .call_waiting(
                "sendChatAction",
                in_topic(json!({"chat_id": chat, "action": "typing"}), topic),
                false,
            )
            .await;
    }
}

#[cfg(test)]
mod tests;

//! `lnk-discord bridge`: carries the owner's direct messages to a harness
//! that speaks no Discord, and its answers back (`link_connection_contract`).
//! It listens on Discord's Gateway with the direct messages and server
//! messages intents, outside the harness's sandbox, so the harness never
//! holds the token, and resumes its session after a cut, so Discord sends
//! what it missed. Each direct message from an allowed user goes to the
//! endpoint alone, its thread `discord:<channel id>`; the agent shows
//! typing until the answer starts, which is posted as it's written, edited
//! in place about once a second, in the Markdown Discord reads
//! ([`crate::format`]), and split into more messages past its limit. An
//! allowed user's mention of the bot in a server goes too,
//! without the mention, its thread `discord:<the channel's id>`; the
//! answer is shown the same way, as a reply to it.
//! In a channel the agent is shared with ([`Bridge::shared`]), anyone's
//! mention of the bot goes too, as theirs (`Link-From:
//! member:discord:<user id>` and their name), never as the owner's; a
//! bot's, a webhook's and Discord's own never do.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use anyhow::Context;
use link_connection_contract::{self as connection, Bridge};
use reqwest::Method;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::{DIRECT_MESSAGES, Event, GUILD_MESSAGES, Limited, format, keep, request};

/// How often an answer being written may be shown anew, at most:
/// Discord's documented global limit, 50 requests a second; its other
/// limits are in each answer's headers, and change, so they're followed
/// as a rate limit says when Discord answers with one.
const FLOOR: Duration = Duration::from_millis(20);

/// How long to wait to show an answer again after it failed (an edit
/// refused, Discord not reached) without Discord saying how long.
const RETRY: Duration = Duration::from_secs(1);

/// How often "typing" is shown again: Discord shows it for 10 seconds.
const TYPING: Duration = Duration::from_secs(8);

/// The owner's direct messages with the bot, `dm_channel`, if connect
/// saved one (`lnk-discord owner`).
pub fn owner(bridge: &Bridge) -> anyhow::Result<Vec<String>> {
    let settings: link_harness_contract::harness::Discord = bridge
        .settings
        .expose()
        .read()
        .context("reading the Discord settings")?;
    Ok(settings.dm_channel.into_iter().collect())
}

/// Carries messages until it fails for good; a failure to reach Discord
/// is tried again.
/// Sends `text` to Discord channel `chat`, a message nobody asked for
/// (an answer to a schedule or a subagent): only the owner's direct
/// messages with the bot (`dm_channel`), or, while its spec lets it answer
/// in groups, a server's channel an allowed user mentioned the bot in, or
/// one the agent is shared with now ([`Bridge::shared`]), which `unshare`
/// ends; never another channel.
pub async fn send(
    bridge: Bridge,
    chat: String,
    text: String,
) -> anyhow::Result<connection::SentMessages> {
    let settings: link_harness_contract::harness::Discord = bridge
        .settings
        .expose()
        .read()
        .context("reading the Discord settings on stdin")?;
    let ours = settings.dm_channel.as_deref() == Some(chat.as_str())
        || (bridge.answers_groups()
            && (connection::knows_group(&bridge.state, &chat) || bridge.shared.contains(&chat)));
    anyhow::ensure!(
        ours,
        "Discord channel {chat} isn't the owner's direct messages, nor a channel they talk to \
         the agent in: not sending there"
    );
    let bot = Bot {
        token: settings.bot_token.duplicate(),
    };
    let mut messages = Vec::new();
    for piece in format::pieces(&text) {
        let message = bot.post(&chat, None, &piece).await?;
        if let Some(id) = message["id"].as_str() {
            messages.push(id.into());
        }
    }
    Ok(connection::SentMessages { messages })
}

pub async fn carry(bridge: Bridge) -> anyhow::Result<()> {
    let settings: link_harness_contract::harness::Discord = bridge
        .settings
        .expose()
        .read()
        .context("reading the Discord settings on stdin")?;
    let bot = Bot {
        token: settings.bot_token.duplicate(),
    };
    link_connection_contract::note(&format!("Carrying {}'s Discord messages", bridge.agent));
    // The Gateway is read (and its heartbeats kept) while answers are
    // written: one at a time and in order in each thread, threads side
    // by side.
    let (queue, messages) = mpsc::unbounded_channel::<Message>();
    let answering = connection::per_thread_interacting(
        messages,
        |message| thread(&message.connection),
        |message| {
            connection::questions::urgent(message.text.as_deref(), message.question_reply.is_some())
        },
        |message| {
            let (bot, bridge) = (&bot, &bridge);
            async move {
                if let Err(err) = message.answer(bot, bridge).await {
                    link_connection_contract::warn(&format!("Discord: {err:#}"));
                }
            }
        },
    );
    tokio::select! {
        () = keep_listening(&settings, &bridge.shared, queue) => {}
        () = answering => {}
    }
    Ok(())
}

/// Listens on the Gateway for good, connecting again whenever Discord
/// asks or the connection drops, and resuming where it can.
async fn keep_listening(
    settings: &link_harness_contract::harness::Discord,
    shared: &[String],
    queue: mpsc::UnboundedSender<Message>,
) {
    let mut seen = Seen::default();
    // The bot's own user id, from READY.
    let mut me = String::new();
    let intents = DIRECT_MESSAGES | GUILD_MESSAGES;
    keep(
        settings.bot_token.expose(),
        intents,
        "Discord",
        |event| match event {
            Event::Up(id) => me = id.to_string(),
            Event::Message(event) => {
                if let Some(message) = carried(event, &settings.allowed_users, shared, &me)
                    && seen.first(&message.id)
                {
                    let _ = queue.send(message);
                }
            }
            Event::Down | Event::Refused => {}
        },
    )
    .await
}

/// A message for the agent: the owner's, or a member's of a channel it's
/// shared with.
#[derive(Debug, PartialEq)]
pub(crate) struct Message {
    question_reply: Option<String>,
    /// The direct message's channel id, or the server channel's it
    /// mentions the bot in.
    pub(crate) connection: String,
    /// The message's own id.
    pub(crate) id: String,
    /// Its text; none when it holds only files or stickers.
    text: Option<String>,
    /// Its files, by their URLs on Discord's CDN.
    files: Vec<connection::Held<String>>,
    /// Whether it's a mention in a server, which the answer replies to.
    mention: bool,
    /// Who sent it, when it isn't the owner: someone in a channel the
    /// agent is shared with.
    pub(crate) from: Option<Member>,
}

/// Someone in a channel the agent is shared with.
#[derive(Debug, PartialEq)]
pub(crate) struct Member {
    /// As IAM writes them: `member:discord:<user id>`.
    pub(crate) person: link_agent_identity::access::ChatMember,
    /// The name they go by there: their nickname in the server, else
    /// their display name, else their username.
    name: Option<String>,
}

/// What `message` (a `MESSAGE_CREATE`'s) is to the bridge, and to
/// `sift` and `take` ([`crate::held`]): a direct message from a person
/// allowed, or their mention of the bot `me` (its user id) in a server,
/// without the mention; or anyone's mention of the bot in a channel
/// `shared` names, as theirs ([`Member`]). Anything else (the bot's own,
/// another bot's, a webhook's, a stranger's, a member's direct message,
/// an empty one, a server's that doesn't mention the bot, a mention with
/// no text left) is left unanswered. A channel's id names one channel of
/// one server, so a shared channel's messages are that server's alone; a
/// thread in it has an id of its own, and isn't shared.
pub(crate) fn carried(
    message: &Value,
    allowed: &[String],
    shared: &[String],
    me: &str,
) -> Option<Message> {
    let author = &message["author"];
    let user = author["id"].as_str()?;
    let connection = message["channel_id"].as_str()?;
    let direct = message["guild_id"].is_null();
    let human =
        author["bot"] != true && author["system"] != true && message["webhook_id"].is_null();
    // A plain message, or a reply; not a pin, a call or a member joining.
    let plain = matches!(message["type"].as_u64(), None | Some(0 | 19));
    let snowflake = |id: &str| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit());
    let id = message["id"].as_str().unwrap_or_default();
    if !(human && plain && user != me && snowflake(user) && snowflake(connection) && snowflake(id))
    {
        return None;
    }
    let from = match allowed.iter().any(|a| a == user) {
        true => None,
        // Only a mention in a channel it's shared with.
        false if !direct && shared.iter().any(|c| c == connection) => Some(Member {
            person: link_agent_identity::access::chat_member("discord", user).ok()?,
            name: [
                &message["member"]["nick"],
                &author["global_name"],
                &author["username"],
            ]
            .into_iter()
            .filter_map(Value::as_str)
            .find(|name| !name.trim().is_empty())
            .map(str::to_string),
        }),
        false => return None,
    };
    if !direct {
        let mentioned = message["mentions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|user| !me.is_empty() && user["id"] == me);
        let text = without_mention(message["content"].as_str().unwrap_or_default(), me);
        let files = files(message);
        return (mentioned && !(text.is_empty() && files.is_empty())).then(|| Message {
            question_reply: None,
            connection: connection.to_string(),
            id: id.to_string(),
            text: Some(text),
            files,
            mention: true,
            from,
        });
    }
    let text = message["content"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string);
    let held = [&message["attachments"], &message["sticker_items"]]
        .iter()
        .any(|v| v.as_array().is_some_and(|a| !a.is_empty()));
    (text.is_some() || held).then(|| Message {
        question_reply: message["message_reference"]["message_id"]
            .as_str()
            .map(str::to_owned),
        connection: connection.to_string(),
        id: id.to_string(),
        text,
        files: files(message),
        mention: false,
        from,
    })
}

/// The files `message` holds, each at its URL on Discord's CDN; one
/// anywhere else is left out.
fn files(message: &Value) -> Vec<connection::Held<String>> {
    message["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| {
            let url = file["url"].as_str().filter(|url| on_cdn(url))?;
            Some(connection::Held {
                media: file["content_type"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                name: file["filename"].as_str().map(str::to_string),
                size: file["size"].as_u64(),
                at: url.to_string(),
            })
        })
        .collect()
}

/// Whether `url` is on Discord's CDN, whose URLs are signed and need no
/// token: the bot's is never sent there. `LNK_DISCORD_API`'s host is
/// taken too (tests).
fn on_cdn(url: &str) -> bool {
    [
        "https://cdn.discordapp.com/",
        "https://media.discordapp.net/",
    ]
    .iter()
    .any(|cdn| url.starts_with(cdn))
        || std::env::var("LNK_DISCORD_API").is_ok_and(|api| url.starts_with(&format!("{api}/")))
}

/// `text` without its mentions of the bot `me` (`<@id>`, or `<@!id>`),
/// trimmed.
fn without_mention(text: &str, me: &str) -> String {
    if me.is_empty() {
        return text.trim().to_string();
    }
    text.replace(&format!("<@{me}>"), "")
        .replace(&format!("<@!{me}>"), "")
        .trim()
        .to_string()
}

/// The thread of messages in `channel`: a direct message's, or a server
/// channel's.
pub(crate) fn thread(connection: &str) -> String {
    format!("discord:{connection}")
}

/// The messages already carried, the latest 100: Discord can send one
/// again.
#[derive(Default)]
struct Seen(VecDeque<String>);

impl Seen {
    /// Whether message `id` is new, remembering it.
    fn first(&mut self, id: &str) -> bool {
        if self.0.iter().any(|seen| seen == id) {
            return false;
        }
        self.0.push_back(id.to_string());
        if self.0.len() > 100 {
            self.0.pop_front();
        }
        true
    }
}

impl Message {
    /// Answers it: the agent typing, its answer shown as it's written,
    /// split past Discord's limit. Ends once the answer is sent, or Discord
    /// refused it (the channel gone, the bot blocked or not let in):
    /// [`link_connection_contract::ReplyRefused`].
    pub(crate) async fn answer(&self, bot: &Bot, bridge: &Bridge) -> anyhow::Result<()> {
        if self.from.is_none()
            && self.files.is_empty()
            && let (Some(reply), Some(text)) = (&self.question_reply, &self.text)
            && let Some(answer) = connection::questions::caught_reply(
                &bridge.agent,
                &thread(&self.connection),
                reply,
                text,
                &self.id,
            )
        {
            return self.say(bot, &answer).await;
        }
        // Where an answer nobody waits for may go too: a channel the owner
        // talks to the agent in. A member's is sent to only while it's
        // shared (`send`), never kept.
        if self.mention
            && self.from.is_none()
            && let Err(err) = connection::remember_group(&bridge.state, &self.connection)
        {
            link_connection_contract::warn(&format!("Discord: keeping the channel: {err:#}"));
        }
        let held = connection::holding(
            bridge,
            self.text.as_deref(),
            &self.files,
            |url| async move { crate::download(&url).await },
        );
        let (text, parts) = match held.await {
            connection::Holding::Ask { text, parts } => (text, parts),
            connection::Holding::Answer(answer) => {
                return self.say(bot, &answer).await;
            }
        };
        // A reaction on the message while it's answered, as the agent's
        // spec says: none unless it names one.
        let working = bridge.spec.as_ref().and_then(|s| s.working.clone());
        if let Some(working) = &working {
            bot.react(Method::PUT, self, working).await;
        }
        let (progress, mut seen) = tokio::sync::watch::channel(String::new());
        let thread = thread(&self.connection);
        let from = self.from.as_ref().map(|member| connection::Speaker {
            from: member.person.as_str(),
            name: member.name.as_deref(),
            hops: 0,
        });
        let asking = connection::ask_holding(bridge, &thread, &text, &parts, from, &progress);
        tokio::pin!(asking);
        let mut shown = Shown::default();
        let mut typing: Option<Instant> = None;
        let every = connection::pace(bridge, FLOOR);
        // Not streaming: how often it looks for the answer's end.
        let step = every.unwrap_or(RETRY);
        let mut next = Instant::now() + step;
        let result = loop {
            if (seen.borrow().is_empty() || every.is_none())
                && typing.is_none_or(|t| t.elapsed() >= TYPING)
            {
                bot.typing(&self.connection).await;
                typing = Some(Instant::now());
            }
            tokio::select! {
                result = &mut asking => break result,
                _ = tokio::time::sleep_until(next.into()) => {
                    let so_far = seen.borrow_and_update().clone();
                    // Shown again a second on, or once a rate limit ends.
                    // On the way: what wasn't posted goes with the whole.
                    let wait = match so_far.is_empty() || every.is_none() {
                        true => None,
                        false => shown.show(bot, self, &so_far).await.unwrap_or(None),
                    };
                    next = Instant::now() + wait.unwrap_or(step).max(step);
                }
            }
        };
        // Joined a turn already running: its answer answers this too.
        let result = match result {
            Ok(connection::Asked::Steered) => {
                if let Some(steered) = bridge.spec.as_ref().and_then(|s| s.steered.clone()) {
                    bot.react(Method::PUT, self, &steered).await;
                }
                if let Some(working) = &working {
                    bot.react(Method::DELETE, self, working).await;
                }
                return Ok(());
            }
            Ok(connection::Asked::Answer(answer)) => Ok(answer),
            Err(err) => Err(err),
        };
        let answered = self.show_answer(bot, bridge, &mut shown, result).await;
        if let Some(working) = &working {
            bot.react(Method::DELETE, self, working).await;
        }
        answered
    }

    /// Shows the finished answer, `result`, or why there's none.
    #[expect(
        clippy::disallowed_methods,
        reason = "waits out Discord's rate limit before the rest of the answer"
    )]
    async fn show_answer(
        &self,
        bot: &Bot,
        bridge: &Bridge,
        shown: &mut Shown,
        result: anyhow::Result<String>,
    ) -> anyhow::Result<()> {
        match result {
            Ok(answer) if answer.trim().is_empty() => {
                self.say(bot, "(Your agent answered nothing.)").await
            }
            Ok(answer) => {
                // The whole answer, even past a rate limit.
                for _ in 0..3 {
                    match shown.show(bot, self, &answer).await? {
                        None => break,
                        Some(wait) => tokio::time::sleep(wait.max(RETRY)).await,
                    }
                }
                Ok(())
            }
            Err(err) => {
                self.say(
                    bot,
                    &link_connection_contract::couldnt_answer(&bridge.agent, &err),
                )
                .await
            }
        }
    }

    /// Says `text`, Markdown, in its channel, all at once: each piece
    /// posted, the first as a reply to a mention.
    async fn say(&self, bot: &Bot, text: &str) -> anyhow::Result<()> {
        for (i, piece) in format::pieces(text).into_iter().enumerate() {
            match i {
                0 => self.post(bot, &piece).await?,
                _ => answer_refused(bot.post(&self.connection, None, &piece).await)?,
            };
        }
        Ok(())
    }

    /// Posts `text`, Discord's Markdown, in its channel, as a reply to a
    /// mention.
    async fn post(&self, bot: &Bot, text: &str) -> anyhow::Result<Value> {
        let reply = self.mention.then_some(self.id.as_str());
        answer_refused(bot.post(&self.connection, reply, text).await)
    }
}

/// `posted`, Discord refusing it the connection refusing the answer
/// ([`link_connection_contract::ReplyRefused`]).
fn answer_refused(posted: anyhow::Result<Value>) -> anyhow::Result<Value> {
    match posted {
        Err(err) if err.is::<crate::Refused>() => {
            Err(anyhow::Error::from(connection::ReplyRefused).context(err.to_string()))
        }
        posted => posted,
    }
}

/// An answer's messages as they're shown: each one's id and text.
#[derive(Default)]
struct Shown {
    messages: Vec<(String, String)>,
}

impl Shown {
    /// Shows `answer` to `message`: each piece posted (the first as a
    /// reply to a mention), or edited where it changed. None when all of
    /// it is shown; else how long to wait before trying again; a piece
    /// Discord refused, [`link_connection_contract::ReplyRefused`].
    async fn show(
        &mut self,
        bot: &Bot,
        message: &Message,
        answer: &str,
    ) -> anyhow::Result<Option<Duration>> {
        let connection = &message.connection;
        let mut wait = None;
        for (i, piece) in format::pieces(answer).into_iter().enumerate() {
            let path = format!("/channels/{connection}/messages");
            match self.messages.get_mut(i) {
                Some((_, text)) if *text == piece => {}
                Some((id, text)) => {
                    let path = format!("{path}/{id}");
                    match bot.send(Method::PATCH, &path, &piece).await {
                        Ok(_) => *text = piece,
                        Err(err) => wait = Some(waited(&err)),
                    }
                }
                None => {
                    let sent = match i {
                        0 => message.post(bot, &piece).await,
                        _ => answer_refused(bot.post(connection, None, &piece).await),
                    };
                    match sent {
                        Ok(sent) => match sent["id"].as_str() {
                            Some(id) => self.messages.push((id.to_string(), piece)),
                            None => return Ok(Some(RETRY)),
                        },
                        Err(err) if connection::reply_refused(&err) => return Err(err),
                        Err(err) => return Ok(Some(waited(&err))),
                    }
                }
            }
        }
        Ok(wait)
    }
}

/// How long to wait after `err`: as long as a rate limit says.
fn waited(err: &anyhow::Error) -> Duration {
    err.downcast_ref::<Limited>()
        .map_or(RETRY, |limited| limited.0)
}

/// The bot, as its token: what answers go out as.
pub(crate) struct Bot {
    pub(crate) token: link_secretive::Secret,
}

impl Bot {
    /// Sends `text` to `path` (`method`: a new message, or an edit), able
    /// to mention no one: not @everyone, a role or a person.
    async fn send(&self, method: Method, path: &str, text: &str) -> anyhow::Result<Value> {
        let body = json!({"content": text, "allowed_mentions": {"parse": []}});
        self.sent(method, path, body).await
    }

    /// Sends `body` to `path`, its error printed.
    async fn sent(&self, method: Method, path: &str, body: Value) -> anyhow::Result<Value> {
        let sent = request(self.token.expose(), method.clone(), path, Some(body)).await;
        if let Err(err) = &sent {
            link_connection_contract::warn(&format!("{method} {path}: {err:#}"));
        }
        sent
    }

    /// Posts `text` in `channel`, as a reply to message `reply` if one is
    /// given, which it doesn't ping; able to mention no one.
    async fn post(
        &self,
        connection: &str,
        reply: Option<&str>,
        text: &str,
    ) -> anyhow::Result<Value> {
        let path = format!("/channels/{connection}/messages");
        let mut body = json!({"content": text, "allowed_mentions": {"parse": []}});
        if let Some(message) = reply {
            body["message_reference"] = json!({"message_id": message, "fail_if_not_exists": false});
        }
        self.sent(Method::POST, &path, body).await
    }

    /// Adds (`PUT`) or removes (`DELETE`) the bot's reaction `name` on
    /// `message`, by the emoji [`reaction`] names: a name it doesn't know
    /// is warned of, and a reaction Discord refused only warned of, the
    /// answer going on without it.
    async fn react(&self, method: Method, message: &Message, name: &str) {
        let Some(emoji) = reaction(name) else {
            link_connection_contract::warn(&format!(
                "Discord: no reaction is named {:?}",
                link_format::clean(name)
            ));
            return;
        };
        let path = format!(
            "/channels/{}/messages/{}/reactions/{}/@me",
            message.connection,
            message.id,
            encoded(emoji)
        );
        if let Err(err) = request(self.token.expose(), method.clone(), &path, None).await {
            link_connection_contract::warn(&format!("{method} {path}: {err:#}"));
        }
    }

    /// Shows the agent typing in `channel`.
    async fn typing(&self, connection: &str) {
        let path = format!("/channels/{connection}/typing");
        let _ = request(self.token.expose(), Method::POST, &path, None).await;
    }
}

/// The emoji reaction `name` (`eyes`) is on Discord, as
/// `reactions.toml` says.
pub(crate) fn reaction(name: &str) -> Option<&'static str> {
    static EMOJI: std::sync::LazyLock<std::collections::BTreeMap<String, String>> =
        std::sync::LazyLock::new(|| {
            link_spec::parse(include_str!("reactions.toml")).expect("reactions.toml reads")
        });
    EMOJI.get(name).map(String::as_str)
}

/// `emoji` as a URL's path holds it: each byte but a letter or digit
/// percent-encoded.
fn encoded(emoji: &str) -> String {
    emoji
        .bytes()
        .map(|b| match b.is_ascii_alphanumeric() {
            true => (b as char).to_string(),
            false => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each reaction's name is an emoji, and goes in a path encoded.
    #[test]
    fn reactions_are_named_emoji() {
        assert_eq!(reaction("eyes"), Some("👀"));
        assert_eq!(reaction("no_such_reaction"), None);
        assert_eq!(encoded("👀"), "%F0%9F%91%80");
    }

    /// The owner's direct chat, as `owner` says it for Link to ask on.
    #[test]
    fn the_owners_chat_is_said() {
        let bridge = Bridge {
            settings:
                serde_json::json!({"bot_token": "x", "allowed_users": ["9"], "dm_channel": "551"})
                    .into(),
            ..Bridge::default()
        };
        assert_eq!(owner(&bridge).unwrap(), ["551"]);
    }

    /// A message nobody asked for goes to the owner's direct messages with
    /// the bot alone: a server's channel, another's, or any with a bot
    /// connected before it was kept, is refused before Discord is asked.
    #[tokio::test]
    async fn send_refuses_a_chat_not_the_owners() {
        let bridge = |dm: Option<&str>| Bridge {
            settings: json!({"bot_token": "x", "allowed_users": ["2"], "dm_channel": dm}).into(),
            ..Bridge::default()
        };
        for (dm, chat) in [(Some("11"), "12"), (None, "11")] {
            let err = send(bridge(dm), chat.into(), "hi".into()).await;
            let err = format!("{:#}", err.unwrap_err());
            assert!(err.contains("not sending there"), "{chat}: {err}");
        }
    }

    #[test]
    fn only_the_owners_direct_messages() {
        let allowed = ["200000000000000002".to_string()];
        let message = |user: &str| {
            json!({"id": "1300000000000000001", "channel_id": "1200000000000000001",
                "type": 0, "content": "hi", "attachments": [],
                "author": {"id": user, "username": "dana"}})
        };
        let owner = message("200000000000000002");
        let owners = |message: &Value| carried(message, &allowed, &[], BOT);
        assert_eq!(
            owners(&owner),
            Some(Message {
                question_reply: None,
                connection: "1200000000000000001".into(),
                id: "1300000000000000001".into(),
                text: Some("hi".into()),
                files: vec![],
                mention: false,
                from: None,
            })
        );
        // A stranger.
        assert!(owners(&message("999")).is_none());
        // The owner in a server, not mentioning the bot.
        let mut server = owner.clone();
        server["guild_id"] = json!("900000000000000001");
        assert!(owners(&server).is_none());
        // A bot, or Discord itself, with the owner's id.
        let mut bot = owner.clone();
        bot["author"]["bot"] = json!(true);
        assert!(owners(&bot).is_none());
        let mut system = owner.clone();
        system["author"]["system"] = json!(true);
        assert!(owners(&system).is_none());
        // A pinned message's notice.
        let mut pin = owner.clone();
        pin["type"] = json!(6);
        assert!(owners(&pin).is_none());
        // A reply is a message.
        let mut reply = owner.clone();
        reply["type"] = json!(19);
        assert!(owners(&reply).is_some());
        // An id that isn't one, which would go in a path.
        let mut odd = owner.clone();
        odd["channel_id"] = json!("../1");
        assert!(owners(&odd).is_none());
        // Nothing in it: skipped; files alone: no text.
        let mut empty = owner.clone();
        empty["content"] = json!(" ");
        assert!(owners(&empty).is_none());
        let mut file = empty.clone();
        file["attachments"] = json!([{"id": "1", "filename": "a.png",
            "content_type": "image/png", "size": 9,
            "url": "https://cdn.discordapp.com/attachments/1/2/a.png?ex=1"},
            {"id": "2", "filename": "b.png", "url": "https://elsewhere.example/b.png"}]);
        let Some(file) = owners(&file) else {
            panic!("a file wasn't carried");
        };
        assert_eq!(file.text, None);
        // Only from Discord's CDN.
        assert_eq!(
            file.files,
            vec![connection::Held {
                media: "image/png".into(),
                name: Some("a.png".into()),
                size: Some(9),
                at: "https://cdn.discordapp.com/attachments/1/2/a.png?ex=1".into(),
            }]
        );
        assert!(owners(&json!({})).is_none());
    }

    const BOT: &str = "123456789012345678";

    #[test]
    fn the_owners_mention_in_a_server_is_carried_without_it() {
        let allowed = ["200000000000000002".to_string()];
        let mention = |user: &str, mentioned: &str| {
            json!({"id": "1300000000000000002", "channel_id": "1200000000000000003",
                "guild_id": "900000000000000001", "type": 0, "content": "<@123456789012345678> hi",
                "author": {"id": user, "username": "dana"},
                "mentions": [{"id": mentioned, "username": "link-main", "bot": true}]})
        };
        let expected = |text: &str| {
            Some(Message {
                question_reply: None,
                connection: "1200000000000000003".into(),
                id: "1300000000000000002".into(),
                text: Some(text.into()),
                files: vec![],
                mention: true,
                from: None,
            })
        };
        assert_eq!(
            carried(&mention("200000000000000002", BOT), &allowed, &[], BOT),
            expected("hi")
        );
        // The older form of a mention, and another user's left in.
        let mut nick = mention("200000000000000002", BOT);
        nick["content"] = json!("ask <@555> <@!123456789012345678>");
        assert_eq!(carried(&nick, &allowed, &[], BOT), expected("ask <@555>"));
        // Nothing left once the mention is out: skipped.
        let mut empty = mention("200000000000000002", BOT);
        empty["content"] = json!(" <@123456789012345678> ");
        assert!(carried(&empty, &allowed, &[], BOT).is_none());
        // Someone else mentioned, a stranger's mention, a bot's.
        let other = mention("200000000000000002", "555555555555555555");
        assert!(carried(&other, &allowed, &[], BOT).is_none());
        assert!(carried(&mention("999", BOT), &allowed, &[], BOT).is_none());
        let mut bot = mention("200000000000000002", BOT);
        bot["author"]["bot"] = json!(true);
        assert!(carried(&bot, &allowed, &[], BOT).is_none());
        // Before READY says who the bot is, nothing is its mention.
        assert!(carried(&mention("200000000000000002", BOT), &allowed, &[], "").is_none());
    }

    /// In a channel the agent is shared with, anyone's mention of the bot
    /// is carried as theirs, named as the server names them; nowhere else,
    /// never in a direct message, and never a bot's or a webhook's.
    #[test]
    fn a_members_mention_in_a_shared_channel_is_carried_as_theirs() {
        let allowed = ["200000000000000002".to_string()];
        let shared = ["1200000000000000003".to_string()];
        let mention = |user: &str, connection: &str| {
            json!({"id": "1300000000000000002", "channel_id": connection,
                "guild_id": "900000000000000001", "type": 0, "content": "<@123456789012345678> hi",
                "author": {"id": user, "username": "ali_k", "global_name": "Ali K"},
                "member": {"nick": "Ali"},
                "mentions": [{"id": BOT, "username": "link-main", "bot": true}]})
        };
        let member = "300000000000000003";
        let carried_as = |message: &Value| {
            carried(message, &allowed, &shared, BOT).map(|m| {
                let from = m.from.map(|f| (f.person.to_string(), f.name));
                (m.text, m.mention, from)
            })
        };
        let theirs = |name: &str| {
            Some((
                Some("hi".to_string()),
                true,
                Some((format!("member:discord:{member}"), Some(name.to_string()))),
            ))
        };
        let message = mention(member, "1200000000000000003");
        assert_eq!(carried_as(&message), theirs("Ali"));
        // Named by their display name, else their username.
        let mut plain = message.clone();
        plain["member"] = json!({"nick": null});
        assert_eq!(carried_as(&plain), theirs("Ali K"));
        plain["author"]["global_name"] = json!(null);
        assert_eq!(carried_as(&plain), theirs("ali_k"));
        // The owner, there too, is the owner.
        let owner = mention("200000000000000002", "1200000000000000003");
        assert_eq!(carried(&owner, &allowed, &shared, BOT).unwrap().from, None);
        // A channel not shared; a member's direct message.
        assert!(carried_as(&mention(member, "1200000000000000004")).is_none());
        let mut direct = message.clone();
        direct["guild_id"] = json!(null);
        assert!(carried_as(&direct).is_none());
        // Not mentioning the bot, or nothing left once it's out.
        let mut quiet = message.clone();
        quiet["mentions"] = json!([]);
        assert!(carried_as(&quiet).is_none());
        let mut empty = message.clone();
        empty["content"] = json!("<@123456789012345678>");
        assert!(carried_as(&empty).is_none());
        // A bot, a webhook, Discord itself, the bot's own.
        for (key, value) in [("bot", json!(true)), ("system", json!(true))] {
            let mut other = message.clone();
            other["author"][key] = value;
            assert!(carried_as(&other).is_none(), "{key}");
        }
        let mut hooked = message.clone();
        hooked["webhook_id"] = json!("1400000000000000001");
        assert!(carried_as(&hooked).is_none());
        assert!(carried_as(&mention(BOT, "1200000000000000003")).is_none());
        // An id that isn't one.
        assert!(carried_as(&mention("../1", "1200000000000000003")).is_none());
        // A member's reply to the bot is a message; a pin's or a thread's
        // notice isn't, nor is a message in a thread of the channel, which
        // has an id of its own.
        let mut reply = message.clone();
        reply["type"] = json!(19);
        assert_eq!(carried_as(&reply), theirs("Ali"));
        for kind in [6, 7, 18] {
            let mut notice = message.clone();
            notice["type"] = json!(kind);
            assert!(carried_as(&notice).is_none(), "type {kind}");
        }
        assert!(carried_as(&mention(member, "1300000000000000099")).is_none());
    }

    /// A message nobody asked for goes to a member's channel only while
    /// it's shared: refused before Discord is asked once it isn't.
    #[tokio::test]
    async fn send_reaches_a_shared_channel_only_while_shared() {
        let state = link_test_kit::TempDir::new("discord-send-shared");
        let bridge = Bridge {
            settings: json!({"bot_token": "x", "allowed_users": ["2"], "dm_channel": "11"}).into(),
            state: state.to_path_buf(),
            shared: vec!["13".into()],
            ..Bridge::default()
        };
        let err = send(bridge, "14".into(), "hi".into()).await.unwrap_err();
        assert!(format!("{err:#}").contains("not sending there"), "{err:#}");
    }

    #[test]
    fn a_thread_is_the_channels_id() {
        assert_eq!(thread("1200000000000000003"), "discord:1200000000000000003");
    }

    #[test]
    fn a_message_sent_again_is_carried_once() {
        let mut seen = Seen::default();
        assert!(seen.first("11"));
        assert!(!seen.first("11"));
        assert!(seen.first("12"));
    }

    #[test]
    fn a_rate_limit_says_how_long_to_wait() {
        let limited = anyhow::Error::from(Limited(Duration::from_millis(2500)));
        assert_eq!(waited(&limited), Duration::from_millis(2500));
        assert_eq!(waited(&anyhow::anyhow!("refused")), RETRY);
    }
}

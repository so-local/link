//! `lnk-discord`: the Discord connection module, which `lnk agent connect
//! discord` runs (`link_connection_contract`), and which carries the owner's
//! direct messages for a harness that speaks no Discord ([`bridge`]), or
//! holds many bots' Gateways for a machine that packs agents ([`held`]).
//!
//! What it asks Discord: the bot and its
//! application (whether it may read messages' text), and the user id of
//! the owner, so the harness can refuse everyone else. The owner is
//! whoever sends the bot, in a direct message, the one-time code printed
//! in the terminal: sharing a server with the bot isn't enough.
//!
//! A bot can't list its direct messages over HTTP, so Link watches them
//! arrive on Discord's Gateway (its websocket) for as long as it waits,
//! with only the direct messages intent.

use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

pub mod bridge;
mod format;
pub mod held;

/// The HTTP API base, and through its `/gateway/bot` the Gateway's.
/// `LNK_DISCORD_API` overrides it (tests).
fn api() -> String {
    std::env::var("LNK_DISCORD_API").unwrap_or_else(|_| "https://discord.com/api/v10".into())
}

/// The process's one client, so each call reuses a connection to
/// Discord rather than making a new one.
fn client() -> link_fetch::Client {
    static CLIENT: std::sync::OnceLock<link_fetch::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            link_fetch::Client::builder()
                .timeout(Duration::from_secs(30))
                .outside()
                .build()
                .expect("a client by Link's rules is made")
        })
        .clone()
}

/// A bot token is three dot-separated base64url parts: the bot's id, a
/// timestamp and a signature.
pub fn looks_like_token(token: &str) -> bool {
    let parts: Vec<_> = token.split('.').collect();
    parts.len() == 3
        && parts[0].len() >= 16
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// The bytes of a message's file at `url`, on Discord's CDN, which needs
/// no token.
pub(crate) async fn download(url: &str) -> anyhow::Result<Vec<u8>> {
    let client = client();
    let request = client.get(url).limit(link_connection_contract::PART_MAX);
    request.bytes().await.context("downloading a Discord file")
}

/// `GET <path>` (or `POST` with `body`), as the bot `token`, which goes in
/// a header, never the URL.
async fn call(token: &str, path: &str, body: Option<Value>) -> anyhow::Result<Value> {
    let method = match body {
        Some(_) => link_fetch::Method::POST,
        None => link_fetch::Method::GET,
    };
    request(token, method, path, body).await
}

/// `<method> <path>` with `body` as JSON, as the bot `token`; Discord's
/// answer, `null` when it's empty. A rate limit fails with [`Limited`].
pub(crate) async fn request(
    token: &str,
    method: link_fetch::Method,
    path: &str,
    body: Option<Value>,
) -> anyhow::Result<Value> {
    let authorization = link_secretive::Secret::new(format!("Bot {token}"));
    let client = client();
    let mut request = client
        .request(method, &format!("{}{path}", api()))
        .secret_header("Authorization", &authorization);
    if let Some(body) = &body {
        request = request.json(body);
    }
    let err = match request.answer::<Value>().await {
        Ok(value) => return Ok(value),
        Err(err) => err,
    };
    let Some(refusal) = err.refusal() else {
        return Err(anyhow::Error::from(err).context("reaching Discord"));
    };
    // Discord says why in its answer's JSON.
    let value: Value = refusal.read().unwrap_or(Value::Null);
    if refusal.status == 429 {
        let seconds = value["retry_after"].as_f64().unwrap_or(1.0);
        return Err(Limited(Duration::from_secs_f64(seconds.clamp(0.0, 60.0))).into());
    }
    let reason = value["message"].as_str().unwrap_or("no reason given");
    let refused = format!(
        "Discord refused ({}): {}",
        refusal.status,
        link_format::clean(reason)
    );
    // Unauthorized: the bot's token is revoked or wrong, for good.
    if refusal.status == 401 {
        return Err(anyhow::Error::from(link_connection_contract::TokenRefused).context(refused));
    }
    // Any other 4xx: Discord said no to this request; a 5xx is its own
    // failure, not a refusal.
    if (400..500).contains(&refusal.status) {
        return Err(Refused(refused).into());
    }
    bail!(refused)
}

/// Discord answered a request and said no (a 4xx), saying why: never its
/// token refused, which is [`link_connection_contract::TokenRefused`], nor a
/// rate limit ([`Limited`]).
#[derive(Debug)]
pub(crate) struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

/// Discord's rate limit: how long to wait before asking again.
#[derive(Debug)]
pub(crate) struct Limited(pub Duration);

impl std::fmt::Display for Limited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Discord asked to wait {:.1}s (rate limit)",
            self.0.as_secs_f64()
        )
    }
}

impl std::error::Error for Limited {}

/// A Discord bot, as its token says.
#[derive(Clone, Debug, PartialEq)]
pub struct Bot {
    /// Its user id, the same whichever token names it.
    pub id: String,
    pub name: String,
    /// Its application's id, which its invite link names.
    pub application: String,
    /// Whether its application has the Message Content intent on, which
    /// the harnesses ask for.
    pub message_content: bool,
}

/// The application flags of the Message Content intent (on, verified or
/// not).
const MESSAGE_CONTENT: u64 = (1 << 18) | (1 << 19);

/// Where Discord takes a bot's token, as data a proxy holding the token
/// enforces: its HTTP API reads it in the `Authorization` header, as
/// `Bot <token>`, under `api/v10/`; its Gateway, a WebSocket that stays
/// open, reads it in the first message, a login (`op` 2) or a resume
/// (`op` 6), at `/d/token`. The bot's own id is in its settings'
/// `bot_id`, so nothing reads the token to learn it.
pub fn rule() -> link_connection_contract::Rule {
    use link_secret_contract::route::{
        SecretHeader, SecretPath, SecretRest, SecretRule, SecretSocket,
    };
    link_connection_contract::Rule {
        field: "bot_token".into(),
        root: "api_root".into(),
        wire: SecretRule {
            host: "discord.com".into(),
            paths: vec![SecretPath {
                prefix: "api/v10/".into(),
                rest: SecretRest::Path,
            }],
            header: Some(SecretHeader::prefixed("Authorization", "Bot ")),
            methods: ["GET", "POST", "PUT", "PATCH", "DELETE"]
                .iter()
                .map(|method| method.to_string())
                .collect(),
            socket: Some(SecretSocket {
                host: "gateway.discord.gg".into(),
                path: "gateway".into(),
                pointer: "/d/token".into(),
            }),
            ..SecretRule::default()
        },
    }
}

/// The bot `token` is, which also proves it works.
pub async fn bot(token: &str) -> anyhow::Result<Bot> {
    let me = call(token, "/users/@me", None).await?;
    if me["bot"] != Value::Bool(true) {
        bail!("that token isn't a bot's: use the token from your application's Bot page");
    }
    let app = call(token, "/oauth2/applications/@me", None).await?;
    let text = |v: &Value| link_format::clean(v.as_str().unwrap_or_default());
    Ok(Bot {
        id: text(&me["id"]),
        name: text(&me["username"]),
        application: text(&app["id"]),
        message_content: app["flags"].as_u64().unwrap_or(0) & MESSAGE_CONTENT != 0,
    })
}

/// The page that adds the bot of `application` to a server of yours, so
/// you can send it direct messages: reading and sending messages, and its
/// slash commands.
pub fn invite_url(application: &str) -> String {
    reqwest::Url::parse_with_params(
        "https://discord.com/oauth2/authorize",
        &[
            ("client_id", application),
            ("scope", "bot applications.commands"),
            // View channels, send messages (and in threads), embed links,
            // attach files, read history, add reactions.
            ("permissions", "274878024768"),
        ],
    )
    .map(String::from)
    .unwrap_or_else(|_| "https://discord.com/developers/applications".into())
}

/// The Gateway's intent for direct messages. Their text comes with them
/// without the (privileged) Message Content intent.
pub(crate) const DIRECT_MESSAGES: u64 = 1 << 12;

/// The Gateway's intent for messages in servers. A message's text comes
/// with it without the Message Content intent only when it mentions the
/// bot; who it mentions always does.
pub(crate) const GUILD_MESSAGES: u64 = 1 << 9;

/// The owner: the first person to send the bot `code` in a direct message
/// (in a server, anyone could), with their user id, name and the direct
/// message's channel id. `ready` runs
/// once Link is listening, to show the code. Other messages are skipped.
/// Waits up to `limit`. The bot answers them, so they know it worked.
#[expect(
    clippy::disallowed_methods,
    reason = "reconnects after a pause when Discord asks or the connection drops"
)]
pub async fn owner(
    token: &str,
    code: &str,
    limit: Duration,
    ready: impl FnOnce(),
) -> anyhow::Result<(String, String, String)> {
    let start = Instant::now();
    let mut session = Gateway::new(token, gateway(token).await?, DIRECT_MESSAGES);
    let mut ready = Some(ready);
    loop {
        let left = limit.saturating_sub(start.elapsed());
        if left.is_zero() {
            break;
        }
        let seen = |event: Event| match event {
            Event::Up(_) => {
                if let Some(ready) = ready.take() {
                    ready();
                }
                None
            }
            Event::Message(message) => sent_code(message, code),
            Event::Down | Event::Refused => None,
        };
        match tokio::time::timeout(left, listen(&mut session, seen)).await {
            Err(_) => break,
            Ok(Ok((user, name, connection))) => {
                let _ = call(
                    token,
                    &format!("/channels/{connection}/messages"),
                    Some(json!({"content": "Got it: only you can talk to me now."})),
                )
                .await;
                return Ok((user, name, connection));
            }
            Ok(Err(Closed::Refused(why))) => bail!("Discord refused the bot's connection: {why}"),
            // Discord asked to reconnect, or the connection dropped.
            Ok(Err(Closed::Again)) => tokio::time::sleep(Duration::from_secs(2)).await,
        }
    }
    Err(link_connection_contract::Expired {
        connection: "discord".into(),
        waited: limit,
    }
    .into())
}

/// `lnk-discord share` (`link_connection_contract::Sharing`): waits up to
/// its `wait_secs` for a mention of the bot with the share's code in a
/// server's channel, from a user it's paired with (`allowed_users`):
/// nobody else's message is read, so only the owner shares. Says there
/// that the channel is shared, and gives the channel, with its name
/// (`#team`). The code is taken once, and never written anywhere.
#[expect(
    clippy::disallowed_methods,
    reason = "reconnects after a pause when Discord asks or the connection drops"
)]
pub async fn share(
    sharing: link_connection_contract::Sharing,
) -> anyhow::Result<link_connection_contract::Shared> {
    let settings: link_harness_contract::harness::Discord = sharing
        .settings
        .expose()
        .read()
        .context("reading the Discord settings on stdin")?;
    let token = settings.bot_token.expose();
    let limit = Duration::from_secs(sharing.wait_secs);
    let start = Instant::now();
    let mut session = Gateway::new(token, gateway(token).await?, GUILD_MESSAGES);
    // The bot's own user id, from READY: what a mention of it names.
    let mut me = String::new();
    // The messages refused already (a thread's), never read again.
    let mut refused: Vec<String> = Vec::new();
    loop {
        let left = limit.saturating_sub(start.elapsed());
        if left.is_zero() {
            break;
        }
        let seen = |event: Event| match event {
            Event::Up(id) => {
                me = id.to_string();
                None
            }
            Event::Message(message) => {
                shared_code(message, &settings.allowed_users, &me, &sharing.code)
                    .filter(|(_, id)| !refused.contains(id))
            }
            Event::Down | Event::Refused => None,
        };
        match tokio::time::timeout(left, listen(&mut session, seen)).await {
            Err(_) => break,
            Ok(Ok((connection, message))) => {
                let about = call(token, &format!("/channels/{connection}"), None)
                    .await
                    .context("reading the channel from Discord")?;
                let posts = format!("/channels/{connection}/messages");
                let reply = |content: String| {
                    let body = json!({
                        "content": content,
                        "allowed_mentions": {"parse": []},
                        "message_reference": {"message_id": message, "fail_if_not_exists": false},
                    });
                    call(token, &posts, Some(body))
                };
                // A thread has an id of its own, and the bridge answers
                // only the channel shared: the code stays unused.
                if matches!(about["type"].as_u64(), Some(THREADS_FROM..=THREADS_TO)) {
                    let _ = reply(
                        "A thread can't be shared: mention me with the code in its channel.".into(),
                    )
                    .await;
                    refused.push(message);
                    continue;
                }
                let name = about["name"]
                    .as_str()
                    .map(link_format::clean)
                    .filter(|name| !name.trim().is_empty());
                let said = match &name {
                    Some(name) => format!("Shared with #{name}: whoever is here can talk to me."),
                    None => "Shared with this channel: whoever is here can talk to me.".into(),
                };
                let _ = reply(said).await;
                return Ok(link_connection_contract::Shared {
                    chat: link_agent_identity::access::chat("discord", &connection)?,
                    name: name.map(|name| format!("#{name}")),
                });
            }
            Ok(Err(Closed::Refused(why))) => bail!("Discord refused the bot's connection: {why}"),
            // Discord asked to reconnect, or the connection dropped.
            Ok(Err(Closed::Again)) => tokio::time::sleep(Duration::from_secs(2)).await,
        }
    }
    bail!(
        "the code didn't reach the bot in {} minutes; run lnk agent share add discord again",
        limit.as_secs().div_ceil(60)
    )
}

/// Discord's channel types that are threads (announcement, public and
/// private), each with an id of its own beside its channel's.
const THREADS_FROM: u64 = 10;
const THREADS_TO: u64 = 12;

/// Where `message` (a `MESSAGE_CREATE`'s) shares a chat: its channel and
/// its own id, if a person in `allowed` (not a bot, a webhook or Discord)
/// mentioned the bot `me` with `code` in a server's channel, in a plain
/// message or a reply, as the bridge carries ([`bridge::carried`]).
fn shared_code(
    message: &Value,
    allowed: &[String],
    me: &str,
    code: &link_secretive::Secret,
) -> Option<(String, String)> {
    let author = &message["author"];
    let user = author["id"].as_str()?;
    let connection = message["channel_id"].as_str()?;
    let id = message["id"].as_str()?;
    let snowflake = |id: &str| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit());
    let human = author["bot"] != Value::Bool(true)
        && author["system"] != Value::Bool(true)
        && message["webhook_id"].is_null();
    let plain = matches!(message["type"].as_u64(), None | Some(0 | 19));
    let mentioned = !me.is_empty()
        && message["mentions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|user| user["id"] == me);
    let ours = !message["guild_id"].is_null()
        && human
        && plain
        && allowed.iter().any(|a| a == user)
        && mentioned
        && snowflake(connection)
        && snowflake(id);
    // The code is looked for only in a message that could share.
    (ours
        && link_connection_contract::has_code(
            message["content"].as_str().unwrap_or_default(),
            code,
        ))
    .then(|| (connection.to_string(), id.to_string()))
}

/// The Gateway's address for the bot `token`, from Discord's HTTP API.
pub(crate) async fn gateway(token: &str) -> anyhow::Result<String> {
    let gateway = call(token, "/gateway/bot", None).await?;
    Ok(at(gateway["url"]
        .as_str()
        .context("Discord sent no Gateway address")?))
}

/// The Gateway at `url`, as Link speaks to it: version 10, in JSON.
fn at(url: &str) -> String {
    format!("{}/?v=10&encoding=json", url.trim_end_matches('/'))
}

/// Why a Gateway session ended.
pub(crate) enum Closed {
    /// For good (a bad token, say).
    Refused(String),
    /// Discord asked to reconnect, or the connection dropped.
    Again,
}

/// What a Gateway connection sees.
pub(crate) enum Event<'a> {
    /// The bot is listening (READY, or RESUMED after a cut): its user id.
    Up(&'a str),
    /// A new message: a `MESSAGE_CREATE`'s data.
    Message(&'a Value),
    /// A connection that was up dropped ([`keep`] alone says so).
    Down,
    /// Down, and Discord refused the bot's token or intents (an HTTP 401,
    /// a close 4004, 4013 or 4014): it won't come up until they change
    /// ([`keep`] alone says so).
    Refused,
}

/// What [`keep`] last said of the connection.
#[derive(Clone, Copy, PartialEq)]
enum Said {
    Up,
    Down,
    Refused,
}

/// A bot's Gateway session, kept across its connections: a connection
/// that drops is resumed, and Discord sends what it missed.
pub(crate) struct Gateway {
    token: link_secretive::Secret,
    /// Where a new session starts.
    url: String,
    intents: u64,
    /// The bot's own user id, from READY.
    me: String,
    /// Discord's id for the session and where to resume it, from READY;
    /// none: a new session is started.
    resume: Option<(String, String)>,
    /// The last event's sequence number, which a heartbeat and a resume
    /// carry.
    sequence: Value,
}

impl Gateway {
    /// The bot `token`'s session at `url`, with `intents`, not started.
    pub(crate) fn new(token: &str, url: String, intents: u64) -> Self {
        Self {
            token: link_secretive::Secret::from(token.to_string()),
            url,
            intents,
            me: String::new(),
            resume: None,
            sequence: Value::Null,
        }
    }

    /// Forgets the session: the next connection starts a new one.
    fn forget(&mut self) {
        self.resume = None;
        self.sequence = Value::Null;
    }
}

/// Holds the bot `token`'s Gateway with `intents` for good, connecting
/// again whenever Discord asks or the connection drops, and resuming the
/// session where Discord lets it. `seen` sees each event, and
/// [`Event::Down`] once each time the connection goes down, or
/// [`Event::Refused`] once when Discord refuses it for good. `name`
/// heads its log lines.
#[expect(
    clippy::disallowed_methods,
    reason = "backs off before reconnecting to Discord"
)]
pub(crate) async fn keep(token: &str, intents: u64, name: &str, mut seen: impl FnMut(Event)) {
    let mut session: Option<Gateway> = None;
    // What it last said; none before the first connection.
    let mut said: Option<Said> = None;
    let mut wait = Duration::from_secs(1);
    loop {
        let started = Instant::now();
        let mut refused = false;
        if session.is_none() {
            match gateway(token).await {
                Ok(url) => session = Some(Gateway::new(token, url, intents)),
                Err(err) => {
                    refused = link_connection_contract::token_refused(&err);
                    link_connection_contract::warn(&format!("{name}: {err:#}; connecting again"));
                }
            }
        }
        if let Some(session) = &mut session {
            let up = |event: Event| {
                if matches!(event, Event::Up(_)) {
                    said = Some(Said::Up);
                }
                seen(event);
                None::<std::convert::Infallible>
            };
            match listen(session, up).await {
                Ok(never) => match never {},
                Err(Closed::Again) => link_connection_contract::warn(&format!(
                    "{name}: the connection closed; connecting again"
                )),
                Err(Closed::Refused(why)) => {
                    refused = true;
                    link_connection_contract::warn(&format!(
                        "{name}: Discord refused the bot's connection: {why}; trying again"
                    ))
                }
            }
        }
        let now = match refused {
            true => Said::Refused,
            false => Said::Down,
        };
        if said != Some(now) {
            said = Some(now);
            seen(match now {
                Said::Refused => Event::Refused,
                _ => Event::Down,
            });
        }
        // Discord asks now and then: then after a second.
        if started.elapsed() > Duration::from_secs(60) {
            wait = Duration::from_secs(1);
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(60));
    }
}

/// One connection to `session`'s Gateway: a new session (Identify), or
/// the one it had resumed (Resume), until `seen` finds what it looks for
/// in an event.
pub(crate) async fn listen<T>(
    session: &mut Gateway,
    mut seen: impl FnMut(Event) -> Option<T>,
) -> Result<T, Closed> {
    link_fetch::install_crypto_provider();
    let url = match &session.resume {
        Some((_, url)) => url.clone(),
        None => session.url.clone(),
    };
    let (mut ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|_| Closed::Again)?;
    // No heartbeat before Discord's Hello says how often (an interval's
    // first tick is at once).
    let mut beat = tokio::time::interval(Duration::from_secs(3600));
    beat.tick().await;
    // A heartbeat Discord didn't acknowledge means the connection is dead.
    let mut acked = true;
    loop {
        let frame = tokio::select! {
            frame = ws.next() => frame,
            _ = beat.tick() => {
                if !acked {
                    return Err(Closed::Again);
                }
                acked = false;
                let heartbeat = json!({"op": 1, "d": session.sequence}).to_string();
                ws.send(Message::text(heartbeat)).await.map_err(|_| Closed::Again)?;
                continue;
            }
        };
        let text = match frame {
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(Message::Close(frame))) => {
                return Err(match frame.map(|f| u16::from(f.code)) {
                    // Authentication failed, or intents it may not have.
                    Some(code @ (4004 | 4013 | 4014)) => {
                        session.forget();
                        Closed::Refused(format!("close code {code}"))
                    }
                    // A sequence it doesn't know, or a session timed out:
                    // a new one.
                    Some(4007 | 4009) => {
                        session.forget();
                        Closed::Again
                    }
                    _ => Closed::Again,
                });
            }
            Some(Ok(_)) => continue,
            _ => return Err(Closed::Again),
        };
        let Ok(event) = serde_json::from_str::<Value>(text.as_str()) else {
            continue;
        };
        if !event["s"].is_null() {
            session.sequence = event["s"].clone();
        }
        match event["op"].as_u64() {
            // Hello: heartbeat as asked, and say who it is, or which
            // session it resumes.
            Some(10) => {
                let every = event["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250);
                beat = tokio::time::interval(Duration::from_millis(every.max(1_000)));
                beat.tick().await;
                let hello = match &session.resume {
                    Some((id, _)) => json!({"op": 6, "d": {
                        "token": session.token.expose(),
                        "session_id": id,
                        "seq": session.sequence,
                    }}),
                    None => json!({"op": 2, "d": {
                        "token": session.token.expose(),
                        "intents": session.intents,
                        "properties": {"os": std::env::consts::OS, "browser": "lnk", "device": "lnk"},
                    }}),
                };
                ws.send(Message::text(hello.to_string()))
                    .await
                    .map_err(|_| Closed::Again)?;
            }
            // A heartbeat asked for now.
            Some(1) => {
                let heartbeat = json!({"op": 1, "d": session.sequence}).to_string();
                ws.send(Message::text(heartbeat))
                    .await
                    .map_err(|_| Closed::Again)?;
            }
            // Reconnect: the session goes on, resumed.
            Some(7) => return Err(Closed::Again),
            // An invalid session: resumed if Discord says it may be, else
            // a new one.
            Some(9) => {
                if event["d"] != true {
                    session.forget();
                }
                return Err(Closed::Again);
            }
            // A heartbeat acknowledged.
            Some(11) => acked = true,
            Some(0) => {
                let found = match event["t"].as_str() {
                    Some("READY") => {
                        let ready = &event["d"];
                        session.me = ready["user"]["id"].as_str().unwrap_or_default().into();
                        session.resume = ready["session_id"]
                            .as_str()
                            .zip(ready["resume_gateway_url"].as_str())
                            .map(|(id, url)| (id.to_string(), at(url)));
                        seen(Event::Up(&session.me))
                    }
                    Some("RESUMED") => seen(Event::Up(&session.me)),
                    Some("MESSAGE_CREATE") => seen(Event::Message(&event["d"])),
                    _ => None,
                };
                if let Some(found) = found {
                    let _ = ws.close(None).await;
                    return Ok(found);
                }
            }
            _ => {}
        }
    }
}

/// Who sent `message` and where, if a person (not a bot) sent `code` in
/// it, in a direct message.
fn sent_code(message: &Value, code: &str) -> Option<(String, String, String)> {
    let author = &message["author"];
    let direct = message["guild_id"].is_null();
    let human = author["bot"] != Value::Bool(true) && author["system"] != Value::Bool(true);
    let has_code = message["content"]
        .as_str()
        .is_some_and(|text| link_connection_contract::is_code(text, code));
    let id = author["id"].as_str()?;
    let snowflake = !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit());
    if !(direct && human && has_code && snowflake) {
        return None;
    }
    let name = [&author["global_name"], &author["username"]]
        .into_iter()
        .filter_map(Value::as_str)
        .find(|n| !n.is_empty())
        .map_or_else(|| "you".into(), link_format::clean);
    Some((
        id.to_string(),
        name,
        message["channel_id"].as_str()?.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_keeps_the_token_to_discord() {
        let rule = rule();
        rule.wire.check().unwrap();
        let socket = rule.wire.socket.unwrap();
        assert_eq!(
            (socket.host.as_str(), socket.pointer.as_str()),
            ("gateway.discord.gg", "/d/token")
        );
    }

    #[test]
    fn tokens() {
        assert!(looks_like_token(concat!(
            "MTIzNDU2Nzg5MDEyMzQ1Njc4",
            ".GabcDe.",
            "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-"
        )));
        assert!(!looks_like_token("MTIzNDU2Nzg5MDEyMzQ1Njc4.GabcDe"));
        assert!(!looks_like_token("short.GabcDe.AbCdEf"));
        assert!(!looks_like_token(
            "MTIzNDU2Nzg5MDEyMzQ1Njc4.Gab cDe.AbCdEfGhIjKlMnOpQrStUvWxYz"
        ));
    }

    /// A chat is shared only by a paired user's mention of the bot with
    /// the code, in any case, in a server's channel.
    #[test]
    fn only_the_owners_mention_with_the_code_shares_a_channel() {
        let code = link_secretive::Secret::from("K7Q2M9XA".to_string());
        let allowed = ["200000000000000002".to_string()];
        let me = "123456789012345678";
        let mention = |user: &str, content: &str| {
            json!({"id": "1300000000000000002", "channel_id": "1200000000000000003",
                "guild_id": "900000000000000001", "content": content,
                "author": {"id": user, "username": "dana"},
                "mentions": [{"id": me, "bot": true}]})
        };
        let shared = |message: &Value| shared_code(message, &allowed, me, &code);
        let owners = mention("200000000000000002", "<@123456789012345678> k7q2m9xa");
        assert_eq!(
            shared(&owners),
            Some(("1200000000000000003".into(), "1300000000000000002".into()))
        );
        // Another code, no code, a stranger with the code.
        assert!(
            shared(&mention(
                "200000000000000002",
                "<@123456789012345678> K7Q2M9XB"
            ))
            .is_none()
        );
        assert!(shared(&mention("200000000000000002", "<@123456789012345678> hi")).is_none());
        assert!(
            shared(&mention(
                "300000000000000003",
                "<@123456789012345678> K7Q2M9XA"
            ))
            .is_none()
        );
        // Not mentioning the bot; before READY says who it is.
        let mut quiet = owners.clone();
        quiet["mentions"] = json!([]);
        assert!(shared(&quiet).is_none());
        assert!(shared_code(&owners, &allowed, "", &code).is_none());
        // A direct message; a bot or a webhook with the owner's id.
        let mut direct = owners.clone();
        direct["guild_id"] = json!(null);
        assert!(shared(&direct).is_none());
        let mut bot = owners.clone();
        bot["author"]["bot"] = json!(true);
        assert!(shared(&bot).is_none());
        let mut hooked = owners.clone();
        hooked["webhook_id"] = json!("1400000000000000001");
        assert!(shared(&hooked).is_none());
        // The code inside a longer word; a notice, not a message; a reply
        // is a message.
        assert!(
            shared(&mention(
                "200000000000000002",
                "<@123456789012345678> xK7Q2M9XA"
            ))
            .is_none()
        );
        let mut notice = owners.clone();
        notice["type"] = json!(7);
        assert!(shared(&notice).is_none());
        let mut reply = owners.clone();
        reply["type"] = json!(19);
        assert!(shared(&reply).is_some());
        // A channel id that isn't one.
        let mut odd = owners.clone();
        odd["channel_id"] = json!("../1");
        assert!(shared(&odd).is_none());
    }

    #[test]
    fn only_a_person_sending_the_code_directly_is_the_owner() {
        let sent = |m: Value| sent_code(&m, "K7Q2M9XA").map(|(id, name, _)| (id, name));
        let dana = json!({"id": "200000000000000002", "username": "dana", "global_name": "Dana"});
        assert_eq!(
            sent(json!({"author": dana, "channel_id": "1", "content": "k7q2m9xa"})),
            Some(("200000000000000002".into(), "Dana".into()))
        );
        assert_eq!(
            sent(json!({"author": dana, "channel_id": "1", "content": "hi"})),
            None
        );
        // In a server, anyone could have.
        assert_eq!(
            sent(
                json!({"author": dana, "guild_id": "9", "channel_id": "1", "content": "K7Q2M9XA"})
            ),
            None
        );
        let bot = json!({"id": "1", "username": "b", "bot": true});
        assert_eq!(
            sent(json!({"author": bot, "channel_id": "1", "content": "K7Q2M9XA"})),
            None
        );
    }

    #[test]
    fn the_invite_adds_the_bot_with_what_it_needs() {
        let url = invite_url("1234567890");
        assert!(
            url.starts_with("https://discord.com/oauth2/authorize?client_id=1234567890&scope=bot"),
            "{url}"
        );
        assert!(url.contains("permissions=274878024768"), "{url}");
    }
}

#[cfg(test)]
link_test_kit::program!(unit);

/// `lnk-discord`'s command line, generated from its declarations: the
/// connection protocol's commands its contract declares (`actions.toml`),
/// whose shapes are the contract's (`link_connection_contract::declared`).
pub mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));

    /// The connection protocol's command this run is given.
    pub fn operation() -> link_connection_contract::Operation {
        use link_connection_contract::Operation;
        let parsed: link_spec::Parsed<Cli> = PROGRAM.parse();
        match parsed.command.command {
            Command::DiscordConnect(given) => Operation::Connect {
                agent: given.agent,
                taken: given.taken,
            },
            Command::Uninstall(given) => Operation::Uninstall(given),
            // Every other command is given nothing: the protocol's, by its
            // words.
            _ => Operation::of(&parsed.words).expect("a command of the connection protocol"),
        }
    }

    /// `lnk-discord`'s whole command line, as clap sees it.
    pub fn command() -> clap::Command {
        <Cli as clap::CommandFactory>::command()
    }
}

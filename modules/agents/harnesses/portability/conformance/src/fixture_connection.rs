//! The connection fixture's service, faked: what `lnk-connection-fixture` (the core's
//! fixture, `tests/fixtures/connection`) talks to, for a test whose
//! agent needs a connection but whose subject isn't one: the core's own
//! behaviour carried on a connection, and a harness's beside the core. Its
//! bots are its tokens', each with the messages people sent it, which a
//! poll confirms as Telegram's offset does, and what it sent.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use serde_json::{Value, json};

/// What `lnk-connection-fixture` is pointed at: `LNK_CONNECTION_FIXTURE_SERVICE`.
pub const SERVICE: &str = "LNK_CONNECTION_FIXTURE_SERVICE";

/// A bot's token, `<bot id>:<secret>`.
pub fn token(id: u32) -> String {
    format!("{id}:mini{id:0>8}secret{id:0>8}")
}

/// An agent's settings for the connection fixture, its section of them: the
/// bot whose token is `token`, paired with `owner`, as `lnk agent
/// connect connection-fixture` would leave it.
pub fn section(token: &str, owner: &str) -> String {
    let id = token.split(':').next().unwrap_or_default();
    format!(
        "[connections.connection-fixture.settings]\ntoken = \"{token}\"\nallowed_users = [\"{owner}\"]\n\n\
         [connections.connection-fixture.bot]\nid = \"{id}\"\nname = \"mini_{id}_bot\"\n\n\
         [connections.connection-fixture.about]\ntitle = \"Connection fixture\"\nsecret_fields = [\"token\"]\n\
         bridged = true\n"
    )
}

/// One bot of the service.
#[derive(Default)]
struct Bot {
    /// What people sent it: each message's `{"id", "from", "text"}`, its
    /// ids from 1.
    messages: Vec<Value>,
    /// The first message not yet confirmed.
    confirmed: u64,
    /// What it sent: each chat and text.
    sent: Vec<(String, String)>,
    /// When it sent each of `sent`.
    sent_at: Vec<Instant>,
}

/// The service, its bots by token.
#[derive(Clone, Default)]
pub struct ConnectionFixture {
    bots: Arc<Mutex<BTreeMap<String, Bot>>>,
    /// Woken by each message people send, ending a poll that waits.
    arrived: Arc<tokio::sync::Notify>,
    /// Where it listens: `http://127.0.0.1:<port>`.
    pub url: String,
}

impl ConnectionFixture {
    /// The service, listening on a port of 127.0.0.1.
    pub async fn serve() -> Self {
        let mut service = Self::default();
        let port = crate::agent::serve(
            Router::new()
                .route("/{bot}/{method}", post(call))
                .with_state(service.clone()),
        )
        .await;
        service.url = format!("http://127.0.0.1:{port}");
        service
    }

    /// The variable pointing `lnk-connection-fixture` at it.
    pub fn env(&self) -> (&'static str, String) {
        (SERVICE, self.url.clone())
    }

    /// `from` sends the bot whose token is `token` `text`.
    pub fn message(&self, token: &str, from: &str, text: &str) {
        let mut bots = self.bots.lock().unwrap();
        let bot = bots.entry(token.to_string()).or_default();
        let id = bot.messages.len() + 1;
        bot.messages
            .push(json!({"id": id, "from": from, "text": text}));
        self.arrived.notify_waiters();
    }

    /// Every bot that was called: its token.
    pub fn tokens(&self) -> Vec<String> {
        self.bots.lock().unwrap().keys().cloned().collect()
    }

    /// What the bot whose token is `token` sent: each chat and text.
    pub fn sent(&self, token: &str) -> Vec<(String, String)> {
        let bots = self.bots.lock().unwrap();
        bots.get(token)
            .map(|bot| bot.sent.clone())
            .unwrap_or_default()
    }

    /// What it sent to `chat`.
    pub fn sent_to(&self, token: &str, chat: &str) -> Vec<String> {
        let sent = self.sent(token).into_iter();
        sent.filter(|(to, _)| to == chat)
            .map(|(_, text)| text)
            .collect()
    }

    /// Whether it sent `chat` something holding `text`.
    pub fn answered(&self, token: &str, chat: &str, text: &str) -> bool {
        self.answered_at(token, chat, text).is_some()
    }

    /// When it first sent `chat` something holding `text`: the reply's
    /// own time, however late a poll for it looks.
    pub fn answered_at(&self, token: &str, chat: &str, text: &str) -> Option<Instant> {
        let bots = self.bots.lock().unwrap();
        let bot = bots.get(token)?;
        let mut at = bot.sent.iter().zip(&bot.sent_at);
        at.find(|((to, sent), _)| to == chat && sent.contains(text))
            .map(|(_, at)| *at)
    }
}

/// `POST /bot<token>/<method>`.
async fn call(
    State(service): State<ConnectionFixture>,
    Path((bot, method)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let token = bot.strip_prefix("bot").unwrap_or(&bot).to_string();
    match method.as_str() {
        "me" => {
            let id = token.split(':').next().unwrap_or_default();
            Json(json!({"id": id, "name": format!("mini_{id}_bot")}))
        }
        "poll" => {
            let confirm = body["confirm"].as_u64().unwrap_or(0);
            let start = Instant::now();
            // A long poll, shortened.
            loop {
                // Registered before looking, so a message sent between
                // the look and the wait still ends it.
                let arrived = service.arrived.notified();
                tokio::pin!(arrived);
                arrived.as_mut().enable();
                let ready: Vec<Value> = {
                    let mut bots = service.bots.lock().unwrap();
                    let bot = bots.entry(token.clone()).or_default();
                    bot.confirmed = bot.confirmed.max(confirm);
                    let from = bot.confirmed;
                    let ready = bot.messages.iter();
                    ready
                        .filter(|m| m["id"].as_u64().unwrap() >= from)
                        .cloned()
                        .collect()
                };
                let left = Duration::from_millis(500).saturating_sub(start.elapsed());
                if !ready.is_empty() || left.is_zero() {
                    return Json(json!({"messages": ready}));
                }
                let _ = tokio::time::timeout(left, arrived).await;
            }
        }
        "send" => {
            let mut bots = service.bots.lock().unwrap();
            let bot = bots.entry(token).or_default();
            let chat = body["chat"].as_str().unwrap_or_default().to_string();
            let text = body["text"].as_str().unwrap_or_default().to_string();
            bot.sent.push((chat, text));
            bot.sent_at.push(Instant::now());
            Json(json!({"id": bot.sent.len()}))
        }
        _ => Json(json!({"error": "no such method"})),
    }
}

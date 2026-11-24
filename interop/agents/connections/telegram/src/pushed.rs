//! The bot's messages pushed to a machine that packs agents
//! (`link_connection_contract::pushed`), instead of polled by `bridge`.
//! `hook` sets the bot's webhook, each push carrying a secret, and
//! `unhook` deletes it; `sift` checks the secret through Link's one way
//! in (`link_secretive::inbound`, Telegram's scheme) and keeps an
//! allowed user's message, as `bridge` keeps one it polls; `take` answers
//! it as `bridge` does.

use anyhow::Context;
use link_connection_contract::pushed::{
    self, Hooked, Hooking, Pushed, Sifting, Taking, Unhooking, Verdict,
};
use link_secretive::inbound::{Inbound, Receiver, Scheme};
use serde_json::{Value, json};

use crate::bridge::{self, Bot, Me};
use crate::format;

/// Telegram's pushes: `hook`, `unhook`, `sift` and `take`.
pub fn pushed() -> Pushed {
    Pushed {
        hook: Some(|hooking| Box::pin(hook(hooking))),
        unhook: Some(|unhooking| Box::pin(unhook(unhooking))),
        sift,
        take: |taking| Box::pin(take(taking)),
        hold: None,
    }
}

/// The connection's settings, with its token.
fn settings(settings: &Value) -> anyhow::Result<link_harness_contract::harness::Telegram> {
    let settings: link_harness_contract::harness::Telegram =
        serde_json::from_value(settings.clone())
            .context("reading the Telegram settings on stdin")?;
    anyhow::ensure!(
        !settings.bot_token.is_empty(),
        "the Telegram settings hold no bot token"
    );
    Ok(settings)
}

/// Whether `secret` is one Telegram takes as a webhook's `secret_token`:
/// 1 to 256 of `A-Z`, `a-z`, `0-9`, `_` and `-`.
fn legal_secret(secret: &str) -> bool {
    (1..=256).contains(&secret.len())
        && secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Whether Telegram may push to `url`: HTTPS, or, against a Bot API of a
/// test's (`LNK_TELEGRAM_API`), HTTP to this machine.
fn pushable(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost"));
    match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => local && std::env::var_os("LNK_TELEGRAM_API").is_some(),
        _ => false,
    }
}

/// Sets the bot's webhook to `url`, each push carrying the secret, only
/// its messages pushed, and those waiting kept; prints who may talk to
/// the agent and who the bot is, for `sift`. A roll is this again with
/// the new secret: Telegram pushes with it from then on, and `sift` takes
/// the previous one beside it until it's dropped.
async fn hook(hooking: Hooking) -> anyhow::Result<Hooked> {
    let telegram = settings(hooking.settings.expose())?;
    anyhow::ensure!(
        legal_secret(hooking.secret.expose()),
        "a webhook's secret is 1 to 256 of A-Z, a-z, 0-9, _ and -"
    );
    anyhow::ensure!(
        pushable(&hooking.url),
        "Telegram pushes only to an https:// address"
    );
    let bot = Bot::new(&telegram.bot_token);
    let me = Me::of(&bot.call("getMe", json!({})).await?);
    anyhow::ensure!(!me.username.is_empty(), "Telegram sent no bot name");
    bot.call(
        "setWebhook",
        json!({
            "url": hooking.url,
            "secret_token": hooking.secret.expose(),
            "allowed_updates": ["message"],
            "drop_pending_updates": false,
        }),
    )
    .await?;
    Ok(Hooked {
        settings: json!({
            "allowed_users": telegram.allowed_users,
            "bot_id": me.id,
            "bot_username": link_format::clean(&me.username),
        }),
        steps: Vec::new(),
    })
}

/// Deletes the bot's webhook, its messages waiting kept for `bridge`.
async fn unhook(unhooking: Unhooking) -> anyhow::Result<()> {
    let telegram = settings(unhooking.settings.expose())?;
    Bot::new(&telegram.bot_token)
        .call("deleteWebhook", json!({"drop_pending_updates": false}))
        .await?;
    Ok(())
}

/// One push: refused as Link's one way in answers (401 without the
/// webhook's secret, its current one or, while it rolls, the one before;
/// 413 over [`pushed::UPDATE_MAX`]; 503 with no secret at all), dropped
/// 200 when it isn't a message `bridge` would answer; else taken, its
/// thread `bridge`'s and its `update_id` the dedupe, so Telegram's retry
/// of it is answered once.
/// A `say` is Telegram's `sendMessage` in the push's response, in its HTML
/// ([`format`]), or taken when it's longer than one message.
fn sift(sifting: &Sifting) -> Verdict {
    if let Some(say) = &sifting.say {
        let Some((chat, topic)) = bridge::place(&say.chat) else {
            return Verdict::drop(400);
        };
        let pieces = format::pieces(&say.text);
        let [piece] = &pieces[..] else {
            return Verdict::Take {
                thread: bridge::thread(chat, topic),
                chat: say.chat.clone(),
                dedupe: String::new(),
                from: None,
            };
        };
        let say = json!({
            "method": "sendMessage",
            "chat_id": chat,
            "text": piece.out,
            "parse_mode": format::FORMATTING.parse_mode,
        });
        return Verdict::json(&bridge::in_topic(say, topic));
    }
    let inbound = Inbound {
        receiver: Receiver::new("telegram").expect("a name"),
        scheme: Scheme::TELEGRAM,
        max_body: pushed::UPDATE_MAX,
        secrets: sifting.secrets(),
    };
    // Telegram's scheme signs no time.
    if let Err(refused) = inbound.verify(&sifting.headers, sifting.body.as_bytes(), 0) {
        return Verdict::drop(refused.status());
    }
    let Ok(update) = serde_json::from_str::<Value>(&sifting.body) else {
        return Verdict::drop(400);
    };
    let allowed: Vec<i64> = sifting.settings["allowed_users"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .collect();
    let me = Me {
        id: sifting.settings["bot_id"].as_i64().unwrap_or(0),
        username: sifting.settings["bot_username"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    };
    match (
        bridge::carried(&update["message"], &allowed, &sifting.shared, &me),
        update["update_id"].as_i64(),
    ) {
        (Some(message), Some(id)) => Verdict::Take {
            thread: bridge::thread(message.chat, message.topic),
            chat: bridge::at(message.chat, message.topic),
            dedupe: id.to_string(),
            from: message.from.map(|member| member.person),
        },
        _ => Verdict::drop(200),
    }
}

/// Answers one pushed update as `bridge` answers a message it polls;
/// returns once the answer is sent, or as its answer failed: Telegram
/// refusing it, [`link_connection_contract::ReplyRefused`].
async fn take(taking: Taking) -> anyhow::Result<()> {
    let telegram = settings(taking.bridge.settings.expose())?;
    let bot = Bot::new(&telegram.bot_token);
    // Who the bot is, to find its mention in a group.
    let me = Me::of(&bot.call("getMe", json!({})).await?);
    let Some(message) = bridge::carried(
        &taking.update["message"],
        &telegram.allowed_users,
        &taking.bridge.shared,
        &me,
    ) else {
        return Ok(());
    };
    message.answer(&bot, &taking.bridge).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_pushes_carry_the_member_and_unsharing_stops_them() {
        let body = json!({"update_id": 9, "message": {"message_id": 7,
            "from": {"id": 43, "is_bot": false}, "chat": {"id": -1001, "type": "group"},
            "text": "hello", "reply_to_message": {"from": {"id": 123456}}}})
        .to_string();
        let mut sifting = Sifting {
            settings: json!({"allowed_users": [42], "bot_id": 123456, "bot_username": "my_bot"}),
            secret: "shared-secret".to_string().into(),
            headers: [(
                "x-telegram-bot-api-secret-token".into(),
                "shared-secret".into(),
            )]
            .into(),
            body,
            ..Default::default()
        };
        assert_eq!(sift(&sifting), Verdict::drop(200));
        sifting.shared = vec!["-1001".into()];
        assert_eq!(
            sift(&sifting),
            Verdict::Take {
                thread: "telegram:-1001".into(),
                chat: "-1001".into(),
                dedupe: "9".into(),
                from: Some(link_agent_identity::access::chat_member("telegram", "43").unwrap()),
            }
        );
        sifting.shared.clear();
        assert_eq!(sift(&sifting), Verdict::drop(200));
    }

    #[test]
    fn secrets_telegram_takes() {
        assert!(legal_secret("aZ09_-"));
        assert!(legal_secret(&"a".repeat(256)));
        assert!(!legal_secret(""));
        assert!(!legal_secret(&"a".repeat(257)));
        assert!(!legal_secret("has space"));
        assert!(!legal_secret("dot.ted"));
    }

    #[test]
    fn only_https_is_pushed_to() {
        assert!(pushable("https://push.example.com/t/abc"));
        assert!(!pushable("http://push.example.com/t/abc"));
        assert!(!pushable("ftp://push.example.com/"));
        assert!(!pushable("not a url"));
    }
}

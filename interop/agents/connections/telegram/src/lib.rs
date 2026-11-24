//! `lnk-telegram`: the Telegram connection module, which `lnk agent connect
//! telegram` runs (`link_connection_contract`), and which carries the
//! messages for a harness that speaks no Telegram ([`bridge`]).
//!
//! What it asks Telegram's Bot API: the bot's name, and
//! the user id of the owner, so the harness can refuse everyone else. The
//! owner is whoever sends the bot, in a private chat, the one-time code
//! printed in the terminal: knowing the bot's name isn't enough.

pub mod bridge;
mod format;
pub mod pushed;

use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::Value;

/// The Bot API base. `LNK_TELEGRAM_API` overrides it (tests).
pub(crate) fn api() -> String {
    std::env::var("LNK_TELEGRAM_API").unwrap_or_else(|_| "https://api.telegram.org".into())
}

/// The process's one client, so each call reuses a connection to
/// Telegram rather than making a new one.
pub(crate) fn client() -> link_fetch::Client {
    static CLIENT: std::sync::OnceLock<link_fetch::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            link_fetch::Client::builder()
                .timeout(Duration::from_secs(60))
                .outside()
                .build()
                .expect("a client by Link's rules is made")
        })
        .clone()
}

/// What Telegram answered `method` at `url` (which holds the bot's
/// token, so it's named by its method alone), its refusals too: Telegram
/// says why in the answer's JSON (`"ok": false`).
pub(crate) async fn said(
    method: link_fetch::Method,
    url: &link_secretive::Secret,
    name: &str,
    build: impl FnOnce(link_fetch::Request<'_>) -> link_fetch::Request<'_>,
) -> anyhow::Result<Value> {
    let client = client();
    let request = build(client.secret_url(method, url, &format!("Telegram's {name}")));
    let said = match request.answer::<Value>().await {
        Ok(said) => said,
        Err(err) => match err.refusal().and_then(|r| r.read::<Value>().ok()) {
            Some(said) => said,
            None => return Err(anyhow::Error::from(err).context("reaching Telegram")),
        },
    };
    // Unauthorized: the bot's token is revoked or wrong, for good.
    if said["error_code"] == 401 {
        let why = said["description"].as_str().unwrap_or("no reason given");
        return Err(
            anyhow::Error::from(link_connection_contract::TokenRefused).context(format!(
                "Telegram refused {name}: {}",
                link_format::clean(why)
            )),
        );
    }
    Ok(said)
}

/// What `lnk-telegram describe` and `rule` say of Telegram.
pub fn about() -> link_connection_contract::About {
    link_connection_contract::About {
        name: "telegram",
        describe: link_connection_contract::Describe {
            title: "Telegram".into(),
            secret_fields: vec!["bot_token".into()],
            hosts: vec!["api.telegram.org".into()],
            disconnect_hint: "Your bot stays yours: delete it with BotFather (/deletebot).".into(),
            ..Default::default()
        },
        rule: Some(rule()),
        send: Some(|bridge, chat, text| Box::pin(crate::bridge::send(bridge, chat, text))),
        pushed: Some(pushed::pushed()),
        share: Some(|sharing| Box::pin(crate::bridge::share(sharing))),
        owner: Some(crate::bridge::owner),
    }
}

/// How a harness speaking Telegram reaches the Bot API without the bot's
/// token (`lnk-telegram rule --json`): it's given a placeholder in
/// `bot_token` and its proxy as `api_root`, and the proxy adds the token
/// to the paths the Bot API takes it in (`bot<token>/<method>`,
/// `file/bot<token>/<path>`). What would hand the bot's updates, or the
/// bot, to something outside Link is refused: `setWebhook`, `logOut` and
/// `close` (Telegram's method names ignore case). The bot's id, before
/// the token's `:`, isn't secret, and stays in the placeholder.
pub fn rule() -> link_connection_contract::Rule {
    use link_secret_contract::route::{SecretPath, SecretRest, SecretRule};
    link_connection_contract::Rule {
        field: "bot_token".into(),
        root: "api_root".into(),
        wire: SecretRule {
            host: "api.telegram.org".into(),
            paths: vec![
                SecretPath {
                    prefix: "bot{secret}/".into(),
                    rest: SecretRest::Name,
                },
                SecretPath {
                    prefix: "file/bot{secret}/".into(),
                    rest: SecretRest::Path,
                },
            ],
            header: None,
            refuse: ["setwebhook", "logout", "close"]
                .iter()
                .map(|method| format!("bot{{secret}}/{method}"))
                .collect(),
            methods: vec!["GET".into(), "POST".into()],
            public: Some(":".into()),
            socket: None,
        },
    }
}

/// A bot token looks like `123456:ABC-...`.
pub fn looks_like_token(token: &str) -> bool {
    token.split_once(':').is_some_and(|(id, secret)| {
        !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && secret.len() >= 20
    })
}

async fn call(token: &str, method: &str, query: &[(&str, String)]) -> anyhow::Result<Value> {
    let url = link_secretive::Secret::new(format!("{}/bot{token}/{method}", api()));
    let response = said(link_fetch::Method::GET, &url, method, |r| r.query(&query)).await?;
    if response["ok"] != Value::Bool(true) {
        let reason = response["description"]
            .as_str()
            .unwrap_or("no reason given");
        bail!("Telegram refused: {}", link_format::clean(reason));
    }
    Ok(response["result"].clone())
}

/// The bot's id: the first part of its token, which isn't secret.
pub fn bot_id(token: &str) -> Option<i64> {
    token.split_once(':')?.0.parse().ok()
}

/// Who else reads a bot's messages ([`polled_elsewhere`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elsewhere {
    /// Another program polls it.
    Polled,
    /// Its messages are pushed to a webhook.
    Hooked,
}

/// What a person is told when the bot's messages are pushed to a webhook,
/// so a bridge can't poll them.
pub const HOOKED: &str = "this bot's messages are pushed to a webhook (the agent moved to a \
                          machine that packs agents?): lnk agent move brings it back";

/// Whether Telegram's refusal `why` of `getUpdates` is its 409 for a bot
/// whose webhook is set ("can't use getUpdates method while webhook is
/// active"), not another program polling it.
pub fn webhook_set(why: &str) -> bool {
    why.to_ascii_lowercase().contains("webhook is active")
}

/// Whether another program reads the bot's messages: Telegram then
/// answers `getUpdates` with 409, its reason telling a webhook set from
/// another poller. Asked a few times, 2 seconds apart, since a poller
/// that just stopped (this agent's own) holds on for a moment. Nothing is
/// marked read.
#[expect(
    clippy::disallowed_methods,
    reason = "backs off before asking Telegram again"
)]
pub async fn polled_elsewhere(token: &str) -> anyhow::Result<Option<Elsewhere>> {
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let url = link_secretive::Secret::new(format!("{}/bot{token}/getUpdates", api()));
        let response = said(link_fetch::Method::GET, &url, "getUpdates", |r| {
            r.query(&[("timeout", "0"), ("limit", "1")])
        })
        .await?;
        if response["error_code"] != 409 {
            return Ok(None);
        }
        if webhook_set(response["description"].as_str().unwrap_or_default()) {
            return Ok(Some(Elsewhere::Hooked));
        }
    }
    Ok(Some(Elsewhere::Polled))
}

/// The bot's `@username`, which also proves the token works.
pub async fn bot_name(token: &str) -> anyhow::Result<String> {
    let me = call(token, "getMe", &[]).await?;
    let name = me["username"]
        .as_str()
        .context("Telegram sent no bot name")?;
    Ok(link_format::clean(name))
}

/// The owner: the first person to send the bot `code` in a private chat
/// (a group's members can't claim it), with their user id and first
/// name. Other messages are skipped. Waits up to `limit`. The message is
/// marked read, so the harness doesn't answer it later.
pub async fn owner(token: &str, code: &str, limit: Duration) -> anyhow::Result<(i64, String)> {
    let start = Instant::now();
    // Skip anything sent before now.
    let mut offset = latest_update(token).await?.map_or(0, |id| id + 1);
    while start.elapsed() < limit {
        let updates = call(
            token,
            "getUpdates",
            &[
                ("offset", offset.to_string()),
                ("timeout", "25".into()),
                ("allowed_updates", r#"["message"]"#.into()),
            ],
        )
        .await?;
        for update in updates.as_array().into_iter().flatten() {
            offset = offset.max(update["update_id"].as_i64().unwrap_or(0) + 1);
            let message = &update["message"];
            let from = &message["from"];
            let private = message["chat"]["type"] == "private";
            let has_code = message["text"]
                .as_str()
                .is_some_and(|text| link_connection_contract::is_code(text, code));
            let human = from["is_bot"] != Value::Bool(true);
            if let (Some(id), true) = (from["id"].as_i64(), private && has_code && human) {
                let name = link_format::clean(from["first_name"].as_str().unwrap_or("you"));
                // Acknowledge it.
                let _ = call(
                    token,
                    "getUpdates",
                    &[("offset", offset.to_string()), ("timeout", "0".into())],
                )
                .await;
                return Ok((id, name));
            }
        }
    }
    Err(link_connection_contract::Expired {
        connection: "telegram".into(),
        waited: limit,
    }
    .into())
}

async fn latest_update(token: &str) -> anyhow::Result<Option<i64>> {
    let updates = call(
        token,
        "getUpdates",
        &[("offset", "-1".into()), ("timeout", "0".into())],
    )
    .await?;
    Ok(updates
        .as_array()
        .and_then(|u| u.last())
        .and_then(|u| u["update_id"].as_i64()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert!(looks_like_token(concat!(
            "123456789",
            ":AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw"
        )));
        assert!(!looks_like_token("AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw"));
        assert!(!looks_like_token("12a:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw"));
        assert!(!looks_like_token("123:short"));
    }

    /// The rule the sandbox's test of the verb uses, as data.
    #[test]
    fn the_rule_keeps_the_token_to_the_bot_api() {
        let rule = rule();
        rule.wire.check().unwrap();
        assert_eq!(
            serde_json::to_value(&rule).unwrap(),
            serde_json::json!({
                "field": "bot_token",
                "root": "api_root",
                "host": "api.telegram.org",
                "paths": [
                    {"prefix": "bot{secret}/", "rest": "name"},
                    {"prefix": "file/bot{secret}/", "rest": "path"},
                ],
                "refuse": ["bot{secret}/setwebhook", "bot{secret}/logout", "bot{secret}/close"],
                "methods": ["GET", "POST"],
                "public": ":",
            })
        );
        let placeholder = link_secret_contract::route::placeholder(
            &link_sandbox_route::Placeholders::new().unwrap(),
            "telegram",
            &concat!("1234", "56:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw")
                .to_string()
                .into(),
            rule.wire.public.as_deref(),
        );
        let placeholder = placeholder.expose();
        assert!(placeholder.starts_with("123456:link-wire-"));
        assert_eq!(bot_id(placeholder), Some(123456));
    }
}

#[cfg(test)]
link_test_kit::program!(system);

/// `lnk-telegram`'s command line, generated from its declarations: the
/// connection protocol's commands its contract declares (`actions.toml`),
/// whose shapes are the contract's (`link_connection_contract::declared`).
pub mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));

    /// The connection protocol's command this run is given.
    pub fn operation() -> link_connection_contract::Operation {
        use link_connection_contract::Operation;
        let parsed: link_spec::Parsed<Cli> = PROGRAM.parse();
        match parsed.command.command {
            Command::TelegramConnect(given) => Operation::Connect {
                agent: given.agent,
                taken: given.taken,
            },
            Command::Uninstall(given) => Operation::Uninstall(given),
            // Every other command is given nothing: the protocol's, by its
            // words.
            _ => Operation::of(&parsed.words).expect("a command of the connection protocol"),
        }
    }

    /// `lnk-telegram`'s whole command line, as clap sees it.
    pub fn command() -> clap::Command {
        <Cli as clap::CommandFactory>::command()
    }
}

//! `lnk-slack`: the Slack connection module, which `lnk agent connect slack`
//! runs (`link_connection_contract`), and which carries the messages for a
//! harness that speaks no Slack ([`bridge`]).
//!
//! What it asks Slack's Web API: the app's bot and
//! workspace, whether its app-level token opens a Socket Mode connection,
//! and the member id of the owner, so the harness can refuse everyone
//! else. The owner is whoever sends the bot, in a direct message, the
//! one-time code printed in the terminal: being in the workspace isn't
//! enough.
//!
//! Where Slack pushes the app's events to an address instead, on a
//! machine that packs agents, [`pushed`] switches it there and reads
//! them.

pub mod bridge;
mod format;
mod pairing;
pub mod pushed;
mod socket;
pub use pairing::{Pairing, PairingProgress};

use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::{Value, json};

/// The Web API base. `LNK_SLACK_API` overrides it (tests).
fn api() -> String {
    std::env::var("LNK_SLACK_API").unwrap_or_else(|_| "https://slack.com/api".into())
}

/// The process's one client, so each call reuses a connection to
/// Slack rather than making a new one.
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

/// Whether `url` is Slack's own, where a message's files are kept: the
/// bot's token is sent nowhere else. `LNK_SLACK_API`'s host is taken
/// too (tests).
pub(crate) fn on_slack(url: &str) -> bool {
    url.starts_with("https://files.slack.com/")
        || std::env::var("LNK_SLACK_API").is_ok_and(|api| url.starts_with(&format!("{api}/")))
}

/// The bytes of a message's file at `url` ([`on_slack`]), with the bot's
/// `token`.
pub(crate) async fn download(token: &link_secretive::Secret, url: &str) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(on_slack(url), "a file not on Slack");
    let client = client();
    let request = client
        .get(url)
        .bearer(token)
        .limit(link_connection_contract::PART_MAX);
    request.bytes().await.context("downloading a Slack file")
}

/// A bot token looks like `xoxb-...`.
pub fn looks_like_bot_token(token: &str) -> bool {
    looks_like(token, "xoxb-")
}

/// An app-level token looks like `xapp-...`.
pub fn looks_like_app_token(token: &str) -> bool {
    looks_like(token, "xapp-")
}

/// A signing secret, which signs each event Slack pushes, is 32 hex
/// digits.
pub fn looks_like_signing_secret(secret: &str) -> bool {
    secret.len() == 32 && secret.bytes().all(|b| b.is_ascii_hexdigit())
}

/// An app configuration token, which edits the app's manifest, looks
/// like `xoxe.xoxp-...`.
pub fn looks_like_config_token(token: &str) -> bool {
    token.strip_prefix("xoxe.").is_some_and(|rest| {
        rest.len() >= 20
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    })
}

fn looks_like(token: &str, prefix: &str) -> bool {
    token.strip_prefix(prefix).is_some_and(|rest| {
        rest.len() >= 20 && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// Web API method `method` with `token`, which goes in a header, never
/// the URL.
///
/// Told to wait (a 429 with `Retry-After`), it waits as long as it's told,
/// up to a minute, and tries again, a few times.
pub(crate) async fn call(
    token: &str,
    method: &str,
    form: &[(&str, &str)],
) -> anyhow::Result<Value> {
    call_waiting(token, method, form, None).await
}

/// [`call`]; with `held`, failing at once when told to wait, and noting
/// there until when.
#[expect(
    clippy::disallowed_methods,
    reason = "waits out Slack's rate limit, as its Retry-After says"
)]
pub(crate) async fn call_waiting(
    token: &str,
    method: &str,
    form: &[(&str, &str)],
    held: Option<&std::sync::Mutex<Option<std::time::Instant>>>,
) -> anyhow::Result<Value> {
    use link_fetch::Error;
    let token = link_secretive::Secret::new(token.to_string());
    let mut tries = 0;
    let response = loop {
        let answer = client()
            .post(&format!("{}/{method}", api()))
            .bearer(&token)
            .form(&form)
            .answer::<Value>()
            .await;
        tries += 1;
        let err = match answer {
            Ok(said) => break said,
            Err(err) => err,
        };
        let wait = match &err {
            Error::Later(refusal) => refusal.retry_after().map(|after| after.as_secs()),
            _ => None,
        };
        match (wait, held) {
            (Some(wait), Some(held)) => {
                let until = std::time::Instant::now() + std::time::Duration::from_secs(wait.max(1));
                *held.lock().unwrap_or_else(|e| e.into_inner()) = Some(until);
            }
            (Some(wait), None) if wait <= 60 && tries < 4 => {
                tokio::time::sleep(std::time::Duration::from_secs(wait.max(1))).await;
                continue;
            }
            _ => {}
        }
        // Slack says why in its answer's JSON, a refusal's too.
        match err
            .refusal()
            .and_then(|refusal| refusal.read::<Value>().ok())
        {
            Some(said) => break said,
            None => return Err(anyhow::Error::from(err).context("reaching Slack")),
        }
    };
    if response["ok"] != Value::Bool(true) {
        let reason = response["error"].as_str().unwrap_or("no reason given");
        let refused = format!("Slack refused: {}", link_format::clean(reason));
        // The bot's token is revoked, wrong or its account gone, for good.
        if TOKEN_REFUSED.contains(&reason) {
            return Err(
                anyhow::Error::from(link_connection_contract::TokenRefused).context(refused)
            );
        }
        // Told to wait past what's waited: not a refusal.
        if reason == "ratelimited" {
            bail!(refused);
        }
        return Err(Refused(refused).into());
    }
    Ok(response)
}

/// Slack answered a call and said no (`"ok": false`), saying why: never
/// its token refused, which is [`link_connection_contract::TokenRefused`], nor
/// told to wait.
#[derive(Debug)]
pub(crate) struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

/// What Slack says when it refuses the token itself.
const TOKEN_REFUSED: [&str; 4] = [
    "invalid_auth",
    "not_authed",
    "token_revoked",
    "account_inactive",
];

/// A Slack app's bot, as its bot token says.
#[derive(Clone, Debug, PartialEq)]
pub struct Bot {
    /// Its member id (`U...`): the same bot whichever token names it.
    pub id: String,
    /// Its name (`link-main`).
    pub name: String,
    /// Its workspace's name.
    pub team: String,
    /// Its permanent workspace ID, used to refuse Slack Connect outsiders.
    pub team_id: String,
}

/// The bot `token` is, which also proves it works.
pub async fn bot(token: &str) -> anyhow::Result<Bot> {
    let me = call(token, "auth.test", &[]).await?;
    let text = |key: &str| link_format::clean(me[key].as_str().unwrap_or_default());
    let bot = Bot {
        id: text("user_id"),
        name: text("user"),
        team: text("team"),
        team_id: text("team_id"),
    };
    if bot.id.is_empty() || me["bot_id"].as_str().is_none() {
        bail!("that token isn't a bot's: use the Bot User OAuth Token (xoxb-...)");
    }
    Ok(bot)
}

/// Whether the app-level `token` opens a Socket Mode connection (it has
/// `connections:write`, and the app has Socket Mode on). Nothing is
/// connected: Slack only hands out an address.
pub async fn check_app_token(token: &str) -> anyhow::Result<()> {
    call(token, "apps.connections.open", &[])
        .await
        .context("checking the app-level token (it needs connections:write, and Socket Mode on)")?;
    Ok(())
}

/// Who sent `message`, if a person (not a bot, nor an edit or a join)
/// sent `code` in it.
fn sent_code(message: &Value, code: &str) -> Option<String> {
    let human = message["bot_id"].is_null() && message["subtype"].is_null();
    let has_code = message["text"]
        .as_str()
        .is_some_and(|text| pairing_code_matches(text, code));
    let user = message["user"].as_str()?;
    let member = !user.is_empty() && user.bytes().all(|b| b.is_ascii_alphanumeric());
    (human && has_code && member).then(|| user.to_string())
}

/// Slack preserves pasted bold and code styling in its message text.
/// Only a whole code, plain or inside one matching wrapper, can pair.
fn pairing_code_matches(text: &str, code: &str) -> bool {
    static WRAPPERS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
        let mut fields: std::collections::BTreeMap<String, Vec<String>> =
            link_spec::parse(include_str!("pairing-code.toml"))
                .expect("Slack pairing wrappers read");
        let wrappers = fields
            .remove("wrappers")
            .expect("pairing wrappers are declared");
        assert!(fields.is_empty(), "no unknown Slack pairing fields");
        wrappers
    });
    let text = text.trim();
    link_connection_contract::is_code(text, code)
        || WRAPPERS.iter().any(|wrapper| {
            text.strip_prefix(wrapper.as_str())
                .and_then(|inner| inner.strip_suffix(wrapper.as_str()))
                .is_some_and(|inner| link_connection_contract::is_code(inner, code))
        })
}

/// A member's name, as they show it; "you" if Slack won't say.
async fn name_of(token: &str, user: &str) -> String {
    let Ok(info) = call(token, "users.info", &[("user", user)]).await else {
        return "you".into();
    };
    let user = &info["user"];
    [
        &user["profile"]["display_name"],
        &user["real_name"],
        &user["name"],
    ]
    .into_iter()
    .filter_map(Value::as_str)
    .find(|name| !name.is_empty())
    .map_or_else(|| "you".into(), link_format::clean)
}

/// The app Link has the user make: a bot that takes direct messages, in
/// Socket Mode (no public address), with what a harness that speaks
/// Slack reads and posts. `name` is the agent's. Its events are the
/// same pushed to a request URL, which only `hook` fills in
/// ([`pushed`]).
pub fn manifest(name: &str) -> Value {
    let app = format!("Link {name}");
    json!({
        "display_information": {
            "name": app,
            "description": "Your agent, through Link",
        },
        "features": {
            "bot_user": {"display_name": app, "always_online": true},
            "app_home": {
                "home_tab_enabled": false,
                "messages_tab_enabled": true,
                "messages_tab_read_only_enabled": false,
            },
        },
        "oauth_config": {
            "scopes": {
                "bot": [
                    "app_mentions:read",
                    "channels:history",
                    "channels:read",
                    "chat:write",
                    "files:read",
                    "files:write",
                    "groups:history",
                    "groups:read",
                    "im:history",
                    "im:read",
                    "im:write",
                    "mpim:history",
                    "mpim:read",
                    "reactions:read",
                    "reactions:write",
                    "users:read",
                ],
            },
        },
        "settings": {
            "socket_mode_enabled": true,
            "interactivity": {"is_enabled": true},
            "event_subscriptions": {
                "bot_events": [
                    "app_home_opened",
                    "app_mention",
                    "message.channels",
                    "message.groups",
                    "message.im",
                    "message.mpim",
                ],
            },
            "org_deploy_enabled": false,
            "token_rotation_enabled": false,
        },
    })
}

/// The page that makes the app from [`manifest`], filled in.
pub fn new_app_url(name: &str) -> String {
    reqwest::Url::parse_with_params(
        "https://api.slack.com/apps",
        &[
            ("new_app", "1"),
            ("manifest_json", &manifest(name).to_string()),
        ],
    )
    .map(String::from)
    .unwrap_or_else(|_| "https://api.slack.com/apps".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert!(looks_like_bot_token(concat!(
            "xoxb",
            "-1234567890-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx"
        )));
        assert!(!looks_like_bot_token(concat!(
            "xapp",
            "-1-A0123456789-1234567890-abcdef0123"
        )));
        assert!(!looks_like_bot_token("xoxb-short"));
        assert!(!looks_like_bot_token(concat!(
            "xoxb",
            "-1234567890-12345\n67890123-AbCdEf"
        )));
        assert!(looks_like_app_token(concat!(
            "xapp",
            "-1-A0123456789-1234567890123-abcdef0123456789"
        )));
        assert!(!looks_like_app_token(concat!(
            "xoxb",
            "-1234567890-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx"
        )));
        assert!(looks_like_signing_secret(
            "8f742231b10e8888abcd99eeef0085a5"
        ));
        assert!(!looks_like_signing_secret(
            "8f742231b10e8888abcd99eeef0085a"
        ));
        assert!(!looks_like_signing_secret(
            "8f742231b10e8888abcd99eeef0085z5"
        ));
        assert!(looks_like_config_token(concat!(
            "xoxe.xoxp",
            "-1-Mi0yLTEyMzQ1Njc4OTAtMTIzNDU2Nzg5MA"
        )));
        assert!(!looks_like_config_token(concat!(
            "xoxb",
            "-1234567890-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx"
        )));
        assert!(!looks_like_config_token("xoxe.xoxp-1-a b"));
    }

    #[test]
    fn only_a_person_sending_the_code_is_the_owner() {
        let sent = |m: Value| sent_code(&m, "K7Q2M9XA");
        assert_eq!(
            sent(json!({"user": "U01ABC2DEF3", "text": "k7q2m9xa"})).as_deref(),
            Some("U01ABC2DEF3")
        );
        assert_eq!(sent(json!({"user": "U01ABC2DEF3", "text": "hello"})), None);
        assert_eq!(
            sent(json!({"user": "U0BOT", "bot_id": "B1", "text": "K7Q2M9XA"})),
            None
        );
        assert_eq!(
            sent(json!({"user": "U01", "subtype": "message_changed", "text": "K7Q2M9XA"})),
            None
        );
        assert_eq!(sent(json!({"user": "U01,U02", "text": "K7Q2M9XA"})), None);
    }

    #[test]
    fn a_single_formatted_pairing_code_matches() {
        for text in [
            "*K7Q2M9XA*",
            "**K7Q2M9XA**",
            "`K7Q2M9XA`",
            "```K7Q2M9XA```",
            " \n*k7q2m9xa* \n",
            "```\nK7Q2M9XA\n```",
        ] {
            assert_eq!(
                sent_code(&json!({"user":"U0DANA", "text":text}), "K7Q2M9XA").as_deref(),
                Some("U0DANA"),
                "{text}"
            );
        }
    }

    #[test]
    fn formatting_does_not_accept_extra_codes_or_bypass_sender_checks() {
        for text in [
            "*K7Q2M9XA* and *ABCDEFGH*",
            "**K7Q2M9XA ABCDEFGH**",
            "my code is *K7Q2M9XA*",
            "*K7Q2M9XA**",
            "`K7Q2M9XA",
            "```K7Q2M9XA\nABCDEFGH```",
            "****",
            "*ABCDEFGH*",
        ] {
            assert!(
                sent_code(&json!({"user":"U0DANA", "text":text}), "K7Q2M9XA").is_none(),
                "{text}"
            );
        }
        for extra in [
            json!({"bot_id":"B01"}),
            json!({"subtype":"message_changed"}),
            json!({"user":"U01,U02"}),
        ] {
            let mut message = json!({"user":"U0DANA", "text":"*K7Q2M9XA*"});
            message
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert!(sent_code(&message, "K7Q2M9XA").is_none());
        }
    }

    #[test]
    fn the_manifest_is_socket_mode_with_direct_messages() {
        let manifest = manifest("main");
        assert_eq!(manifest["settings"]["socket_mode_enabled"], true);
        assert_eq!(
            manifest["features"]["app_home"]["messages_tab_enabled"],
            true
        );
        let events = manifest["settings"]["event_subscriptions"]["bot_events"].to_string();
        for event in ["message.im", "app_mention"] {
            assert!(events.contains(event), "{events}");
        }
        assert!(manifest["settings"]["event_subscriptions"]["request_url"].is_null());
        let scopes = manifest["oauth_config"]["scopes"]["bot"].to_string();
        for scope in ["im:history", "im:read", "chat:write", "users:read"] {
            assert!(scopes.contains(scope), "{scope}");
        }
        let url = new_app_url("main");
        assert!(url.starts_with("https://api.slack.com/apps?new_app=1&manifest_json="));
        assert!(!url.contains(' ') && !url.contains('"'), "{url}");
    }
}

#[cfg(test)]
link_test_kit::program!(unit);

/// `lnk-slack`'s command line, generated from its declarations: the
/// connection protocol's commands its contract declares (`actions.toml`),
/// whose shapes are the contract's (`link_connection_contract::declared`).
pub mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));

    /// The connection protocol's command this run is given.
    pub fn operation() -> link_connection_contract::Operation {
        use link_connection_contract::Operation;
        let parsed: link_spec::Parsed<Cli> = PROGRAM.parse();
        match parsed.command.command {
            Command::SlackConnect(given) => Operation::Connect {
                agent: given.agent,
                taken: given.taken,
            },
            Command::Uninstall(given) => Operation::Uninstall(given),
            // Every other command is given nothing: the protocol's, by its
            // words.
            _ => Operation::of(&parsed.words).expect("a command of the connection protocol"),
        }
    }

    /// `lnk-slack`'s whole command line, as clap sees it.
    pub fn command() -> clap::Command {
        <Cli as clap::CommandFactory>::command()
    }
}

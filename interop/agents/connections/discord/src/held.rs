//! Discord's messages on a machine that packs agents
//! (`link_connection_contract::pushed`): Discord pushes them only on its
//! Gateway, so `lnk-discord hold` holds many bots' Gateways in one
//! process, each resumed after a cut, and writes each message it gets.
//! `sift` keeps the owner's, and a member's of a channel the agent is
//! shared with, as the bridge would ([`bridge::carried`]),
//! and `take` answers one as the bridge does, over Discord's HTTP API:
//! never through the holder.

use std::collections::HashMap;

use anyhow::Context;
use link_connection_contract::pushed::{Held, HoldLine, HoldState, Sifting, Taking, Verdict};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::{DIRECT_MESSAGES, Event, GUILD_MESSAGES, bridge, keep};

/// `sift`, for one line: a held update (a `MESSAGE_CREATE`'s data, which
/// carries no secret) from the owner is taken, its thread the bridge's,
/// and a member's mention in a channel `shared` names (as the push
/// comes), with who it's from; anything else is dropped. Its settings are `{"allowed_users",
/// "bot_id"}`: who may talk to the agent, and the bot's user id, for its
/// mentions. A [`link_connection_contract::pushed::Say`] is taken: Discord
/// has no answer in a push's response.
pub fn sift(sifting: &Sifting) -> Verdict {
    if let Some(say) = &sifting.say {
        return Verdict::Take {
            thread: bridge::thread(&say.chat),
            chat: say.chat.clone(),
            dedupe: String::new(),
            from: None,
        };
    }
    let Ok(update) = serde_json::from_str::<Value>(&sifting.body) else {
        return Verdict::drop(400);
    };
    let settings = &sifting.settings;
    let allowed: Vec<String> = settings["allowed_users"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let me = settings["bot_id"].as_str().unwrap_or_default();
    match bridge::carried(&update, &allowed, &sifting.shared, me) {
        Some(message) => Verdict::Take {
            thread: bridge::thread(&message.connection),
            chat: message.connection,
            dedupe: message.id,
            from: message.from.map(|member| member.person),
        },
        None => Verdict::drop(200),
    }
}

/// `take`: answers the update (a `MESSAGE_CREATE`'s data) as the bridge
/// answers a message, over Discord's HTTP API. An update that isn't the
/// owner's, nor a member's of a channel the agent is shared with
/// ([`link_connection_contract::Bridge::shared`]), is refused; an answer
/// Discord refused fails as [`link_connection_contract::ReplyRefused`].
pub async fn take(taking: Taking) -> anyhow::Result<()> {
    let settings: link_harness_contract::harness::Discord = taking
        .bridge
        .settings
        .expose()
        .read()
        .context("reading the Discord settings on stdin")?;
    anyhow::ensure!(
        !settings.bot_id.is_empty(),
        "the Discord settings name no bot_id: connect the bot again"
    );
    let me = settings.bot_id.clone();
    let message = bridge::carried(
        &taking.update,
        &settings.allowed_users,
        &taking.bridge.shared,
        &me,
    )
    .context("that update isn't a message from the owner or a shared channel: not answering it")?;
    let bot = bridge::Bot {
        token: settings.bot_token.duplicate(),
    };
    message.answer(&bot, &taking.bridge).await
}

/// `hold`: holds a bot's Gateway for each `Hold` line, as its key, until
/// a `Drop` line for it, or until `lines` ends, when every hold is let go.
/// Each message one gets goes to `held` as it came, and each time one
/// comes up or goes down, its state. It answers nothing.
pub async fn hold(
    mut lines: mpsc::Receiver<HoldLine>,
    held: mpsc::Sender<Held>,
) -> anyhow::Result<()> {
    // Each key's connection, numbered: what an older one of the same key
    // got after it was let go isn't written.
    let mut holding: HashMap<String, (u64, tokio::task::JoinHandle<()>)> = HashMap::new();
    let (got, mut gets) = mpsc::unbounded_channel::<(u64, Held)>();
    let mut count = 0;
    loop {
        tokio::select! {
            line = lines.recv() => match line {
                None => break,
                Some(HoldLine::Hold { key, settings }) => {
                    count += 1;
                    let task = tokio::spawn(connection(key.clone(), count, settings, got.clone()));
                    if let Some((_, old)) = holding.insert(key, (count, task)) {
                        old.abort();
                    }
                }
                Some(HoldLine::Drop(key)) => {
                    if let Some((_, old)) = holding.remove(&key) {
                        old.abort();
                    }
                }
            },
            Some((number, line)) = gets.recv() => {
                let current = holding.get(&line.key).is_some_and(|(n, _)| *n == number);
                if current && held.send(line).await.is_err() {
                    break;
                }
            }
        }
    }
    for (_, task) in holding.into_values() {
        task.abort();
    }
    Ok(())
}

/// One held bot's Gateway, as `key` (its `number`th hold), with the
/// bridge's intents, for good: what it gets goes to `got`.
async fn connection(
    key: String,
    number: u64,
    settings: link_secretive::Secret<link_secretive::Json>,
    got: mpsc::UnboundedSender<(u64, Held)>,
) {
    let line = |update: Option<Value>, state: Option<HoldState>| {
        let held = Held {
            key: key.clone(),
            update,
            state,
        };
        let _ = got.send((number, held));
    };
    let Some(token) = settings.expose()["bot_token"]
        .as_str()
        .filter(|t| !t.is_empty())
    else {
        link_connection_contract::warn(&format!("Discord ({key}): no bot token in its settings"));
        line(None, Some(HoldState::Down));
        return;
    };
    let name = format!("Discord ({key})");
    let intents = DIRECT_MESSAGES | GUILD_MESSAGES;
    keep(token, intents, &name, |event| match event {
        Event::Up(_) => line(None, Some(HoldState::Up)),
        Event::Down => line(None, Some(HoldState::Down)),
        Event::Refused => line(None, Some(HoldState::Refused)),
        Event::Message(message) => line(Some(message.clone()), None),
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_connection_contract::pushed::Say;
    use serde_json::json;

    const BOT: &str = "123456789012345678";
    const OWNER: &str = "200000000000000002";

    fn sifted(update: Value) -> Verdict {
        sift(&Sifting {
            settings: json!({"allowed_users": [OWNER], "bot_id": BOT}),
            body: update.to_string(),
            ..Default::default()
        })
    }

    fn message(user: &str) -> Value {
        json!({"id": "1300000000000000001", "channel_id": "1200000000000000001",
            "type": 0, "content": "hi", "author": {"id": user, "username": "dana"}})
    }

    #[test]
    fn the_owners_direct_message_is_taken() {
        assert_eq!(
            sifted(message(OWNER)),
            Verdict::Take {
                thread: "discord:1200000000000000001".into(),
                chat: "1200000000000000001".into(),
                dedupe: "1300000000000000001".into(),
                from: None,
            }
        );
    }

    #[test]
    fn a_strangers_message_is_dropped() {
        assert_eq!(sifted(message("999999999999999999")), Verdict::drop(200));
    }

    /// The bot's own message, even with the owner allowed as it: never
    /// answered.
    #[test]
    fn the_bots_own_message_is_dropped() {
        let mut own = message(BOT);
        own["author"]["bot"] = json!(true);
        assert_eq!(sifted(own), Verdict::drop(200));
        let allowed = sift(&Sifting {
            settings: json!({"allowed_users": [BOT], "bot_id": BOT}),
            body: message(BOT).to_string(),
            ..Default::default()
        });
        assert_eq!(allowed, Verdict::drop(200));
    }

    #[test]
    fn the_owners_mention_in_a_server_is_taken() {
        let mut mention = message(OWNER);
        mention["guild_id"] = json!("900000000000000001");
        mention["content"] = json!(format!("<@{BOT}> hi"));
        mention["mentions"] = json!([{"id": BOT, "bot": true}]);
        assert!(matches!(
            sifted(mention.clone()),
            Verdict::Take { ref thread, .. } if thread == "discord:1200000000000000001"
        ));
        // Not mentioning the bot: dropped.
        mention["mentions"] = json!([]);
        assert_eq!(sifted(mention), Verdict::drop(200));
    }

    /// A member's mention is taken, with who it's from, only in a channel
    /// the line says is shared as the push comes.
    #[test]
    fn a_members_mention_is_taken_only_in_a_shared_channel() {
        let mut mention = message("300000000000000003");
        mention["guild_id"] = json!("900000000000000001");
        mention["content"] = json!(format!("<@{BOT}> hi"));
        mention["mentions"] = json!([{"id": BOT, "bot": true}]);
        let sifted = |shared: Vec<String>| {
            sift(&Sifting {
                settings: json!({"allowed_users": [OWNER], "bot_id": BOT}),
                body: mention.to_string(),
                shared,
                ..Default::default()
            })
        };
        assert_eq!(
            sifted(vec!["1200000000000000001".into()]),
            Verdict::Take {
                thread: "discord:1200000000000000001".into(),
                chat: "1200000000000000001".into(),
                dedupe: "1300000000000000001".into(),
                from: Some(
                    link_agent_identity::access::chat_member("discord", "300000000000000003")
                        .unwrap()
                ),
            }
        );
        assert_eq!(sifted(Vec::new()), Verdict::drop(200));
        assert_eq!(
            sifted(vec!["1200000000000000009".into()]),
            Verdict::drop(200)
        );
    }

    #[test]
    fn a_body_not_json_is_dropped_and_a_say_taken() {
        let bad = sift(&Sifting {
            body: "{".into(),
            ..Default::default()
        });
        assert_eq!(bad, Verdict::drop(400));
        let say = sift(&Sifting {
            say: Some(Say {
                chat: "1200000000000000001".into(),
                text: "hi".into(),
            }),
            ..Default::default()
        });
        assert!(matches!(
            say,
            Verdict::Take { ref thread, ref chat, .. }
                if thread == "discord:1200000000000000001" && chat == "1200000000000000001"
        ));
    }
}

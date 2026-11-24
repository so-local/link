//! Slack's messages pushed by the Events API to a machine that packs
//! agents, instead of heard in Socket Mode
//! (`link_connection_contract::pushed`).
//!
//! `hook` switches the app from Socket Mode to a request URL, and
//! `unhook` back, with the app's configuration token, which edits its
//! manifest (`apps.manifest.export`, `apps.manifest.update`); without
//! one, or with one Slack refuses (they last 12 hours), it gives the
//! steps on the app's settings page instead. Both secrets are given in
//! the settings by whatever pushes the agent's messages, since `lnk
//! agent connect slack` asks only for Socket Mode's tokens: `hook`
//! refuses without the signing secret, saying where it is, and uses the
//! configuration token, never keeping it. Slack signs each push with
//! the app's signing secret, which `sift` checks through Link's one way
//! in (`link_secretive::inbound`, Slack's scheme); it answers Slack's
//! check of the address (`url_verification`) and takes only what the
//! bridge would answer ([`crate::bridge::owners`]). `take` answers it as
//! the bridge does.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use link_connection_contract::pushed::{
    self, Hooked, Hooking, Sifting, Taking, Unhooking, Verdict,
};
use link_secretive::inbound::{Inbound, Receiver, Scheme};
use serde_json::{Value, json};

use crate::{bridge, call};

/// Points the app's events at `hooking.url`; what `sift` is handed: who
/// may talk to the agent, the bot's member id and its workspace's id.
pub async fn hook(hooking: Hooking) -> anyhow::Result<Hooked> {
    if !hooking.url.starts_with("https://") {
        bail!("Slack pushes only to an https:// address");
    }
    let settings: &serde_json::Value = hooking.settings.expose();
    if !crate::looks_like_signing_secret(text(settings, "signing_secret")) {
        bail!(
            "no signing secret for the agent's Slack app, which signs what Slack pushes \
             (signing_secret, 32 characters, 0-9 and a-f): it's on the app's page at \
             https://api.slack.com/apps, Basic Information > App Credentials > Signing Secret. \
             Nothing was switched."
        );
    }
    let me = call(text(settings, "bot_token"), "auth.test", &[])
        .await
        .context("checking the bot token")?;
    let steps = switch(settings, Some(&hooking.url)).await;
    Ok(Hooked {
        settings: json!({
            "allowed_users": allowed(settings),
            "bot_user": me["user_id"].as_str().unwrap_or_default(),
            "team": me["team_id"].as_str().unwrap_or_default(),
        }),
        steps,
    })
}

/// Switches the app back to Socket Mode; the steps a person takes
/// instead, when it couldn't.
pub async fn unhook(unhooking: Unhooking) -> anyhow::Result<Vec<String>> {
    Ok(switch(unhooking.settings.expose(), None).await)
}

/// Switches the app to pushing its events to `url`, or back to Socket
/// Mode (none), with its configuration token; the steps on its settings
/// page instead, without one or when Slack refuses it.
async fn switch(settings: &Value, url: Option<&str>) -> Vec<String> {
    let config = text(settings, "config_token");
    if !config.is_empty() {
        match switch_with(settings, config, url).await {
            Ok(()) => return Vec::new(),
            Err(err) => link_connection_contract::warn(&format!(
                "Slack: switching the app with its configuration token: {err:#}"
            )),
        }
    }
    steps(url)
}

/// [`switch`], with the configuration token `config`.
async fn switch_with(settings: &Value, config: &str, url: Option<&str>) -> anyhow::Result<()> {
    let token = text(settings, "bot_token");
    let me = call(token, "auth.test", &[]).await?;
    let bot = me["bot_id"].as_str().context("that token isn't a bot's")?;
    let info = call(token, "bots.info", &[("bot", bot)]).await?;
    let app = info["bot"]["app_id"]
        .as_str()
        .context("Slack didn't say which app the bot is")?;
    let exported = call(config, "apps.manifest.export", &[("app_id", app)]).await?;
    let manifest = switched(exported["manifest"].clone(), url);
    call(
        config,
        "apps.manifest.update",
        &[("app_id", app), ("manifest", &manifest.to_string())],
    )
    .await?;
    Ok(())
}

/// `manifest` pushing its events to `url`, Socket Mode off, or back in
/// Socket Mode with no request URL (none). Its interactivity, when on,
/// follows: a request URL needs one too.
fn switched(mut manifest: Value, url: Option<&str>) -> Value {
    let settings = &mut manifest["settings"];
    settings["socket_mode_enabled"] = json!(url.is_none());
    let interactive = settings["interactivity"]["is_enabled"] == true;
    let mut places = vec!["event_subscriptions"];
    places.extend(interactive.then_some("interactivity"));
    for place in places {
        match url {
            Some(url) => settings[place]["request_url"] = json!(url),
            None => {
                if let Some(place) = settings[place].as_object_mut() {
                    place.remove("request_url");
                }
            }
        }
    }
    manifest
}

/// What a person does on the app's settings page to switch it: to
/// `url`, or back to Socket Mode; or the configuration token to give,
/// so it's switched for them.
fn steps(url: Option<&str>) -> Vec<String> {
    let page = "Open your agent's app at https://api.slack.com/apps, then:";
    let mut steps = match url {
        Some(url) => vec![
            page.into(),
            "1. Socket Mode: turn Enable Socket Mode off.".into(),
            format!("2. Event Subscriptions: set the Request URL to {url}"),
        ],
        None => vec![
            page.into(),
            "1. Socket Mode: turn Enable Socket Mode on.".into(),
        ],
    };
    steps.push(
        "Or give an app configuration token (config_token, xoxe.xoxp-...: Your App \
         Configuration Tokens > Generate Token at https://api.slack.com/apps), used for this \
         switch alone and never kept, and Link makes it."
            .into(),
    );
    steps
}

/// What `sift` makes of one line, now.
pub fn sift(sifting: &Sifting) -> Verdict {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    sift_at(sifting, now)
}

/// [`sift`], `now` seconds since 1970.
fn sift_at(sifting: &Sifting, now: u64) -> Verdict {
    // Slack takes no message in a push's response: sent with send.
    if let Some(say) = &sifting.say {
        let Some((channel, in_thread)) = bridge::place(&say.chat) else {
            return Verdict::drop(400);
        };
        return Verdict::Take {
            thread: bridge::thread(channel, in_thread),
            chat: say.chat.clone(),
            dedupe: String::new(),
            from: None,
        };
    }
    // Signed with the signing secret (or, while it rolls, the one
    // before), within 300 s of now, as Slack advises, so a push heard on
    // the way can't be sent again later.
    let inbound = Inbound {
        receiver: Receiver::new("slack").expect("a name"),
        scheme: Scheme::SLACK,
        max_body: pushed::UPDATE_MAX,
        secrets: sifting.secrets(),
    };
    if let Err(refused) = inbound.verify(&sifting.headers, sifting.body.as_bytes(), now) {
        return Verdict::drop(refused.status());
    }
    // Signed, but not an event (an interaction): nothing for the agent.
    let Ok(envelope) = serde_json::from_str::<Value>(&sifting.body) else {
        return Verdict::drop(200);
    };
    match envelope["type"].as_str() {
        Some("url_verification") => match envelope["challenge"].as_str() {
            Some(challenge) => Verdict::json(&json!({"challenge": challenge})),
            None => Verdict::drop(400),
        },
        Some("event_callback") => {
            let settings = &sifting.settings;
            let team = text(settings, "team");
            if !team.is_empty() && envelope["team_id"].as_str() != Some(team) {
                return Verdict::drop(200);
            }
            let Some(dedupe) = envelope["event_id"].as_str().filter(|e| id(e)) else {
                return Verdict::drop(400);
            };
            match bridge::carried(
                &envelope["event"],
                &allowed(settings),
                &sifting.shared,
                text(settings, "bot_user"),
                team,
            ) {
                Some(message) => Verdict::Take {
                    thread: message.conversation(),
                    chat: bridge::at(&message.connection, message.thread.as_deref()),
                    dedupe: dedupe.to_string(),
                    from: message.from,
                },
                None => Verdict::drop(200),
            }
        }
        _ => Verdict::drop(200),
    }
}

/// Answers the pushed event in `taking.update` (the Events API's
/// envelope) as the bridge answers a message it hears, once the answer
/// is sent.
pub async fn take(taking: Taking) -> anyhow::Result<()> {
    bridge::answer_event(&taking.bridge, &taking.update["event"]).await
}

/// `settings`'s `key`, as text; empty if none.
fn text<'a>(settings: &'a Value, key: &str) -> &'a str {
    settings[key].as_str().unwrap_or_default()
}

/// The member ids `settings` allows.
fn allowed(settings: &Value) -> Vec<String> {
    settings["allowed_users"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Whether `text` is an id Slack gives: letters and digits.
fn id(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use link_connection_contract::pushed::Say;

    use super::*;

    const SECRET: &str = "8f742231b10e8888abcd99yyyzzz85a5";
    /// The signing secret before [`SECRET`], while it rolls.
    const BEFORE: &str = "0123456789abcdef0123456789abcdef";
    const NOW: u64 = 1_700_000_000;

    /// The headers Slack sends with `body` at `at`, signed with `secret`.
    fn signed(body: &str, at: u64, secret: &str) -> BTreeMap<String, String> {
        let secret = link_secretive::OpaqueSecret::from_text(secret.to_string().into());
        Scheme::SLACK
            .sign(&secret, at, "", body.as_bytes())
            .expect("Slack signs")
    }

    /// `body` pushed at `at`, signed with `SECRET`.
    fn push(body: &Value, at: u64) -> Sifting {
        let body = body.to_string();
        Sifting {
            settings: json!({"allowed_users": ["U0DANA"], "bot_user": "U0BOT", "team": "T0ACME"}),
            secret: SECRET.to_string().into(),
            headers: signed(&body, at, SECRET),
            body,
            ..Default::default()
        }
    }

    fn event(user: &str, id: &str) -> Value {
        json!({
            "type": "event_callback",
            "team_id": "T0ACME",
            "event_id": id,
            "event": {"type": "message", "user": user, "channel": "D0DANA",
                "channel_type": "im", "ts": "1.2", "text": "hi"},
        })
    }

    fn taken(dedupe: &str) -> Verdict {
        Verdict::Take {
            thread: "slack:D0DANA".into(),
            chat: "D0DANA".into(),
            dedupe: dedupe.into(),
            from: None,
        }
    }

    /// Every case a push is sifted for, each with what `sift` answers: the
    /// signing secret, its roll, its window and the dedupe the keeper
    /// refuses a repeat by, through Link's one way in.
    #[test]
    fn a_table_of_pushes_each_answered() {
        let owners = event("U0DANA", "Ev01");
        let good = || push(&owners, NOW);
        let with = |change: &dyn Fn(&mut Sifting)| {
            let mut sifting = good();
            change(&mut sifting);
            sifting
        };
        let header = |name: &'static str, value: Option<&'static str>| {
            with(&move |s: &mut Sifting| match value {
                Some(value) => {
                    s.headers.insert(name.into(), value.into());
                }
                None => {
                    s.headers.remove(name);
                }
            })
        };
        // Pushed at `at`, signed with `secret`, sifted with `current` and,
        // while it rolls, `previous`.
        let rolled = |at: u64, secret: &str, current: &str, previous: &str| {
            let mut sifting = push(&owners, at);
            sifting.headers = signed(&sifting.body, at, secret);
            sifting.secret = current.to_string().into();
            sifting.previous = previous.to_string().into();
            sifting
        };
        let cases: Vec<(&str, Sifting, u64, Verdict)> = vec![
            ("a good post", good(), NOW, taken("Ev01")),
            // Within five minutes either way.
            (
                "at the window's start",
                push(&owners, NOW - 300),
                NOW,
                taken("Ev01"),
            ),
            (
                "at the window's end",
                push(&owners, NOW + 300),
                NOW,
                taken("Ev01"),
            ),
            // Too old, or from the future: it could have been heard and
            // sent again.
            ("stale", push(&owners, NOW - 301), NOW, Verdict::drop(401)),
            (
                "from the future",
                push(&owners, NOW + 301),
                NOW,
                Verdict::drop(401),
            ),
            // A replay inside the window is a good post again, with the
            // same dedupe: the keeper refuses it by that.
            ("replayed, the same dedupe", good(), NOW + 60, taken("Ev01")),
            (
                "another event, another dedupe",
                push(&event("U0DANA", "Ev02"), NOW),
                NOW,
                taken("Ev02"),
            ),
            (
                "replayed after the window",
                good(),
                NOW + 301,
                Verdict::drop(401),
            ),
            // The signing secret wrong, or none: closed.
            (
                "a wrong secret",
                rolled(NOW, "another", SECRET, ""),
                NOW,
                Verdict::drop(401),
            ),
            (
                "no secret",
                rolled(NOW, SECRET, "", ""),
                NOW,
                Verdict::drop(503),
            ),
            // While it rolls, the one before is taken beside it; once
            // rolled, not.
            (
                "the previous secret, rolling",
                rolled(NOW, BEFORE, SECRET, BEFORE),
                NOW,
                taken("Ev01"),
            ),
            (
                "the new secret, rolling",
                rolled(NOW, SECRET, SECRET, BEFORE),
                NOW,
                taken("Ev01"),
            ),
            (
                "the previous secret, rolled",
                rolled(NOW, BEFORE, SECRET, ""),
                NOW,
                Verdict::drop(401),
            ),
            (
                "a previous secret alone",
                rolled(NOW, BEFORE, "", BEFORE),
                NOW,
                Verdict::drop(503),
            ),
            // A signature changed, or missing, or not hex.
            (
                "unsigned",
                header("x-slack-signature", None),
                NOW,
                Verdict::drop(401),
            ),
            (
                "too short",
                header("x-slack-signature", Some("v0=00")),
                NOW,
                Verdict::drop(401),
            ),
            (
                "another version",
                header("x-slack-signature", Some("v1=abcd")),
                NOW,
                Verdict::drop(401),
            ),
            (
                "not hex",
                header("x-slack-signature", Some("v0=zz")),
                NOW,
                Verdict::drop(401),
            ),
            (
                "odd hex",
                header("x-slack-signature", Some("v0=abc")),
                NOW,
                Verdict::drop(401),
            ),
            // No time, or one that isn't: malformed.
            (
                "untimed",
                header("x-slack-request-timestamp", None),
                NOW,
                Verdict::drop(400),
            ),
            (
                "timed in words",
                header("x-slack-request-timestamp", Some("now")),
                NOW,
                Verdict::drop(400),
            ),
            // The body altered, or the time.
            (
                "the body altered",
                with(&|s: &mut Sifting| s.body = s.body.replace("hi", "ho")),
                NOW,
                Verdict::drop(401),
            ),
            (
                "the time moved",
                with(&|s: &mut Sifting| {
                    s.headers
                        .insert("x-slack-request-timestamp".into(), (NOW + 1).to_string());
                }),
                NOW,
                Verdict::drop(401),
            ),
            // Over the cap, before anything is read.
            (
                "too big",
                with(&|s: &mut Sifting| s.body = "a".repeat(pushed::UPDATE_MAX + 1)),
                NOW,
                Verdict::drop(413),
            ),
        ];
        for (case, sifting, now, verdict) in cases {
            assert_eq!(sift_at(&sifting, now), verdict, "{case}");
        }
    }

    #[test]
    fn slacks_check_of_the_address_is_answered() {
        let check = json!({"type": "url_verification", "token": "x", "challenge": "3eZbrw1aB"});
        assert_eq!(
            sift_at(&push(&check, NOW), NOW),
            Verdict::Answer {
                status: 200,
                content_type: "application/json".into(),
                body: r#"{"challenge":"3eZbrw1aB"}"#.into(),
            }
        );
        // Unsigned, it isn't; nor with no time to check either.
        let mut unsigned = push(&check, NOW);
        unsigned.headers.remove("x-slack-signature");
        assert_eq!(sift_at(&unsigned, NOW), Verdict::drop(401));
        unsigned.headers.clear();
        assert_eq!(sift_at(&unsigned, NOW), Verdict::drop(400));
    }

    #[test]
    fn a_retry_is_taken_with_the_same_dedupe() {
        let first = push(&event("U0DANA", "Ev0RETRY"), NOW);
        let mut again = push(&event("U0DANA", "Ev0RETRY"), NOW + 60);
        again.headers.insert("x-slack-retry-num".into(), "1".into());
        assert_eq!(sift_at(&first, NOW), taken("Ev0RETRY"));
        assert_eq!(sift_at(&again, NOW + 60), taken("Ev0RETRY"));
    }

    #[test]
    fn shared_pushes_carry_the_members_principal() {
        let mention = json!({"type": "event_callback", "team_id": "T0ACME", "event_id": "EvSHARED",
            "event": {"type": "app_mention", "user": "U0ALICE", "user_team": "T0ACME",
                "channel": "C0TEAM", "ts": "1.3", "text": "<@U0BOT> hello"}});
        let mut sifting = push(&mention, NOW);
        assert_eq!(sift_at(&sifting, NOW), Verdict::drop(200));
        sifting.shared = vec!["C0TEAM".into()];
        assert_eq!(
            sift_at(&sifting, NOW),
            Verdict::Take {
                thread: "slack:C0TEAM.1.3".into(),
                chat: "C0TEAM.1.3".into(),
                dedupe: "EvSHARED".into(),
                from: Some(
                    link_agent_identity::access::chat_member("slack", "T0ACME/U0ALICE").unwrap()
                ),
            }
        );
        let mut outsider = mention;
        outsider["event"]["user_team"] = json!("T0OTHER");
        let mut sifting = push(&outsider, NOW);
        sifting.shared = vec!["C0TEAM".into()];
        assert_eq!(sift_at(&sifting, NOW), Verdict::drop(200));
    }

    #[test]
    fn only_what_the_bridge_answers_is_taken() {
        // A stranger, the bot itself, an edit: dropped 200, so Slack
        // doesn't push them again.
        assert_eq!(
            sift_at(&push(&event("U0EVE", "Ev02"), NOW), NOW),
            Verdict::drop(200)
        );
        let mut own = event("U0BOT", "Ev03");
        own["event"]["bot_id"] = json!("B01");
        assert_eq!(sift_at(&push(&own, NOW), NOW), Verdict::drop(200));
        let mut edit = event("U0DANA", "Ev04");
        edit["event"]["subtype"] = json!("message_changed");
        assert_eq!(sift_at(&push(&edit, NOW), NOW), Verdict::drop(200));
        // Another workspace's, an event that's no message, a body that
        // isn't one.
        let mut elsewhere = event("U0DANA", "Ev05");
        elsewhere["team_id"] = json!("T0OTHER");
        assert_eq!(sift_at(&push(&elsewhere, NOW), NOW), Verdict::drop(200));
        let opened = json!({"type": "event_callback", "team_id": "T0ACME", "event_id": "Ev06",
            "event": {"type": "app_home_opened", "user": "U0DANA", "channel": "D0DANA"}});
        assert_eq!(sift_at(&push(&opened, NOW), NOW), Verdict::drop(200));
        let mut form = push(&json!(null), NOW);
        form.body = "payload=%7B%7D".into();
        form.headers = signed(&form.body, NOW, SECRET);
        assert_eq!(sift_at(&form, NOW), Verdict::drop(200));
        // The owner's mention of the app in a channel is taken, its thread
        // the Slack thread the mention starts.
        let mention = json!({"type": "event_callback", "team_id": "T0ACME", "event_id": "Ev07",
            "event": {"type": "app_mention", "user": "U0DANA", "channel": "C0GENERAL",
                "ts": "1.3", "text": "<@U0BOT> hi"}});
        assert_eq!(
            sift_at(&push(&mention, NOW), NOW),
            Verdict::Take {
                thread: "slack:C0GENERAL.1.3".into(),
                chat: "C0GENERAL.1.3".into(),
                dedupe: "Ev07".into(),
                from: None,
            }
        );
    }

    #[test]
    fn a_say_is_taken_to_be_sent() {
        let say = |chat: &str| Sifting {
            say: Some(Say {
                chat: chat.into(),
                text: "hi".into(),
            }),
            ..Sifting::default()
        };
        assert_eq!(
            sift_at(&say("D0DANA"), NOW),
            Verdict::Take {
                thread: "slack:D0DANA".into(),
                chat: "D0DANA".into(),
                dedupe: String::new(),
                from: None,
            }
        );
        assert_eq!(sift_at(&say("../D0"), NOW), Verdict::drop(400));
        // A Slack thread's, sent into it.
        assert_eq!(
            sift_at(&say("C0GENERAL.1.3"), NOW),
            Verdict::Take {
                thread: "slack:C0GENERAL.1.3".into(),
                chat: "C0GENERAL.1.3".into(),
                dedupe: String::new(),
                from: None,
            }
        );
    }

    #[test]
    fn the_manifest_is_switched_and_back() {
        let manifest = crate::manifest("main");
        let url = "https://push.example/h/abc";
        let pushed = switched(manifest.clone(), Some(url));
        assert_eq!(pushed["settings"]["socket_mode_enabled"], false);
        assert_eq!(
            pushed["settings"]["event_subscriptions"]["request_url"],
            url
        );
        assert_eq!(pushed["settings"]["interactivity"]["request_url"], url);
        // The events it gets are the same.
        assert_eq!(
            pushed["settings"]["event_subscriptions"]["bot_events"],
            manifest["settings"]["event_subscriptions"]["bot_events"]
        );
        assert_eq!(switched(pushed, None), manifest);
    }

    #[test]
    fn the_steps_name_the_address() {
        let to = steps(Some("https://push.example/h/abc")).join("\n");
        assert!(to.contains("Socket Mode off"), "{to}");
        assert!(to.contains("https://push.example/h/abc"), "{to}");
        assert!(steps(None).join("\n").contains("Socket Mode on"));
    }
}

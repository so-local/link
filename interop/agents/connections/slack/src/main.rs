//! `lnk-slack connect`: a Slack app of the user's own, made from Link's
//! manifest, answering only whoever sends its bot the one-time code in a
//! direct message (`link_connection_contract`); `lnk-slack bridge`: its
//! messages carried for a harness that speaks no Slack; `lnk-slack hook`,
//! `unhook`, `sift` and `take`: its messages pushed by the Events API
//! instead (`link_connection_contract::pushed`).

use std::time::{Duration, SystemTime};

use link_connection_contract::{self as connection, Bot, Connected, Request};
use link_format::{
    Address, Exit, Foreign, Name, Note, Out, Pairing, Question, Report, Text, Verbatim, WithinExt,
    cmd, phrase, prose, sentence,
};
use link_slack as slack;

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    connection::main_with_bridge(
        about(),
        link_slack::cli::operation(),
        connect,
        slack::bridge::carry,
    )
}

/// What `lnk-slack describe` says of Slack. Its tokens go over a
/// connection it holds open, which the proxy can't read: no `rule`. A
/// signing secret and configuration token are for its pushed messages,
/// given by whatever pushes them (`hook`), never asked at `connect`.
fn about() -> connection::About {
    connection::About {
        name: "slack",
        describe: connection::Describe {
            title: "Slack".into(),
            secret_fields: vec![
                "bot_token".into(),
                "app_token".into(),
                "signing_secret".into(),
                "config_token".into(),
            ],
            hosts: vec!["slack.com".into(), "*.slack.com".into()],
            disconnect_hint: "Your Slack app stays yours: delete it at https://api.slack.com/apps."
                .into(),
            ..Default::default()
        },
        rule: None,
        send: Some(|bridge, chat, text| Box::pin(link_slack::bridge::send(bridge, chat, text))),
        pushed: Some(connection::pushed::Pushed {
            hook: Some(|hooking| Box::pin(slack::pushed::hook(hooking))),
            unhook: Some(|unhooking| {
                Box::pin(async move {
                    let steps = slack::pushed::unhook(unhooking).await?;
                    connection::steps_aside(&steps);
                    Ok(())
                })
            }),
            sift: slack::pushed::sift,
            take: |taking| Box::pin(slack::pushed::take(taking)),
            hold: None,
        }),
        share: Some(|sharing| Box::pin(slack::bridge::share(sharing))),
        owner: Some(slack::bridge::owner),
    }
}

/// Only what Socket Mode needs: the bot token and the app-level token.
/// What only pushing its messages needs (the signing secret, the
/// configuration token) is given to `hook` by whatever pushes them, so
/// it's never kept on a machine that doesn't.
async fn connect(request: Request, out: &mut Out) -> Result<Connected, Report> {
    // Read, or asked for, into Secrets from the start.
    let (bot_token, app_token) = match (
        link_secretive::Secret::from_env("LNK_SLACK_BOT_TOKEN"),
        link_secretive::Secret::from_env("LNK_SLACK_APP_TOKEN"),
    ) {
        (Some(bot), Some(app)) => (bot, app),
        (bot, app) => {
            let url = slack::new_app_url(&request.agent);
            let manifest = slack::manifest(&request.agent);
            let apps = Address::new("https://api.slack.com/apps");
            out.now(Text(prose!(
                "First, make your agent's Slack app: open {}, pick your workspace and press \
                 Next, then Create (Link filled in what it needs).",
                Address::new(&url)
            )));
            out.now(
                Verbatim::new()
                    .lead(phrase!("If the page asks for a manifest, paste this one"))
                    .line(manifest.to_string()),
            );
            out.now(Text(prose!(
                "Made one on an earlier try? Don't make another: open it at {} and go on from \
                 the second step.",
                apps.clone()
            )));
            out.now(Text(prose!(
                "Second, on Basic Information, under App-Level Tokens, press Generate Token and \
                 Scopes, add the scope {}, generate, and paste the token ({}) below.",
                Foreign::new("connections:write"),
                Foreign::new("xapp-...")
            )));
            out.now(Text(prose!(
                "Third, on Install App, press Install to Workspace and Allow, and paste the Bot \
                 User OAuth Token ({}) below.",
                Foreign::new("xoxb-...")
            )));
            connection::open(&url);
            let app = match app {
                Some(app) => app,
                None => out.ask(
                    Question::hidden(sentence!("What is the app-level token?"))
                        .variable("LNK_SLACK_APP_TOKEN"),
                )?,
            };
            let bot = match bot {
                Some(bot) => bot,
                None => out.ask(
                    Question::hidden(sentence!("What is the bot token?"))
                        .variable("LNK_SLACK_BOT_TOKEN"),
                )?,
            };
            (bot, app)
        }
    };
    let (bot_token, app_token) = (
        bot_token.expose_map(|t| t.trim().to_string()),
        app_token.expose_map(|t| t.trim().to_string()),
    );
    let (bot_token, app_token) = (bot_token.expose(), app_token.expose());
    let again = || cmd!("lnk agent connect slack");
    if !slack::looks_like_bot_token(bot_token) {
        return Err(
            Report::error(sentence!("That doesn't look like a bot token."))
                .why(prose!("A bot token starts with {}.", Foreign::new("xoxb-")))
                .exit(Exit::Usage)
                .next(
                    again(),
                    phrase!("connect again with the Bot User OAuth Token"),
                ),
        );
    }
    if !slack::looks_like_app_token(app_token) {
        return Err(
            Report::error(sentence!("That doesn't look like an app-level token."))
                .why(prose!(
                    "An app-level token starts with {}.",
                    Foreign::new("xapp-")
                ))
                .exit(Exit::Usage)
                .next(again(), phrase!("connect again with the app-level token")),
        );
    }
    let (bot, mut pairing) = {
        let _checking = out.activity(phrase!("Checking the tokens with Slack"));
        let bot = slack::bot(bot_token).await.map_err(|err| {
            connection::failed(
                "slack",
                sentence!("Slack didn't accept the bot token."),
                err,
            )
        })?;
        let pairing = slack::Pairing::open(app_token).await.map_err(|err| {
            connection::failed(
                "slack",
                sentence!("Slack didn't accept the app-level token."),
                err,
            )
        })?;
        (bot, pairing)
    };
    // Slack shares a Socket Mode app's messages among whatever connects:
    // each agent needs its own app.
    connection::check_taken(
        &bot.id,
        &request.taken,
        "lnk agent connect slack shows how to make another app",
    )?;
    // Only someone who can see this terminal can claim the app.
    let code = connection::pairing_code().within("making the pairing code")?;
    let wait = Duration::from_secs(600);
    out.now(Pairing::new(
        sentence!(
            "Now, so {} knows it's you, open it in Slack ({}), and in its Messages tab send it \
             this code.",
            Name::new("bot", &bot.name),
            Name::new("workspace", &bot.team)
        ),
        &code,
        SystemTime::now() + wait,
    ));
    let waiting = out.activity(phrase!("Waiting for your code in Slack"));
    let (owner, name, dm_channel) = pairing
        .owner(bot_token, &code, wait, |progress| {
            out.now(Note(link_format::Sentence::adopt(progress.text())));
        })
        .await
        .map_err(|err| connection::failed("slack", sentence!("The app wasn't paired."), err))?;
    drop(waiting);
    out.block(Note(sentence!(
        "Got it, {} (Slack member {}). Only you can talk to {}.",
        Name::new("person", &name),
        Name::new("member", &owner),
        Name::new("bot", &bot.name)
    )));
    let settings = serde_json::json!({
        "bot_token": bot_token,
        "app_token": app_token,
        "allowed_users": [owner],
        "dm_channel": dm_channel,
    });
    Ok(Connected {
        settings: settings.into(),
        bot: Bot {
            id: bot.id,
            name: bot.name,
            team: Some(bot.team),
        },
    })
}

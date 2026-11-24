//! `lnk-discord connect`: a Discord bot of the user's own, added to a
//! server of theirs, answering only whoever sends it the one-time code in
//! a direct message (`link_connection_contract`). `lnk-discord bridge` carries
//! the messages for a harness that speaks no Discord, and `hold`, `sift`
//! and `take` for a machine that packs agents.

use std::time::{Duration, SystemTime};

use link_connection_contract::{self as connection, Bot, Connected, Request};
use link_discord as discord;
use link_format::{
    Address, Exit, Name, Note, Out, Pairing, Question, Report, Text, WithinExt, cmd, phrase, prose,
    sentence,
};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    connection::main_with_bridge(
        about(),
        link_discord::cli::operation(),
        connect,
        discord::bridge::carry,
    )
}

/// What `lnk-discord describe` says of Discord. Its tokens go over a
/// connection it holds open, which the proxy can't read: no `rule`. On a
/// machine that packs agents, `hold` holds that connection for many bots
/// at once, and `sift` and `take` read and answer what it gets. `share`
/// takes the owner's share code in a server's channel.
fn about() -> connection::About {
    connection::About {
        name: "discord",
        describe: connection::Describe {
            title: "Discord".into(),
            secret_fields: vec!["bot_token".into()],
            hosts: vec!["discord.com".into(), "*.discord.gg".into()],
            disconnect_hint: "Your Discord bot stays yours: delete its application at https://discord.com/developers/applications.".into(),
            ..Default::default()
        },
        rule: None,
        send: Some(|bridge, chat, text| {
            Box::pin(link_discord::bridge::send(bridge, chat, text))
        }),
        pushed: Some(connection::pushed::Pushed {
            hook: None,
            unhook: None,
            sift: discord::held::sift,
            take: |taking| Box::pin(discord::held::take(taking)),
            hold: Some(|lines, held| Box::pin(discord::held::hold(lines, held))),
        }),
        share: Some(|sharing| Box::pin(discord::share(sharing))),
        owner: Some(link_discord::bridge::owner),
    }
}

async fn connect(request: Request, out: &mut Out) -> Result<Connected, Report> {
    // Read, or asked for, into a Secret from the start.
    let token = match link_secretive::Secret::from_env("LNK_DISCORD_TOKEN") {
        Some(token) => token,
        None => {
            let url = "https://discord.com/developers/applications";
            out.now(Text(prose!(
                "First, open {}, press New Application and name it (say, {}).",
                Address::new(url),
                Name::new("agent", &request.agent)
            )));
            out.now(Text(prose!(
                "On its Bot page, turn on Message Content Intent under Privileged Gateway \
                 Intents and save, then press Reset Token and paste the token below."
            )));
            connection::open(url);
            out.ask(
                Question::hidden(sentence!("What is the bot token?")).variable("LNK_DISCORD_TOKEN"),
            )?
        }
    };
    let token = token.expose_map(|t| t.trim().to_string());
    let token = token.expose();
    if !discord::looks_like_token(token) {
        return Err(
            Report::error(sentence!("That doesn't look like a bot token."))
                .why(prose!("A bot token has three parts, with dots between."))
                .exit(Exit::Usage)
                .next(
                    cmd!("lnk agent connect discord"),
                    phrase!("connect again with the token from the application's Bot page"),
                ),
        );
    }
    let bot = {
        let _checking = out.activity(phrase!("Checking the token with Discord"));
        discord::bot(token).await
    }
    .map_err(|err| {
        connection::failed(
            "discord",
            sentence!("Discord didn't accept the token."),
            err,
        )
    })?;
    if !bot.message_content {
        return Err(Report::error(sentence!("The bot can't read messages yet."))
            .why(prose!(
                "Message Content Intent is off, and the harness reads messages' text: turn it on \
                 at {} under Privileged Gateway Intents, and save.",
                Address::new(format!(
                    "https://discord.com/developers/applications/{}/bot",
                    bot.application
                ))
            ))
            .exit(Exit::Usage)
            .next(
                cmd!("lnk agent connect discord"),
                phrase!("connect again once it is on"),
            ));
    }
    // Discord gives every connection of a bot its messages: each agent
    // needs its own, or two would answer.
    connection::check_taken(
        &bot.id,
        &request.taken,
        "another application at https://discord.com/developers/applications",
    )?;
    let invite = discord::invite_url(&bot.application);
    out.now(Text(prose!(
        "Add {} to a server of yours, since you can only message a bot you share a server with: \
         open {}, pick the server and authorize.",
        Name::new("bot", &bot.name),
        Address::new(&invite)
    )));
    connection::open(&invite);
    // Only someone who can see this terminal can claim the bot.
    let code = connection::pairing_code().within("making the pairing code")?;
    let bot_name = bot.name.clone();
    let wait = Duration::from_secs(600);
    let (owner, name, dm_channel) = discord::owner(token, &code, wait, || {
        out.now(Pairing::new(
            sentence!(
                "Now, so {} knows it's you, send it this code in a direct message (its name in \
                 the server's member list, then Message).",
                Name::new("bot", &bot_name)
            ),
            &code,
            SystemTime::now() + wait,
        ));
    })
    .await
    .map_err(|err| connection::failed("discord", sentence!("The bot wasn't paired."), err))?;
    out.block(Note(sentence!(
        "Got it, {} (Discord user {}). Only you can talk to {}.",
        Name::new("person", &name),
        Name::new("user", &owner),
        Name::new("bot", &bot.name)
    )));
    Ok(Connected {
        settings: serde_json::json!({
            "bot_token": token,
            "bot_id": bot.id,
            "allowed_users": [owner],
            "dm_channel": dm_channel,
        })
        .into(),
        bot: Bot {
            id: bot.id,
            name: bot.name,
            team: None,
        },
    })
}

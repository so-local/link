//! `lnk-telegram connect`: the agent's own Telegram bot, answering only
//! whoever sends it the one-time code (`link_connection_contract`); `lnk-telegram
//! rule --json`: how a harness speaking Telegram reaches it without its
//! token; `lnk-telegram bridge`: its messages carried for a harness that
//! speaks no Telegram.

use std::time::{Duration, SystemTime};

use link_connection_contract::{self as connection, Bot, Connected, Request};
use link_format::{
    Address, Exit, Foreign, Name, Note, Out, Pairing, Question, Report, Sentence, Text, WithinExt,
    cmd, phrase, prose, sentence,
};
use link_telegram as telegram;

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    connection::main_with_bridge(
        telegram::about(),
        link_telegram::cli::operation(),
        connect,
        telegram::bridge::carry,
    )
}

async fn connect(request: Request, out: &mut Out) -> Result<Connected, Report> {
    // Read, or asked for, into a Secret from the start.
    let token = match link_secretive::Secret::from_env("LNK_TELEGRAM_TOKEN") {
        Some(token) => token,
        None => {
            let botfather = "https://t.me/BotFather";
            out.now(Text(prose!(
                "First, open {} in Telegram and send {}.",
                Address::new(botfather),
                Foreign::new("/newbot")
            )));
            out.now(Text(prose!(
                "Then pick a name and a username for your agent's bot."
            )));
            out.now(Text(prose!(
                "Last, paste the token BotFather gives you ({}) below.",
                Foreign::new("123456:ABC-...")
            )));
            connection::open(botfather);
            out.ask(
                Question::hidden(sentence!("What is the bot token?"))
                    .variable("LNK_TELEGRAM_TOKEN"),
            )?
        }
    };
    let token = token.expose_map(|t| t.trim().to_string());
    let token = token.expose();
    let again = || cmd!("lnk agent connect telegram");
    if !telegram::looks_like_token(token) {
        return Err(
            Report::error(sentence!("That doesn't look like a bot token."))
                .why(prose!(
                    "A bot token looks like {}.",
                    Foreign::new("123456:ABC-...")
                ))
                .exit(Exit::Usage)
                .next(
                    again(),
                    phrase!("connect again with the token BotFather gave"),
                ),
        );
    }
    // Only one program may read a bot's messages at a time: each agent
    // needs its own bot. Its id is the token's first part, so this needs
    // no Telegram.
    let id = telegram::bot_id(token).map(|id| id.to_string());
    if let Some(id) = &id {
        connection::check_taken(id, &request.taken, "BotFather's /newbot makes another")?;
    }
    let (bot, elsewhere) = {
        let _checking = out.activity(phrase!("Checking the token with Telegram"));
        let bot = telegram::bot_name(token).await.map_err(|err| {
            connection::failed(
                "telegram",
                sentence!("Telegram didn't accept the token."),
                err,
            )
        })?;
        // The agent reads this bot only once it's connected, and is stopped
        // when it already was: anything reading now is another program.
        let elsewhere = telegram::polled_elsewhere(token).await.map_err(|err| {
            connection::failed(
                "telegram",
                sentence!("Telegram wouldn't say who reads the bot."),
                err,
            )
        })?;
        (bot, elsewhere)
    };
    let bot_name = Name::new("bot", format!("@{bot}"));
    match elsewhere {
        None => {}
        Some(telegram::Elsewhere::Hooked) => {
            return Err(Report::error(sentence!(
                "The messages of {} are pushed to a webhook.",
                bot_name
            ))
            .why(Sentence::adopt(telegram::HOOKED))
            .exit(Exit::Usage)
            .next(
                cmd!("lnk agent move"),
                phrase!("bring the agent back to this machine"),
            ));
        }
        Some(telegram::Elsewhere::Polled) => {
            return Err(Report::error(sentence!(
                "Another program reads the messages of {}.",
                bot_name
            ))
            .why(prose!(
                "It is an agent on another machine, or a bot you run elsewhere: stop it first, or \
                 make this agent a bot of its own with BotFather's /newbot."
            ))
            .exit(Exit::Usage)
            .next(again(), phrase!("connect again once it is stopped")));
        }
    }
    // Only someone who can see this terminal can claim the bot.
    let code = connection::pairing_code().within("making the pairing code")?;
    let wait = Duration::from_secs(600);
    out.now(Text(prose!(
        "Now, so {} knows it's you, open {} and press Start.",
        bot_name.clone(),
        Address::new(format!("https://t.me/{bot}?start={code}"))
    )));
    out.now(Pairing::new(
        sentence!("Or send {} this code in a private chat.", bot_name.clone()),
        &code,
        SystemTime::now() + wait,
    ));
    let (owner, name) = telegram::owner(token, &code, wait)
        .await
        .map_err(|err| connection::failed("telegram", sentence!("The bot wasn't paired."), err))?;
    out.block(Note(sentence!(
        "Got it, {} (Telegram user {}). Only you can talk to {}.",
        Name::new("person", &name),
        Name::new("user", owner.to_string()),
        bot_name
    )));
    Ok(Connected {
        settings: serde_json::json!({"bot_token": token, "allowed_users": [owner]}).into(),
        bot: Bot {
            id: id.unwrap_or_default(),
            name: bot,
            team: None,
        },
    })
}

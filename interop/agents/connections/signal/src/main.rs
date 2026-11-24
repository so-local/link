//! `lnk-signal connect`: the agent's Signal number and its owner's
//! (`link_connection_contract`). The harness then links the agent's number as
//! a device, through `signal-cli` (`lnk-signal cli`), from a QR code
//! on the terminal; there is no token to ask for. The owner's number is
//! typed at the terminal, so whoever sees it decides, and no one-time code
//! is needed. `lnk-signal link` and `lnk-signal bridge`: Link links it and
//! carries its messages for a harness that speaks no Signal; `lnk-signal
//! state` says what of its folder a move carries.

use link_connection_contract::{self as connection, Bot, Connected, Request, normalize_number};
use link_format::{
    Exit, Foreign, Name, Note, Out, Question, Report, Text, cmd, phrase, prose, sentence,
};
use link_signal::bridge;

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    let operation = match link_signal::cli::given() {
        link_signal::cli::Given::Native(native) => return link_signal::native::main(native),
        link_signal::cli::Given::Connection(operation) => operation,
    };
    connection::main_linked(
        about(),
        operation,
        connect,
        bridge::carry,
        bridge::link,
        link_signal::state(),
    )
}

/// What `lnk-signal describe` says of Signal: linked as a device, with
/// no secret in its settings.
fn about() -> connection::About {
    connection::About {
        name: "signal",
        describe: connection::Describe {
            title: "Signal".into(),
            cli: true,
            hosts: vec!["*.signal.org".into()],
            disconnect_hint:
                "Unlink your agent on the phone with its Signal: Settings > Linked Devices.".into(),
            ..Default::default()
        },
        rule: None,
        send: Some(|bridge, chat, text| Box::pin(bridge::send(bridge, chat, text))),
        pushed: None,
        share: None,
        owner: Some(bridge::owner),
    }
}

async fn connect(request: Request, out: &mut Out) -> Result<Connected, Report> {
    let again = || cmd!("lnk agent connect signal");
    // Read, or asked for, whichever the variable says.
    let asked = std::env::var("LNK_SIGNAL_NUMBER").is_err();
    if asked {
        out.now(Text(prose!(
            "Signal links your agent to a number as a device, as Signal Desktop does: the phone \
             with that number stays its main device. Give the agent a number of its own (a \
             second SIM, a prepaid one): a harness ignores the messages of the account it runs \
             on, so it can't be yours."
        )));
    }
    let agents = out.ask(
        Question::text(sentence!(
            "What is the agent's Signal number, with its country code (+1 555 ...)?"
        ))
        .variable("LNK_SIGNAL_NUMBER"),
    )?;
    let Some(agents) = normalize_number(&agents) else {
        return Err(not_a_number(&agents, again()));
    };
    connection::check_taken(
        &agents,
        &request.taken,
        "a number of its own, or disconnect Signal there",
    )?;
    let owner = out.ask(
        Question::text(sentence!(
            "What is your own number, the one you'll text it from?"
        ))
        .variable("LNK_SIGNAL_OWNER"),
    )?;
    let Some(owner) = normalize_number(&owner) else {
        return Err(not_a_number(&owner, again()));
    };
    if owner == agents {
        return Err(
            Report::error(sentence!("The agent needs a Signal number of its own."))
                .why(prose!(
                    "A harness ignores the messages of the account it runs on."
                ))
                .exit(Exit::Usage)
                .next(again(), phrase!("connect again with another number")),
        );
    }
    out.block(Note(sentence!(
        "Only {} can talk to the agent at {}.",
        Name::new("number", &owner),
        Name::new("number", &agents)
    )));
    Ok(Connected {
        settings: serde_json::json!({"account": agents, "allowed_users": [owner]}).into(),
        bot: Bot {
            id: agents.clone(),
            name: agents,
            team: None,
        },
    })
}

/// What was typed isn't a number.
fn not_a_number(typed: &str, again: link_format::Command) -> Report {
    Report::error(sentence!("{} isn't a phone number.", Foreign::new(typed)))
        .why(prose!("A number has its country code, as +1 555 123 4567."))
        .exit(Exit::Usage)
        .next(again, phrase!("connect again with a number"))
}

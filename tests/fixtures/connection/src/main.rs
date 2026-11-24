//! A connection module against a fake bot service. [`main`] gives the
//! connection contract its description and three operations: [`connect`]
//! pairs the owner, [`carry`] polls allowed users and forwards their messages
//! to the harness, and [`send`] delivers an unsolicited message to the owner.
//! [`answer`] shows the round trip from a received message to a model answer.
//!
//! The shared contract owns command dispatch, stdin shutdown and per-thread
//! serialization. This fixture adds no groups, streaming or pushed messages,
//! and is never shipped.
//!
//! Its service is a fake the test runs, at `LNK_CONNECTION_FIXTURE_SERVICE`
//! (`http://127.0.0.1:<port>`): a bot API, each call `POST
//! <service>/bot<token>/<method>` with a JSON body, answered with JSON.
//! It holds several bots, each its own by its token:
//!
//! ```text
//! me     {}                     the bot: {"id", "name"}
//! poll   {"confirm": <id>}      its messages from <id> on, those before it
//!                               confirmed, never handed out again; waits a
//!                               moment for one when there are none:
//!                               {"messages": [{"id", "from", "text"}]}
//! send   {"chat", "text"}       text to chat (a person, as `from` names
//!                               them): {"id"}
//! ```
//!
//! A bridge stopped before its next poll confirmed what it was handed
//! leaves it for the next, as a real connection keeps what a bot hasn't
//! confirmed. Its settings: `{"token": "<bot id>:<secret>",
//! "allowed_users": ["<person>"]}`; a thread is `connection-fixture:<person>`.

use std::time::Duration;

use anyhow::Context;
use link_connection_contract::{self as connection, Bot, Bridge, Connected, Request};
use link_format::{Out, Report};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// The connection's name: `lnk-connection-fixture`, threads `connection-fixture:<person>`.
const NAME: &str = "connection-fixture";

/// The host a harness speaking it would reach, for its rule: no service
/// is there, since only a test's fake is.
const HOST: &str = "connection-fixture.example";

/// Its section of the agent's settings.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    #[serde(with = "link_secretive::bare")]
    token: link_secretive::Secret,
    allowed_users: Vec<String>,
}

impl Settings {
    fn of(bridge: &Bridge) -> anyhow::Result<Self> {
        bridge
            .settings
            .expose()
            .read()
            .context("reading the connection fixture's settings on stdin")
    }
}

/// A message the service handed out.
#[derive(Deserialize)]
struct Message {
    id: u64,
    from: String,
    text: String,
}

fn main() -> std::process::ExitCode {
    link_secretive::process::guard();
    connection::main_with_bridge(
        about(),
        link_connection_fixture::cli::operation(),
        connect,
        carry,
    )
}

fn about() -> connection::About {
    use link_secret_contract::route::{SecretPath, SecretRest, SecretRule};
    connection::About {
        name: NAME,
        describe: connection::Describe {
            title: "Mini connection".into(),
            secret_fields: vec!["token".into()],
            hosts: vec![HOST.into()],
            disconnect_hint: "Its bot stays its fake service's.".into(),
            ..Default::default()
        },
        // A harness speaking it is given a placeholder in `token`, and its
        // proxy as `api_root`, which adds the token where the path holds
        // it. The bot's id, before the `:`, isn't secret.
        rule: Some(connection::Rule {
            field: "token".into(),
            root: "api_root".into(),
            wire: SecretRule {
                host: HOST.into(),
                paths: vec![SecretPath {
                    prefix: "bot{secret}/".into(),
                    rest: SecretRest::Name,
                }],
                public: Some(":".into()),
                ..Default::default()
            },
        }),
        send: Some(|bridge, chat, text| Box::pin(send(bridge, chat, text))),
        pushed: None,
        share: None,
        owner: None,
    }
}

/// Where its service is: `LNK_CONNECTION_FIXTURE_SERVICE`, which every test sets.
fn service() -> anyhow::Result<String> {
    std::env::var("LNK_CONNECTION_FIXTURE_SERVICE").context(
        "LNK_CONNECTION_FIXTURE_SERVICE isn't set: the connection fixture's service is a test's fake",
    )
}

/// The service's answer to `method`, for the bot whose token is `token`.
async fn call(token: &link_secretive::Secret, method: &str, body: Value) -> anyhow::Result<Value> {
    static CLIENT: std::sync::OnceLock<link_fetch::Client> = std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        link_fetch::Client::builder()
            .timeout(Duration::from_secs(30))
            .outside()
            .build()
            .expect("a client by Link's rules is made")
    });
    let service = service()?;
    let url = token.expose_map(|token| format!("{service}/bot{token}/{method}"));
    let shown = format!("the connection fixture's {method}");
    Ok(client
        .secret_url(link_fetch::Method::POST, &url, &shown)
        .json(&body)
        .answer::<Value>()
        .await?)
}

/// The messages from `confirm` on, those before it confirmed.
async fn poll(token: &link_secretive::Secret, confirm: u64) -> anyhow::Result<Vec<Message>> {
    let polled = call(token, "poll", json!({"confirm": confirm})).await?;
    Ok(serde_json::from_value(polled["messages"].clone())?)
}

/// The bot's id, its token's first part, which isn't secret.
fn bot_id(token: &link_secretive::Secret) -> anyhow::Result<String> {
    let id = token.expose().split_once(':').map(|(id, _)| id.to_string());
    id.filter(|id| !id.is_empty())
        .context("a connection fixture token is <bot id>:<secret>")
}

/// The bot in `LNK_CONNECTION_FIXTURE_TOKEN`, refused when another agent has it,
/// paired with whoever sends it the code printed here.
async fn connect(request: Request, out: &mut Out) -> Result<Connected, Report> {
    let token = link_secretive::Secret::from_env("LNK_CONNECTION_FIXTURE_TOKEN")
        .context("LNK_CONNECTION_FIXTURE_TOKEN isn't set: the bot's token")?;
    let id = bot_id(&token)?;
    connection::check_taken(&id, &request.taken, "its fake service makes another")?;
    Ok(pair(&token, id, out).await?)
}

async fn pair(
    token: &link_secretive::Secret,
    id: String,
    out: &mut Out,
) -> anyhow::Result<Connected> {
    let me = call(token, "me", json!({})).await?;
    let code = connection::pairing_code()?;
    out.now(link_format::Text(link_format::prose!(
        "Send the bot this code: {}.",
        link_format::Foreign::new(&code)
    )));
    let mut confirm = 0;
    let pairing = async {
        loop {
            for message in poll(token, confirm).await? {
                confirm = confirm.max(message.id + 1);
                if connection::is_code(&message.text, &code) {
                    return anyhow::Ok(message.from);
                }
            }
        }
    };
    let owner = tokio::time::timeout(Duration::from_secs(60), pairing)
        .await
        .context("nobody sent the bot the code in a minute")??;
    // The code confirmed, so the agent's bridge never sees it.
    poll(token, confirm).await?;
    out.block(link_format::Note(link_format::Sentence::adopt(&format!(
        "Got it, {owner}"
    ))));
    Ok(Connected {
        settings: json!({"token": token.expose(), "allowed_users": [owner]}).into(),
        bot: Bot {
            id,
            name: me["name"].as_str().unwrap_or_default().to_string(),
            team: None,
        },
    })
}

/// Carries messages until stdin closes: each from an allowed user asked
/// of the harness, its answer sent back.
#[expect(clippy::disallowed_methods, reason = "backs off before trying again")]
async fn carry(bridge: Bridge) -> anyhow::Result<()> {
    let settings = Settings::of(&bridge)?;
    eprintln!("Carrying {}'s connection fixture messages", bridge.agent);
    let (queue, messages) = mpsc::unbounded_channel::<Message>();
    let answering = connection::per_thread(
        messages,
        |message| format!("{NAME}:{}", message.from),
        |message| {
            let (bridge, token) = (&bridge, &settings.token);
            async move { answer(bridge, token, message).await }
        },
    );
    let polling = async {
        let mut confirm = 0;
        loop {
            match poll(&settings.token, confirm).await {
                Ok(messages) => {
                    for message in messages {
                        confirm = confirm.max(message.id + 1);
                        if settings.allowed_users.contains(&message.from) {
                            let _ = queue.send(message);
                        }
                    }
                }
                Err(err) => {
                    eprintln!("the connection fixture: {err:#}; trying again");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    };
    tokio::select! {
        () = polling => {}
        () = answering => {}
    }
    Ok(())
}

/// Asks the harness `message`, and sends its answer back whole.
async fn answer(bridge: &Bridge, token: &link_secretive::Secret, message: Message) {
    let thread = format!("{NAME}:{}", message.from);
    let (progress, _) = tokio::sync::watch::channel(String::new());
    let answer = match connection::ask_for(bridge, &thread, &message.text, &progress).await {
        // Joined a turn already running: its answer answers this too.
        Ok(connection::Asked::Steered) => return,
        Ok(connection::Asked::Answer(answer)) if answer.trim().is_empty() => {
            "(Your agent answered nothing.)".to_string()
        }
        Ok(connection::Asked::Answer(answer)) => answer,
        Err(err) => connection::couldnt_answer(&bridge.agent, &err),
    };
    let sent = json!({"chat": message.from, "text": answer});
    if let Err(err) = call(token, "send", sent).await {
        eprintln!("the connection fixture: {err:#}");
    }
}

/// Sends `text` to `chat`, a message nobody asked for: only to an allowed
/// user's own chat.
async fn send(
    bridge: Bridge,
    chat: String,
    text: String,
) -> anyhow::Result<link_connection_contract::SentMessages> {
    let settings = Settings::of(&bridge)?;
    anyhow::ensure!(
        settings.allowed_users.contains(&chat),
        "connection fixture chat {} isn't an allowed user's own: not sending there",
        link_format::clean(&chat)
    );
    call(&settings.token, "send", json!({"chat": chat, "text": text})).await?;
    Ok(link_connection_contract::SentMessages::default())
}

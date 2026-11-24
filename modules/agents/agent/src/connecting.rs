//! An agent's connections: connected, shared and disconnected, and its
//! webhooks.

use std::{io::Write, path::PathBuf};

use link_agent::cli::{self};
use link_agent::{
    adapter::{Adapter, Boxed},
    bridge, connections, service,
    settings::{self, Settings},
};
use link_format::{
    Address, Done, Exit, Foreign, Name, Note, Out, Product, Question, Report, Sentence, Table,
    Value, Verbatim, WithinExt, cmd, prose, sentence,
};

use crate::confine::{connection_programs, connection_rules, hand_grants, link_on};
use crate::granting::{restart_agent, restart_with};
use crate::launching::{StoppedUnlessStarted, up, up_after};
use crate::listing::gather;
use crate::permissions::{YES, answer_needs};
use crate::shares;

/// `lnk agent webhook`: a webhook created, listed, rotated or deleted. A
/// secret is shown once, as it's made, and never again; the agent
/// restarts when it takes posts at another hook from now on.
pub(crate) async fn webhook(
    out: &mut Out,
    agent: &mut Settings,
    command: cli::Command,
) -> Result<Done, Report> {
    use link_agent::webhooks;
    let now = webhooks::now();
    webhooks::prune(agent, now);
    let who = Name::new("agent", &agent.name);
    match command {
        cli::Command::WebhookCreate(cli::WebhookCreateArgs {
            name,
            sender,
            questions_per_day,
            ..
        }) => {
            let sender = match sender {
                Some(cli::WebhookCreateArgsSender::Github) => webhooks::Sender::Github,
                Some(cli::WebhookCreateArgsSender::Link) => webhooks::Sender::Link,
                None => webhooks::Sender::default(),
            };
            let questions_per_day = questions_per_day
                .map(u32::try_from)
                .transpose()
                .within("reading --questions-per-day")?;
            let made = webhooks::add(agent, &name, sender, webhooks::check_says_who)?;
            if let Some(per_day) = questions_per_day {
                webhooks::questions_per_day(agent, &name, per_day)?;
            }
            let hook = Name::new("webhook", &name);
            show_made(out, &name, &made);
            out.block(Note(sentence!("Reached on this machine at its port.")));
            restart_agent(out, agent).await?;
            Ok(
                Done::success(sentence!("Added webhook {} to {}.", &hook, &who))
                    .next(cmd!("lnk agent webhook list"), "see them"),
            )
        }
        cli::Command::WebhookList(cli::WebhookListArgs { json, .. }) => {
            let listed = webhooks::list(agent, now);
            if json {
                // One line each, and nothing else on stdout.
                for webhook in &listed {
                    link_actions::answer::line::<link_agent_spec::declared::Webhook>(out, webhook)?;
                }
                return Ok(Done::shown());
            }
            let mut table = Table::new(
                ["Name", "From", "Address", "Rolling"],
                sentence!("No webhooks yet."),
            );
            for webhook in &listed {
                table.row([
                    Name::new("webhook", &webhook.name).into(),
                    Value::from(webhook.from.name().to_string()),
                    Address::new(webhook.address.clone()).into(),
                    match webhook.previous_until {
                        Some(_) => Value::from("its previous secret is still taken"),
                        None => Value::from(""),
                    },
                ]);
            }
            out.block(table);
            Ok(Done::shown().next(cmd!("lnk agent webhook create <name>"), "add one"))
        }
        cli::Command::WebhookRotate(cli::WebhookRotateArgs { name, done, .. }) => {
            let hook = Name::new("webhook", &name);
            match webhooks::roll(agent, &name, done, now)? {
                None => {
                    let address = webhooks::list(agent, now)
                        .into_iter()
                        .find(|webhook| webhook.name == name)
                        .map(|webhook| webhook.address)
                        .unwrap_or_default();
                    out.reply(&link_agent_spec::declared::WebhookSecret {
                        name,
                        address,
                        secret: None,
                    });
                    Ok(Done::success(sentence!(
                        "Webhook {} takes only its new secret from now on.",
                        &hook
                    )))
                }
                Some(made) => {
                    show_made(out, &name, &made);
                    Ok(Done::success(sentence!("Webhook {} has a new secret.", &hook))
                        .next(
                            cmd!("lnk agent webhook rotate {} --done", &hook),
                            "once its sender has the new one: its previous secret is taken for 24 hours more",
                        ))
                }
            }
        }
        cli::Command::WebhookDelete(cli::WebhookDeleteArgs { name, .. }) => {
            webhooks::remove(agent, &name)?;
            restart_agent(out, agent).await?;
            Ok(Done::success(sentence!(
                "Removed webhook {} from {}.",
                Name::new("webhook", &name),
                &who
            )))
        }
        _ => unreachable!("only a webhook's commands"),
    }
}

/// A webhook's address and its secret, shown this once, and its declared
/// output, `webhook-secret`, answered when JSON is asked for.
fn show_made(out: &mut Out, name: &str, made: &link_agent::webhooks::Made) {
    out.reply(&link_agent_spec::declared::WebhookSecret {
        name: name.to_string(),
        address: made.address.clone(),
        secret: Some(made.secret.duplicate()),
    });
    out.now(
        Verbatim::new()
            .line(format!("Address: {}", made.address))
            // Shown once, here, and never logged or shown again.
            .line(format!("Secret:  {}", made.secret.expose_owned())),
    );
    out.block(
        Report::warning(sentence!("The secret is shown only now."))
            .why(prose!("Keep it where its sender signs with it.")),
    );
}

/// Refuses connection `connection` when the agent's harness doesn't speak it.
pub(crate) fn speaks(settings: &Settings, connection: &str) -> Result<(), Report> {
    let harness = settings.harness();
    let adapter = Adapter::find_or_add(harness)?;
    let speaking = adapter.speaks(settings);
    if speaking.own.iter().any(|c| c == connection) {
        return Ok(());
    }
    // Link carries it if its module does: the module says, so it's
    // installed first.
    if speaking.endpoint {
        connection_module(connection)?;
        if speaking.speaks(settings, connection) {
            return Ok(());
        }
    }
    let others: Vec<_> = settings::harnesses()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name != harness)
        .filter(|name| {
            Adapter::find(name).is_ok_and(|a| a.speaks(settings).speaks(settings, connection))
        })
        .collect();
    let title = match settings.connections.get(connection) {
        Some(_) => settings.title(connection),
        None => connections::describe(connection)
            .map(|d| link_format::clean(&d.title))
            .unwrap_or_else(|_| link_format::clean(connection)),
    };
    let mut report = Report::error(sentence!(
        "{} doesn't speak {}.",
        Name::new("harness", harness),
        Product::new(&title)
    ));
    if let [one, ..] = others.as_slice() {
        report = report.next(
            cmd!("lnk agent harness set {}", Name::new("harness", one)),
            "switch to one that does",
        );
    }
    Err(report)
}

/// `lnk agent disconnect <connection>`.
pub(crate) async fn disconnect(
    out: &mut Out,
    settings: &mut Settings,
    connection: &str,
) -> Result<Done, Report> {
    let title: Value = Product::new(settings.title(connection)).into();
    let gone = settings
        .connections
        .get(connection)
        .map(|c| link_format::clean(&c.about.disconnect_hint))
        .unwrap_or_default();
    if !settings.connections.contains_key(connection)
        && !connections::is_module(&link_core::modules::list(), connection)
    {
        return Err(Report::error(sentence!(
            "{} isn't a connection.",
            Name::new("connection", connection)
        ))
        .why(prose!("No module Link lists is a connection by that name."))
        .next(cmd!("lnk module list"), "see the connection modules")
        .exit(Exit::Usage));
    }
    if !settings.disconnect(connection) {
        out.block(Note(sentence!("{} isn't connected.", &title)));
        return Ok(Done::shown());
    }
    settings.save()?;
    bridge::forget(&settings.name, connection);
    if service::running(&settings.name) {
        settings.save()?;
        let name = settings.harness().to_string();
        up(out, settings, &name, false).await?;
    }
    let mut done = Done::success(sentence!("{} is disconnected.", &title));
    if !gone.is_empty() {
        out.block(Note(Sentence::adopt(&gone)));
    }
    done = done.next(
        cmd!("lnk agent connect {}", connection.to_string()),
        "connect it again",
    );
    Ok(done)
}

/// `lnk agent connect <connection>`: the connection's module (`lnk-<connection>`)
/// makes and pairs it (`link_connection_contract`), refusing a bot another
/// agent has. An agent that already talks on the connection is stopped
/// meanwhile, since it could answer the code itself, as is one whose
/// harness links the connection as a device, and started again with it;
/// any other keeps answering its other ways while the code waits, and
/// restarts with the connection once it's paired.
pub(crate) async fn connect(
    out: &mut Out,
    settings: &mut Settings,
    connection: &str,
) -> Result<Done, Report> {
    let agent = Name::new("agent", &settings.name);
    // Connected already: connecting again makes another bot or app, which
    // is rarely what's meant (an earlier try that worked).
    if let Some(connected) = settings.connections.get(connection) {
        let title: Value = Product::new(settings.title(connection)).into();
        let bot = &connected.bot;
        let as_bot = match (bot.name.is_empty(), &bot.team) {
            (true, _) => Value::from(""),
            (false, Some(team)) => Value::from(format!(" as {} ({team})", bot.name)),
            (false, None) => Value::from(format!(" as {}", bot.name)),
        };
        out.block(Note(sentence!(
            "{} already talks on {}{}.",
            &agent,
            &title,
            as_bot
        )));
        let again = may_ask()
            && out.ask(Question::confirm(
                sentence!("Connect {} again, from the start?", &title),
                false,
            ))?;
        if !again {
            out.block(Note(sentence!("Nothing changed.")));
            return Ok(Done::shown().next(cmd!("lnk agent show"), "see it").next(
                cmd!("lnk agent disconnect {}", connection.to_string()),
                "forget it",
            ));
        }
    }
    let taken = taken(settings, connection)?;
    let program = connection_module(connection)?;
    let about = connections::describe(connection)?;
    let title: Value = Product::new(link_format::clean(&about.title)).into();
    let was_running = service::running(&settings.name);
    // It stops while you pair only when it reads the connection already,
    // as it could answer the code itself, or when its harness links the
    // connection as a device: asked first, so a running agent isn't
    // stopped and started unawares.
    let pausing = was_running && (settings.connections.contains_key(connection) || about.linked);
    if pausing
        && !YES.load(std::sync::atomic::Ordering::Relaxed)
        && may_ask()
        && !out.ask(Question::confirm(
            sentence!(
                "{} stops while you connect {}, and starts again after. Go on?",
                &agent,
                &title
            ),
            true,
        ))?
    {
        out.block(Note(sentence!("Nothing changed.")));
        return Ok(Done::shown().next(cmd!("lnk agent show"), "see it"));
    }
    // Ctrl-C or a closed terminal ends the connection's setup, not this:
    // caught here (a handler, which the setup doesn't inherit), so a
    // stopped agent is started again below.
    #[cfg(unix)]
    let _caught = {
        use tokio::signal::unix::{SignalKind, signal};
        (
            signal(SignalKind::interrupt()).within("catching Ctrl-C")?,
            signal(SignalKind::hangup()).within("catching a hangup")?,
        )
    };
    // Stopped, and systemd reloaded once: by `up` starting it again, or
    // here when nothing does (an `up` that fails before its start too).
    if pausing {
        service::stop_before_start(&settings.name)?;
    }
    let mut stopped = StoppedUnlessStarted {
        agent: settings.name.clone(),
        armed: pausing,
    };
    // The connection's module asks its questions on this terminal.
    out.hand_over();
    let made = link_secretive::process::ours(&program)
        .args(
            link_connection_contract::declared::ConnectionConnectArgs {
                agent: settings.name.clone(),
                taken: serde_json::to_string(&taken).within("telling it the bots taken")?,
                json: false,
            }
            .argv(),
        )
        .stdin(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .output()
        .within(format!("running {}", program.display()))
        .map_err(Report::from)
        .and_then(|output| {
            if !output.status.success() {
                return Err(
                    Report::error(sentence!("Connecting {} didn't finish.", &title))
                        .why(prose!("Its own words are above.")),
                );
            }
            serde_json::from_slice::<link_connection_contract::Connected>(&output.stdout)
                .within(format!("reading what lnk-{connection} connect made"))
                .map_err(Report::from)
        });
    let connected = match made {
        Ok(connected) => connected,
        Err(err) => {
            if pausing {
                let harness = settings.harness().to_string();
                up_after(out, settings, &harness, &mut stopped).await?;
            }
            return Err(err);
        }
    };
    settings.connect(connection, connected, about)?;
    settings.save()?;
    // A connection the harness links itself (Signal) is linked anew now;
    // without a model yet, when `lnk agent start` first starts it.
    if settings.connections[connection].about.linked && settings.model.is_some() {
        let name = settings.harness().to_string();
        let adapter = Adapter::find_or_add(&name)?;
        // Link carries it for a harness that doesn't speak it: Link links it.
        let linked = if adapter.connections().iter().any(|c| c == connection) {
            // What it needs with the connection, answered before it's set up
            // with it, as a start would.
            let needs = answer_needs(out, settings, &name, &adapter)?;
            let boxed = Boxed::new(settings, &name)?;
            adapter.install(
                &boxed,
                &needs,
                YES.load(std::sync::atomic::Ordering::Relaxed),
            )?;
            out.block(Note(sentence!(
                "Setting up {}.",
                Name::new("harness", &name)
            )));
            // Handed what `up` hands it next, so its configure, the same,
            // is skipped.
            let mut handed = settings.for_harness(&connection_rules(settings)?)?;
            hand_grants(&mut handed, settings, &name, boxed.sandbox);
            connection_programs(&adapter, &boxed, settings, &mut handed)?;
            adapter.configure(&boxed, &handed)?;
            link_on(&adapter, &boxed, settings, connection, true)?
        } else {
            bridge::link(settings, connection, true)?
        };
        if !linked {
            return Err(Report::error(sentence!("{} links from a QR code.", &title))
                .why(prose!("It needs a terminal."))
                .next(
                    cmd!("lnk agent connect {}", connection.to_string()),
                    "run it in a terminal",
                ));
        }
    }
    // Started again only as it ran: one in a terminal restarts there, and
    // one stopped stays so.
    let mut done = Done::success(sentence!("{} talks on {} now.", &agent, &title));
    if pausing {
        let name = settings.harness().to_string();
        up_after(out, settings, &name, &mut stopped).await?;
    } else if was_running {
        restart_with(out, settings).await?;
    } else if service::foreground_running(&settings.name) {
        out.block(Report::warning(sentence!(
            "{} runs in a terminal, so it talks on {} once you start it again there.",
            &agent,
            &title
        )));
    } else {
        done = done.next(cmd!("lnk agent start"), "start your agent");
    }
    Ok(done)
}

/// Whether a question can be asked here: a terminal in and out.
fn may_ask() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && link_format::streams::stderr_is_terminal()
}

/// `lnk agent share add <connection>`: a chat of the connection shared with the
/// agent, by a one-time code its owner mentions the bot with there, taken
/// by the connection's module (`lnk-<connection> share`) while the agent is
/// stopped, as pairing is; then written into its spec's `[access]
/// message` and the agent started again, its bridge answering whoever is
/// in the chat. Refused for a connection the agent hasn't, one whose module
/// can't share, one its harness speaks itself, a harness that can't tell
/// who's speaking, and a spec that says `groups = "never"`.
pub(crate) async fn share(
    out: &mut Out,
    settings: &mut Settings,
    connection: &str,
    json: bool,
) -> Result<Done, Report> {
    let agent = Name::new("agent", &settings.name);
    let Some(connected) = settings.connections.get(connection) else {
        return Err(Report::error(sentence!(
            "{} has no {}.",
            &agent,
            Name::new("connection", connection)
        ))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk agent connect {}", connection.to_string()),
            "connect it first",
        ));
    };
    let title: Value = Product::new(settings.title(connection)).into();
    let bot = link_format::clean(&connected.bot.name);
    let about = connections::describe(connection)?;
    if !about.share {
        return Err(
            Report::error(sentence!("A chat on {} can't be shared.", &title))
                .why(prose!("Its module can't take a share's code."))
                .exit(Exit::Usage),
        );
    }
    let harness = settings.harness().to_string();
    let adapter = Adapter::find_or_add(&harness)?;
    if adapter.speaks(settings).own.iter().any(|c| c == connection) {
        return Err(Report::error(sentence!(
            "{} speaks {} itself.",
            Name::new("harness", &harness),
            &title
        ))
        .why(prose!(
            "Link can't tell who's speaking there: only an agent whose connection Link carries can be shared."
        ))
        .exit(Exit::Usage));
    }
    if !adapter.contract()?.from {
        return Err(Report::error(sentence!(
            "{} can't tell who's speaking.",
            Name::new("harness", &harness)
        ))
        .why(prose!("A shared chat's messages would pass for yours."))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk agent harness set link"),
            "switch to Link Harness, which can",
        ));
    }
    if settings.spec()?.connection(connection).groups == Some(link_agent_spec::Groups::Never) {
        return Err(
            Report::error(sentence!("Its spec answers no chat on {}.", &title))
                .why(prose!(
                    "It says groups = \"never\" for the connection, so it answers no chat, shared or not."
                ))
                .exit(Exit::Usage),
        );
    }
    // Shown to its owner, then held as a secret.
    let shown = link_connection_contract::pairing_code()?;
    let wait = link_connection_contract::SHARE_WAIT;
    let named = match bot.is_empty() {
        true => "the bot".to_string(),
        false => format!("@{bot}"),
    };
    let how = sentence!(
        "In the {} chat to share, mention {} with this code. Whoever is in it can then make {} do all it can.",
        &title,
        Foreign::new(&named),
        &agent
    );
    let pairing = link_format::Pairing::new(
        how,
        format!("{named} {shown}"),
        std::time::SystemTime::now() + wait,
    );
    out.now(pairing);
    let code = link_secretive::Secret::from(shown);
    let program = connection_module(connection)?;
    let was_running = service::running(&settings.name);
    #[cfg(unix)]
    let _caught = {
        use tokio::signal::unix::{SignalKind, signal};
        (
            signal(SignalKind::interrupt()).within("catching Ctrl-C")?,
            signal(SignalKind::hangup()).within("catching a hangup")?,
        )
    };
    // Stopped meanwhile, as pairing stops it: its bridge would otherwise
    // read the same messages.
    if was_running {
        service::stop_before_start(&settings.name)?;
    }
    let mut stopped = StoppedUnlessStarted {
        agent: settings.name.clone(),
        armed: was_running,
    };
    let sharing = link_connection_contract::Sharing {
        settings: settings.connections[connection].settings.duplicate(),
        code,
        wait_secs: wait.as_secs(),
    };
    out.hand_over();
    let taken = run_share(&program, &sharing)
        .and_then(|shared| shares::add(settings, connection, &shared).map_err(Report::from));
    if was_running {
        up_after(out, settings, &harness, &mut stopped).await?;
    }
    let share = taken?;
    in_terminal(out, settings);
    if json {
        link_actions::answer::reply::<link_agent_spec::declared::Share>(out, &share)?;
    }
    Ok(Done::success(sentence!(
        "{} is shared with {} on {}.",
        &agent,
        Foreign::new(&share.said()),
        &title
    ))
    .next(
        cmd!(
            "lnk agent share remove {} --connection {}",
            link_format::clean(&share.chat),
            connection.to_string()
        ),
        "end it",
    ))
}

/// Runs `lnk-<connection> share` (`program`) with `sharing` on its stdin:
/// the chat it shared.
pub(crate) fn run_share(
    program: &std::path::Path,
    sharing: &link_connection_contract::Sharing,
) -> Result<link_connection_contract::Shared, Report> {
    let mut child = link_secretive::process::ours(program)
        .arg("share")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .within(format!("running {}", program.display()))?;
    {
        let mut stdin = child.stdin.take().within("handing it the share")?;
        let line = link_secretive::Secret::from(
            serde_json::to_string(sharing).within("writing the share")?,
        );
        stdin
            .write_all(line.expose().as_bytes())
            .within("handing it the share")?;
        stdin.write_all(b"\n").within("handing it the share")?;
    }
    let output = child.wait_with_output().within("waiting for the share")?;
    if !output.status.success() {
        return Err(Report::error(sentence!("No chat was shared."))
            .why(prose!("Its own words are above."))
            .next(
                cmd!("lnk agent share add <connection>"),
                "run it again for a new code",
            ));
    }
    Ok(serde_json::from_slice(&output.stdout).within("reading the chat it shared")?)
}

/// `lnk agent share list`: the chats the agent is shared with.
pub(crate) fn list_shares(out: &mut Out, settings: &Settings, json: bool) -> Result<Done, Report> {
    let shares = shares::list(&settings.name)?;
    if json {
        link_actions::answer::reply::<Vec<link_agent_spec::declared::Share>>(out, &shares)?;
    }
    let mut table = Table::new(
        ["Connection", "Chat"],
        sentence!(
            "{} is shared with no chat: only you can talk to it.",
            Name::new("agent", &settings.name)
        ),
    );
    for share in &shares {
        table.row([
            Value::from(settings.title(&share.connection)),
            Value::from(share.said()),
        ]);
    }
    out.block(table);
    Ok(match shares.is_empty() {
        true => Done::shown().next(
            cmd!("lnk agent share add <connection>"),
            "share a chat with it",
        ),
        false => Done::shown().next(
            cmd!("lnk agent share remove <chat> --connection <connection>"),
            "end one",
        ),
    })
}

/// `lnk agent share remove <chat> --connection <connection>`: the chat, by its id or its name,
/// no longer shared, and the agent started again if it runs, so its
/// bridge answers nobody there but its owner.
pub(crate) async fn unshare(
    out: &mut Out,
    settings: &mut Settings,
    connection: &str,
    chat: &str,
    json: bool,
) -> Result<Done, Report> {
    let title: Value = Product::new(settings.title(connection)).into();
    let agent = Name::new("agent", &settings.name);
    let Some(share) = shares::remove(settings, connection, chat)? else {
        return Err(Report::error(sentence!(
            "{} isn't shared with {} on {}.",
            &agent,
            Foreign::new(chat),
            &title
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent share list"), "see what it is shared with"));
    };
    if service::running(&settings.name) {
        let harness = settings.harness().to_string();
        up(out, settings, &harness, false).await?;
    }
    in_terminal(out, settings);
    if json {
        link_actions::answer::reply::<link_agent_spec::declared::Share>(out, &share)?;
    }
    Ok(Done::success(sentence!(
        "{} is no longer shared with {} on {}.",
        &agent,
        Foreign::new(&share.said()),
        &title
    )))
}

/// Says, for an agent running in a terminal, that it keeps the shares it
/// started with until it starts again there: only its terminal restarts
/// it, and its tools' standing and hosts are set as it starts.
fn in_terminal(out: &mut Out, settings: &Settings) {
    if service::foreground_running(&settings.name) {
        out.block(Report::warning(sentence!(
            "{} runs in a terminal, so it keeps the chats it was shared with until you start it again there.",
            Name::new("agent", &settings.name)
        )));
    }
}

/// The bots of `connection` other agents have: this machine's, and those
/// running on your boxes (a stopped copy, the one a move leaves, reads
/// nothing).
pub(crate) fn taken(
    settings: &Settings,
    connection: &str,
) -> anyhow::Result<Vec<link_connection_contract::Taken>> {
    let mut taken = Vec::new();
    for other in settings::names()?.iter().filter(|n| **n != settings.name) {
        let other = Settings::load_without_keys(other)?;
        if let Some(id) = other.bot_id(connection) {
            taken.push(link_connection_contract::Taken {
                id,
                agent: other.name.clone(),
                machine: None,
            });
        }
    }
    // Every box, those made elsewhere too: one of them may run the bot.
    if let Ok(crate::listing::Gathered { agents: all, .. }) = gather(false, true) {
        let here = link_machine::installation::installation()
            .ok()
            .map(|m| m.id);
        for agent in all
            .iter()
            .filter(|a| crate::listing::machine_of(a) != here && a["running"] == true)
        {
            let Some(id) = connections::listed_bot_id(agent, connection) else {
                continue;
            };
            taken.push(link_connection_contract::Taken {
                id,
                agent: link_format::clean(agent["name"].as_str().unwrap_or("?")),
                machine: Some(link_format::clean(
                    agent["machine"]["name"].as_str().unwrap_or("a box"),
                )),
            });
        }
    }
    Ok(taken)
}

/// The connection's module, `lnk-<connection>`, installed first (`lnk module
/// add`) if it's missing: `lnk agent connect slack` just works.
pub(crate) fn connection_module(connection: &str) -> anyhow::Result<PathBuf> {
    link_core::modules::add(connection, &format!("Connecting {connection}"))
}

/// Connect a connection on the account hosting this agent, without local settings.
pub(crate) fn connect_hosted(
    out: &mut Out,
    found: crate::moving::Found,
    connection: &str,
) -> Result<Done, Report> {
    if connection != "telegram" {
        return Err(
            Report::error(sentence!("This hosted connect supports Telegram."))
                .why(prose!("Connect other connections in the app."))
                .exit(Exit::Usage),
        );
    }
    let account_name = found.at.within("the hosted agent names no account")?;
    let account =
        link_agent_place::account(&account_name).within("the hosting account isn't connected")?;
    let agent_id = found
        .id
        .as_deref()
        .within("the hosted agent has no stable ID")?;
    link_machine::machine::check_id(agent_id)?;
    let token = out.ask(
        Question::hidden(sentence!("What is the Telegram bot token?"))
            .variable("LNK_TELEGRAM_TOKEN"),
    )?;
    let user = out.ask(
        Question::text(sentence!("What is your Telegram user ID?")).variable("LNK_TELEGRAM_USER"),
    )?;
    let reply = link_agent::hosted::call(&account, "agent-connect", |request| {
        request.agent = Some(link_agent_place::HostedAgent {
            id: agent_id.into(),
            name: found.name.clone(),
            ..Default::default()
        });
        request.connect = Some(link_agent_place::Connect {
            connection: "telegram".into(),
            token,
            user: Some(user),
        });
    })?;
    link_agent::hosted::unless_refused(&account_name, &reply)?;
    let message = reply
        .message
        .within("the hosting account gave no connection result")?;
    Ok(Done::success(Sentence::adopt(&message)))
}

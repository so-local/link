//! An agent's conversations: its instructions, a chat in this
//! terminal, its history moved, and skills added.

use std::{
    io::{BufRead, Write},
    path::PathBuf,
};

use anyhow::ensure;
use link_agent::cli::{self};
use link_agent::{
    adapter::{Adapter, Boxed},
    bridge, moved, service,
    settings::Settings,
};
use link_format::{
    Done, Exit, Foreign, Name, Note, Out, Path, Report, Sentence, Value, WithinExt, cmd, prose,
    sentence,
};

use crate::memory;

/// What the harness printed, as the command's output: with `--json`, the
/// document it printed, replied as one; else the bytes, as they are.
fn passed_on(out: &mut Out, printed: &[u8]) -> Result<(), Report> {
    match out.is_json() {
        true => out.reply(
            &serde_json::from_slice::<serde_json::Value>(printed)
                .within("reading what the harness printed")?,
        ),
        false => out.data(printed),
    }
    Ok(())
}

/// `lnk agent show --instructions`: the prompt its harness sends its model now,
/// as the harness prints it (`lnk-<harness> instructions`), from what its
/// last start configured and the files as they are.
pub(crate) fn instructions(out: &mut Out, settings: &Settings, json: bool) -> Result<Done, Report> {
    let name = settings.harness();
    let adapter = Adapter::find(name)?;
    if !adapter.contract()?.prompt {
        return Err(Report::error(sentence!(
            "{} can't print the prompt it sends its model.",
            Name::new("harness", name)
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent harness set link"), "Link Harness can"));
    }
    let boxed = Boxed::new(settings, name)?;
    let printed = adapter.instructions(&boxed, json)?;
    // Written by the agent and the skills it was given: to a terminal,
    // each line without what could send escapes or hide text.
    let printed = match json {
        true => printed,
        false => String::from_utf8_lossy(&printed)
            .split('\n')
            .map(|line| {
                line.split('\t')
                    .map(link_format::clean)
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes(),
    };
    passed_on(out, &printed)?;
    Ok(Done::shown())
}

/// `lnk agent chat show <thread>`: the conversation as a person reads
/// it, as its harness prints it from its events, wherever they're kept
/// (`lnk-<harness> conversation <thread>`).
pub(crate) fn conversation(
    out: &mut Out,
    settings: &Settings,
    thread: &str,
    json: bool,
) -> Result<Done, Report> {
    if json {
        return conversation_command(out, settings, &["conversation", thread, "--json"], false);
    }
    let (adapter, boxed) = conversations_harness(settings)?;
    let drivers = store_drivers(settings)?;
    let printed = adapter.conversations(&boxed, &["conversation", thread], false, &drivers)?;
    out.data(&printed);
    Ok(Done::shown())
}

/// The typed history and fork contract, using the same confinement and
/// store drivers as the printed conversation.
pub(crate) fn conversation_command(
    out: &mut Out,
    settings: &Settings,
    args: &[&str],
    write: bool,
) -> Result<Done, Report> {
    let (adapter, boxed) = conversations_harness(settings)?;
    let harness = Name::new("harness", settings.harness());
    if !adapter.contract()?.conversation_history {
        return Err(Report::error(sentence!(
            "{} can't list typed conversation history.",
            harness
        ))
        .exit(Exit::Usage));
    }
    if write && !adapter.contract()?.conversation_fork {
        return Err(
            Report::error(sentence!("{} can't fork conversations.", harness)).exit(Exit::Usage),
        );
    }
    let drivers = store_drivers(settings)?;
    let printed = adapter.conversations(&boxed, args, write, &drivers)?;
    passed_on(out, &printed)?;
    Ok(Done::shown())
}

/// What an agent's chat can't reach, said as its harness's own connections.
fn own_connections(settings: &Settings) -> Report {
    Report::error(sentence!(
        "{} speaks its own connections, and has no endpoint to send to.",
        Name::new("harness", settings.harness())
    ))
    .why(prose!(
        "Talk to {} on a connection it speaks, or run it on Link Harness.",
        Name::new("agent", &settings.name)
    ))
    .exit(Exit::Usage)
    .next(cmd!("lnk agent connect telegram"), "talk to it there")
    .next(cmd!("lnk agent harness set link"), "run it on Link Harness")
}

/// `lnk agent chat send`: a conversation with the agent in this terminal,
/// or, with `--message` or no terminal, one message for a script
/// (`link_agent::chat`).
pub(crate) async fn chat(
    out: &mut Out,
    settings: &Settings,
    args: &cli::Chat,
) -> Result<Done, Report> {
    use link_agent::chat::Chat;
    let thread = args.thread()?;
    let agent = Name::new("agent", &settings.name);
    // A harness with no endpoint is refused whether or not it runs: one
    // running has a plan, its endpoint empty.
    let Ok(plan) = bridge::Plan::load(&settings.name) else {
        if Adapter::find(settings.harness())?
            .endpoint(settings.port)
            .is_none()
        {
            return Err(own_connections(settings));
        }
        return Err(
            Report::error(sentence!("{} hasn't started here yet.", agent))
                .exit(Exit::Usage)
                .next(cmd!("lnk agent start"), "start it"),
        );
    };
    if plan.endpoint.is_empty() {
        return Err(own_connections(settings));
    }
    if bridge::paused(&settings.name) {
        return Err(Report::error(sentence!(
            "{} is paused, and takes no message until it's resumed.",
            agent
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent resume"), "resume it"));
    }
    if !(service::running_anywhere(&settings.name) || link_agent::idle::asleep(&settings.key())) {
        return Err(Report::error(sentence!("{} is stopped.", agent))
            .exit(Exit::Usage)
            .next(cmd!("lnk agent start"), "start it"));
    }
    if args.existing {
        has_conversation(settings, &thread)?;
    }
    talk(
        out,
        Chat::new(settings, &plan, thread),
        &settings.name,
        args,
    )
    .await
}

/// Refused unless agent `settings` already has conversation `thread`, as
/// its harness lists them (`lnk agent chat list`). A harness that keeps
/// no typed history can't list them, so a chat with it isn't refused.
fn has_conversation(settings: &Settings, thread: &str) -> Result<(), Report> {
    let Ok((adapter, boxed)) = conversations_harness(settings) else {
        return Ok(());
    };
    if !adapter.contract()?.conversation_history {
        return Ok(());
    }
    let drivers = store_drivers(settings)?;
    let printed = adapter.conversations(
        &boxed,
        &["conversations", "list", "--json"],
        false,
        &drivers,
    )?;
    let listed: link_api::types::RemoteConversations =
        serde_json::from_slice(&printed).within("reading the conversations its harness listed")?;
    if listed
        .threads
        .iter()
        .any(|listed| listed.as_str() == thread)
    {
        return Ok(());
    }
    Err(Report::error(sentence!(
        "{} has no chat {}.",
        Name::new("agent", &settings.name),
        Name::new("chat", thread)
    ))
    .exit(Exit::Usage)
    .next(cmd!("lnk agent chat list"), "list its chats")
    .next(cmd!("lnk agent chat start"), "start a fresh one"))
}

/// `lnk agent chat send` of agent `note.name`, hosted by the cloud account its
/// note names, through that account's adapter (`agent-chat`); or when it
/// may be sent again, refused for now ([`talk_or_later`]).
pub(crate) async fn hosted_chat(
    out: &mut Out,
    note: &moved::Note,
    args: &cli::Chat,
) -> Result<Result<Done, link_agent_place::Later>, Report> {
    use link_agent::chat::Chat;
    let thread = args.thread()?;
    let name = &note.name;
    let place = note.place();
    let account = link_agent_place::account(&note.to.name).within(format!(
        "{name} runs on {place}, which isn't a cloud account here: lnk cloud list"
    ))?;
    let id = note
        .id
        .clone()
        .within(format!("the note of where {name} went has no ID"))?;
    let agent = link_agent_place::HostedAgent {
        id,
        name: name.clone(),
        ..link_agent_place::HostedAgent::default()
    };
    talk_or_later(out, Chat::hosted(account, agent, thread), name, args).await
}

/// The pieces of an answer, written to a command's output as they come:
/// as the words of a person's terminal, or, for `--stream-json`, one
/// text record a line.
struct Pieces<'a> {
    out: &'a mut Out,
    stream: bool,
}

impl Write for Pieces<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(bytes);
        match self.stream {
            true => self
                .out
                .reply_line(&serde_json::json!({"kind": "text", "text": text})),
            false => self.out.live().piece(&text),
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The message `args` sends: given, or read from stdin; for a live chat,
/// only its first message, when given. Read once, so a message sent again
/// is the same.
pub(crate) fn message_of(args: &cli::Chat) -> Result<Option<String>, Report> {
    use std::io::IsTerminal;
    let stdin = std::io::stdin();
    let message = match args.message() {
        Some(message) => Some(message),
        None if !args.live && !stdin.is_terminal() => {
            let mut message = String::new();
            std::io::Read::read_to_string(&mut stdin.lock(), &mut message)
                .within("reading the message on stdin")?;
            // A line's end is the file's, not the message's.
            Some(message.trim_end_matches(['\n', '\r']).to_string())
        }
        None if args.live => None,
        None => {
            return Err(Report::error(sentence!("Give a message to send."))
                .exit(Exit::Usage)
                .next(
                    cmd!("lnk agent chat send --message TEXT"),
                    "send one message",
                )
                .next(cmd!("lnk agent chat"), "start a live chat"));
        }
    };
    if message.as_ref().is_some_and(|m| m.trim().is_empty()) {
        return Err(Report::error(sentence!("There's no message to send."))
            .exit(Exit::Usage)
            .next(cmd!("lnk agent chat send --message TEXT"), "give one"));
    }
    Ok(message)
}

/// `lnk agent chat send` in `chat`, with agent `name`: one message, from
pub(crate) async fn talk(
    out: &mut Out,
    chat: link_agent::chat::Chat,
    name: &str,
    args: &cli::Chat,
) -> Result<Done, Report> {
    match talk_or_later(out, chat, name, args).await? {
        Ok(done) => Ok(done),
        Err(later) => Err(Report::error(Sentence::adopt(&later.why))),
    }
}

/// [`talk`], or, when the cloud hosting the agent refused the message
/// for now (it's moving), when it may be sent again: nothing was taken.
pub(crate) async fn talk_or_later(
    out: &mut Out,
    chat: link_agent::chat::Chat,
    name: &str,
    args: &cli::Chat,
) -> Result<Result<Done, link_agent_place::Later>, Report> {
    use link_agent::chat;
    let message = message_of(args)?;
    // Ctrl-C, from now on, stops waiting for an answer rather than this:
    // caught here, before the chat starts anything, not once a task
    // first runs, so one pressed early never ends this unanswered.
    #[cfg(unix)]
    let mut caught = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .within("catching Ctrl-C")?;
    let (interrupted, mut interrupts) = tokio::sync::mpsc::channel(4);
    tokio::spawn(async move {
        loop {
            #[cfg(unix)]
            let pressed = caught.recv().await.is_some();
            #[cfg(not(unix))]
            let pressed = tokio::signal::ctrl_c().await.is_ok();
            if !pressed || interrupted.send(()).await.is_err() {
                return;
            }
        }
    });
    if args.live {
        out.block(Note(sentence!(
            "Talking to {} in {}. Ctrl-C stops an answer; Ctrl-D leaves.",
            Name::new("agent", name),
            Name::new("chat", chat.thread())
        )));
        // Each line typed, read apart from the answers coming.
        let (typed, mut lines) = tokio::sync::mpsc::channel(1);
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { return };
                if typed.blocking_send(line).is_err() {
                    return;
                }
            }
        });
        let mut pieces = Pieces { out, stream: false };
        chat::converse(&chat, message, &mut lines, &mut interrupts, &mut pieces).await?;
        return Ok(Ok(Done::shown()));
    }
    let Some(message) = message else {
        return Err(Report::error(sentence!("Give a message to send.")).exit(Exit::Usage));
    };
    let stop = async {
        interrupts.recv().await;
    };
    let mut pieces = Pieces {
        out,
        stream: args.stream_json,
    };
    let shown: Option<&mut (dyn Write + Send)> = match args.json {
        true => None,
        false => Some(&mut pieces),
    };
    let chatted = match chat::once(&chat, &message, shown, stop).await {
        Ok(chatted) => chatted,
        Err(err) => {
            return match err.downcast::<link_agent_place::Later>() {
                Ok(later) => Ok(Err(later)),
                Err(err) => Err(err.into()),
            };
        }
    };
    let Some(chatted) = chatted else {
        return Err(Report::error(Sentence::adopt(chat.stopped())).exit(Exit::Interrupted));
    };
    if chatted.answer.trim().is_empty() {
        return Err(Report::error(sentence!("The agent didn't answer."))
            .next(cmd!("lnk agent log show"), "see why"));
    }
    // How it ended, the stream's last event: --json's one answer, and
    // --stream-json's last line.
    if args.json || args.stream_json {
        let mut answered = serde_json::to_value(&chatted).within("the answer as JSON")?;
        answered["kind"] = "answered".into();
        match args.json {
            true => {
                link_actions::answer::reply::<link_agent_spec::declared::ChatEvent>(out, &answered)?
            }
            false => {
                link_actions::answer::line::<link_agent_spec::declared::ChatEvent>(out, &answered)?
            }
        }
    }
    Ok(Ok(Done::shown()))
}

/// `lnk agent chat export --format <way>`: each of its conversations
/// kept another way, moved to `way` by its harness
/// (`lnk-<harness> conversations move <way>`).
/// The agent is stopped, so no turn writes meanwhile.
pub(crate) fn conversations_move(
    out: &mut Out,
    settings: &Settings,
    way: &str,
) -> Result<Done, Report> {
    if service::running_anywhere(&settings.name) {
        return Err(Report::error(sentence!(
            "{} is running, and a turn would write while its conversations move.",
            Name::new("agent", &settings.name)
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent stop"), "stop it first"));
    }
    let (adapter, boxed) = conversations_harness(settings)?;
    let drivers = store_drivers(settings)?;
    let printed = adapter.conversations(&boxed, &["conversations", "move", way], true, &drivers)?;
    out.data(&printed);
    Ok(Done::shown())
}

/// The agent's harness, when it reads and moves its conversations.
pub(crate) fn conversations_harness(settings: &Settings) -> anyhow::Result<(Adapter, Boxed)> {
    let name = settings.harness();
    let adapter = Adapter::find(name)?;
    ensure!(
        adapter.contract()?.conversations,
        "{name} can't print or move its conversations; Link Harness can (lnk agent harness set link)"
    );
    let boxed = Boxed::new(settings, name)?;
    Ok((adapter, boxed))
}

/// The programs of the store drivers the agent's conversations are kept
/// by or its spec names ([`memory::store_drivers`]): its harness runs
/// them to read and write them. One not installed is an error.
pub(crate) fn store_drivers(settings: &Settings) -> anyhow::Result<Vec<PathBuf>> {
    memory::store_drivers(settings)?
        .iter()
        .map(|name| {
            let module = link_store_contract::store::module_for(name);
            let program = link_core::find_module(module).within(format!(
                "the store driver {name} (lnk-{module}) isn't installed: lnk module add {module}"
            ))?;
            Ok(program.canonicalize().unwrap_or(program))
        })
        .collect()
}

/// `lnk agent skill add <source>`: the skill copied into the agent's
/// `skills/`, and whether its harness lists it and it can read it.
pub(crate) fn skill_add(
    out: &mut Out,
    settings: &Settings,
    source: &str,
    replace: bool,
) -> Result<Done, Report> {
    use link_agent::skills;
    let name = settings.harness();
    let lists = match Adapter::find(name) {
        Ok(adapter) => adapter.contract()?.instructions,
        Err(_) => false,
    };
    let added = skills::add(
        &settings.folder().agent,
        &skills::Source::parse(source)?,
        replace,
    )?;
    if !added.left_out.is_empty() {
        out.block(Report::warning(sentence!(
            "Left out, as neither a file nor a folder: {}.",
            Value::List(
                added
                    .left_out
                    .iter()
                    .map(|f| Foreign::new(f).into())
                    .collect()
            )
        )));
    }
    let done = match added.replaced {
        true => sentence!(
            "Replaced skill {} in {}: {}.",
            Foreign::new(&added.skill.name),
            Path::new(&added.at),
            Foreign::new(&added.skill.description)
        ),
        false => sentence!(
            "Added skill {} in {}: {}.",
            Foreign::new(&added.skill.name),
            Path::new(&added.at),
            Foreign::new(&added.skill.description)
        ),
    };
    let done = Done::success(done);
    Ok(if !lists {
        out.block(Note(sentence!(
            "{} doesn't read skills/; Link Harness lists them in its prompt.",
            Name::new("harness", name)
        )));
        done.next(cmd!("lnk agent harness set link"), "switch to Link Harness")
    } else if settings.memory.is_none() {
        out.block(Note(sentence!(
            "Your agent reads a skill through its memory, which is off."
        )));
        done.next(cmd!("lnk agent memory start"), "turn memory on")
    } else {
        out.block(Note(sentence!("Your agent sees it from its next message.")));
        done
    })
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    /// Every command sent as a message can fail for its agent not
    /// answering, which says to read its log: each declares it, or a debug
    /// build panics on that failure instead of reporting it.
    #[test]
    fn each_chat_command_declares_its_log_on_failure() {
        let actions: toml::Value = crate::navigation::navigation();
        for (line, typed) in [
            (&["chat", "send", "-m", "hi"][..], "agent chat send"),
            (&["chat", "start", "--chat", "work"], "agent chat start"),
            (&["chat", "show", "work", "--status"], "agent chat show"),
            (&["chat", "compact"], "agent chat compact"),
            (&["chat", "export"], "agent chat export"),
            (&["chat", "stop"], "agent chat stop"),
            (&["chat", "get", "goal"], "agent chat get"),
            (&["chat", "set", "goal", "--clear"], "agent chat set"),
            (
                &["model", "set", "fast", "--chat", "work"],
                "agent model set",
            ),
        ] {
            let command = link_agent::cli::Cli::try_parse_from(
                std::iter::once("lnk-agent").chain(line.iter().copied()),
            )
            .unwrap()
            .command;
            assert!(
                link_agent::cli::Chat::of(&command).is_some(),
                "{line:?} is a chat"
            );
            let declared: Vec<String> = actions[typed]["fail"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect();
            let given = link_format::Command::fill("lnk agent log show", vec![]);
            let typed_command = link_format::Command::fill(&format!("lnk {typed}"), vec![]);
            assert!(
                link_format::nexts::declares(&declared, &given, &typed_command),
                "{typed} doesn't declare lnk agent log show on failure"
            );
        }
    }
}

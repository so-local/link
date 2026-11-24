//! Commands: a message of the owner's own that names one (`/new`) runs it
//! in place of a turn. Each is a [`Command`] of one list, found by its
//! name: what it does is a [`Does`]. A message from anyone else (a
//! person the agent is shared with, a schedule, a subagent) is never one:
//! it is text for the model, whatever it says.
//!
//! Each is on as the agent's spec says (`[commands]`), read at each
//! message: one it leaves out is text for the model.
//!
//! What a command does is an event of its own, of the role
//! [`event::COMMAND`], kept in the conversation like any other and never
//! sent to the model.

use crate::event::{self, Event};

mod conversations;
pub mod models;
pub mod skills;
pub use conversations::{
    Began, Conversation, Done, began, conversation, conversations, fork, is_new, needs_all, rename,
    resume,
};
pub use models::{Chosen, chosen};

/// A command: its name, as the owner types it after `/`, what it's for,
/// and what it does.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub does: Does,
    /// Other names it runs by, as another harness calls it: `model`.
    pub aliases: Vec<String>,
}

/// What a command does.
#[derive(Clone, Debug, PartialEq)]
pub enum Does {
    /// A proved namespace action, with the same JSON request as its route.
    Action(String),
    /// Names each command that's on, and what it's for.
    Help,
    /// Says what the agent runs on and what this chat's conversation
    /// holds: its model, the chat, its messages and their tokens.
    Status,
    /// Starts a new conversation in the same chat: the events before it
    /// are kept, and searched, but never sent to the model again.
    New,
    /// Ends the answer being written in this chat, as it is: what it
    /// wrote kept, marked cut short, and its calls still running
    /// cancelled.
    Stop,
    /// The model namespace's (`link_model_contract::actions`): what
    /// Link knows of the agent's model.
    ModelShow,
    /// The models its provider offers that its spec allows.
    ModelList,
    /// Given one of them, or an alias of `[model.aliases]`, switches
    /// this conversation to it; with `--every-chat`, every chat of the agent
    /// too, until it starts again; with `--fast`, asks its provider's
    /// faster service; given none, names it and the others.
    ModelSet,
    /// The conversation's model and effort, without changing either.
    ModelGet,
    /// The enabled model commands and their usage.
    ModelHelp,
    /// Asks a turn of its own, with Link's `learn` skill ([`LEARN`]):
    /// a skill written from what the agent just did, or one fixed that
    /// misled it, with what the owner said after it.
    Learn,
    /// Given a condition, sets the chat's goal, asked as a turn of its
    /// own ([`crate::goal`]); alone, shows it; `/goal --clear` drops it.
    Goal,
    /// Given a conversation's number or name, takes it up again in this
    /// chat ([`conversations`]); alone, lists the chat's conversations.
    Resume,
    /// Begins a conversation holding this one as it stands, named by
    /// what follows, if anything.
    Fork,
    /// Names this conversation; alone, says its name.
    Rename,
    /// Writes this conversation, as people read it, to a file in the
    /// agent's folder.
    Export,
    /// Sets what the owner's next message does while a turn runs: waits
    /// for it to end ([`link_agent_spec::Busy::Queue`]), or joins it
    /// ([`link_agent_spec::Busy::Steer`]). Given a message, that's the
    /// one.
    Busy(link_agent_spec::Busy),
    /// Summarizes what's before the owner's last message now, as a turn
    /// of its own ([`crate::compact`]).
    Compact,
    /// Lists the skills the agent has, the owner's and the
    /// repository's ([`skills`]).
    Skills,
    /// Whether this conversation's chat shows each tool call, `on` or
    /// `off`; alone, the other of what it does now.
    Verbose,
    /// Asks the model, given a question, with the conversation as it
    /// stands and no tool callable, keeping neither: while a turn runs
    /// too.
    Btw,
}

impl Does {
    /// Whether it takes anything after its name: one that doesn't, given
    /// something, isn't run, and the message is text.
    fn takes_arguments(&self) -> bool {
        match self {
            Self::Help
            | Self::Status
            | Self::New
            | Self::Stop
            | Self::ModelShow
            | Self::ModelGet
            | Self::ModelHelp
            | Self::ModelList => false,
            Self::Export | Self::Compact | Self::Skills => false,
            Self::ModelSet
            | Self::Learn
            | Self::Goal
            | Self::Action(_)
            | Self::Resume
            | Self::Fork
            | Self::Rename
            | Self::Busy(_)
            | Self::Btw
            | Self::Verbose => true,
        }
    }
}

/// What the owner is told once `/new` has run.
pub const NEW: &str =
    "A new conversation: what came before stays in its history, and isn't sent to the model again.";

/// What the owner is told once `/chat:queue` or `/chat:steer` has run
/// alone: what their next message does while a turn runs.
pub fn busy(busy: link_agent_spec::Busy) -> &'static str {
    match busy {
        link_agent_spec::Busy::Queue => "Your next message waits for the answer being written.",
        link_agent_spec::Busy::Steer => "Your next message joins the answer being written.",
        link_agent_spec::Busy::Interrupt => "Your next message stops the answer being written.",
    }
}

/// What the owner's message `message` asks to do while a turn runs, as
/// a `/chat:queue` or `/chat:steer` that ran alone just before it, the
/// last of `events`, says: none when nothing does.
pub fn busy_asked(events: &[Event]) -> Option<link_agent_spec::Busy> {
    let last = events
        .iter()
        .rev()
        .find(|e| (e.role == event::USER && e.from.is_none()) || e.role == event::COMMAND)?;
    match (last.role == event::COMMAND && !last.failed).then_some(last.text.as_str())? {
        "/chat:queue" => Some(link_agent_spec::Busy::Queue),
        "/chat:steer" => Some(link_agent_spec::Busy::Steer),
        _ => None,
    }
}

/// What `/chat:ask` says given no question.
pub const NO_QUESTION: &str = "Ask something after it: /chat:ask <question>.";

/// What `/chat:compact` says where the spec's `[window] compact_at` is
/// `"never"`.
pub const NO_COMPACTION: &str =
    "Nothing is summarized here: the spec's [window] compact_at is never.";
/// What it says with nothing before the owner's last message to
/// summarize.
pub const NOTHING_TO_COMPACT: &str =
    "Nothing to compact: there's nothing before your last message that isn't summarized.";
/// What a quiet conversation's compaction says, to nobody, when a turn
/// was running as it came: that turn keeps it again.
pub const BUSY_NOT_COMPACTED: &str = "Not compacted: a turn is running, which keeps it again.";
/// What it says when the model wrote no summary.
pub const NOT_COMPACTED: &str = "It couldn't be summarized now: nothing left the window.";

/// What `/chat:compact` says once `events` events before the owner's
/// last message are summarized, and notes saved first if `saved`.
pub fn compacted(events: usize, saved: bool) -> String {
    let notes = match saved {
        true => ", what mattered saved to its notes first",
        false => "",
    };
    format!(
        "Compacted: {events} events before your last message summarized{notes}. Your last message and what follows stay."
    )
}

/// `arguments`, an action's JSON, with `model` where it names none:
/// as it is when it isn't an object, for the action's client to refuse.
pub fn with_model(arguments: &str, model: Option<&str>) -> String {
    let given = match arguments {
        "" => Ok(serde_json::json!({})),
        arguments => serde_json::from_str::<serde_json::Value>(arguments),
    };
    match (given, model) {
        (Ok(serde_json::Value::Object(mut given)), Some(model)) => {
            given.entry("model").or_insert_with(|| model.into());
            serde_json::Value::Object(given).to_string()
        }
        _ => arguments.to_string(),
    }
}

/// What the owner is told once `/stop` has run, an answer stopped or
/// none running.
pub const STOPPED: &str = "Stopped: what it wrote so far is kept.";
pub const NOTHING_TO_STOP: &str = "Nothing to stop: no answer is being written here.";

/// Link's `learn` skill, which `/learn` asks a turn with: what's said
/// after it ends it.
pub const LEARN: &str = include_str!("../commands/learn.md");

/// The message `/learn` asks its turn with: the `learn` skill, past its
/// head, and what the owner said after it, or that they said nothing.
pub fn learn(arguments: &str) -> String {
    let skill = LEARN.splitn(3, "---\n").nth(2).unwrap_or(LEARN).trim();
    match arguments.trim() {
        "" => format!("/learn\n\n{skill} nothing more."),
        asked => format!("/learn {asked}\n\n{skill}\n\n{asked}"),
    }
}

/// Whether `config`'s agent may learn: its file tool on, and `skills/`
/// among the folders its sandbox file lets it write, by name.
fn may_learn(config: &crate::config::Config) -> bool {
    let skills = link_harness_contract::harness::layout::SKILLS;
    config.opinions().tool("file").enabled != Some(false)
        && config
            .environment
            .as_ref()
            .is_some_and(|e| e.folders.write.iter().any(|f| f == skills))
}

/// The commands of Link Harness that `config`'s spec turns on, in the
/// order `/help` names them: `/help`, then each namespace's as its
/// module's `actions.toml` says them, `/skill:write` only where it may
/// learn and `/chat:goal` only where `[goals] turns` is set.
pub fn list(config: &crate::config::Config) -> Vec<Command> {
    let on = config.opinions().commands.unwrap_or_default();
    let goals = config.opinions().goals.and_then(|g| g.turns).is_some();
    let help = Command {
        name: "help".into(),
        description: "List these commands".into(),
        does: Does::Help,
        aliases: Vec::new(),
    };
    let agents = namespace(link_agent_contract::actions());
    let models = namespace(link_model_contract::actions());
    let (skills, chats): (Vec<_>, Vec<_>) = agents.into_iter().partition(|c| c.does == Does::Learn);
    std::iter::once(help)
        .chain(chats)
        .chain(models)
        .chain(skills)
        .filter(|c| {
            let switch = match &c.does {
                Does::Action(path) => Some(
                    config.actions.actions.contains(path)
                        && (path != "task list" || on.tasks == Some(true)),
                ),
                Does::Help => on.help,
                Does::Status => on.status,
                Does::New => on.create,
                Does::Stop => on.stop,
                Does::ModelShow | Does::ModelSet | Does::ModelGet | Does::ModelHelp => on.model,
                Does::ModelList => on.models,
                Does::Learn => on.write.filter(|_| may_learn(config)),
                Does::Goal => on.goal.filter(|_| goals),
                Does::Resume => on.resume,
                Does::Fork => on.fork,
                Does::Rename => on.rename,
                Does::Export => on.export,
                Does::Busy(link_agent_spec::Busy::Steer) => on
                    .steer
                    .filter(|_| crate::turn::takes(config).contains(&link_agent_spec::Busy::Steer)),
                Does::Busy(_) => on.queue,
                Does::Compact => on.compact,
                Does::Skills => on.skills,
                Does::Verbose => on.verbose,
                Does::Btw => on.ask,
            };
            switch == Some(true)
        })
        .collect()
}

/// The actions of `actions` an agent offers, as its module's
/// `actions.toml` says them: one Link Harness can't run isn't offered.
fn namespace(actions: link_component::actions::Actions) -> Vec<Command> {
    use link_component::actions::{Surface, command_name};
    actions
        .into_iter()
        .filter(|(_, action)| !action.hidden() && action.on(Surface::Command))
        .filter_map(|(path, action)| {
            let does = match path.as_str() {
                "chat status" => Does::Status,
                "chat create" => Does::New,
                "chat stop" => Does::Stop,
                "chat goal" => Does::Goal,
                "model show" => Does::ModelShow,
                "model list" => Does::ModelList,
                "model set" => Does::ModelSet,
                "model get" => Does::ModelGet,
                "model help" => Does::ModelHelp,
                "skill write" => Does::Learn,
                "chat resume" => Does::Resume,
                "chat fork" => Does::Fork,
                "chat rename" => Does::Rename,
                "chat export" => Does::Export,
                "chat queue" => Does::Busy(link_agent_spec::Busy::Queue),
                "chat steer" => Does::Busy(link_agent_spec::Busy::Steer),
                "chat compact" => Does::Compact,
                "skill list" => Does::Skills,
                "chat verbose" => Does::Verbose,
                "chat ask" => Does::Btw,
                _ if action.on(Surface::Route) && action.bridge.is_some() => {
                    Does::Action(path.clone())
                }
                _ => return None,
            };
            let name = command_name(&path);
            let usage = match does {
                Does::ModelSet => {
                    format!(": /{name} <name>, --fast, --no-fast, --effort <level>, --every-chat")
                }
                Does::Learn => format!(": /{name} <what>"),
                Does::Goal => format!(": /{name} <condition>, /{name} --clear"),
                Does::Resume => format!(": /{name} <number or name>"),
                Does::Fork | Does::Rename => format!(": /{name} <name>"),
                Does::Busy(_) => format!(": /{name} <message>"),
                Does::Btw => format!(": /{name} <question>"),
                Does::Verbose => format!(": /{name} on|off"),
                _ => String::new(),
            };
            Some(Command {
                description: format!("{}{usage}", action.what()),
                name,
                does,
                aliases: action
                    .aliases
                    .iter()
                    .map(|a| a.trim_start_matches('/').to_string())
                    .collect(),
            })
        })
        .collect()
}

/// A harness command's answer, marked as its agent's spec asks.
pub fn response(spec: &link_agent_spec::Spec, text: &str) -> String {
    match spec
        .command_response
        .as_deref()
        .filter(|prefix| !prefix.is_empty())
    {
        Some(prefix) => format!("{prefix}\n{text}"),
        None => text.to_string(),
    }
}

/// What `/help` says: each command of `commands`, and what it's for.
pub fn help(commands: &[Command]) -> String {
    let mut told = String::from("Commands:");
    for command in commands {
        told.push_str(&format!("\n/{} - {}", command.name, command.description));
    }
    told
}

/// What a conversation holds, as `/status` says it.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    /// The model's id, and its provider.
    pub model: String,
    pub provider: String,
    pub chat: String,
    /// The owner's messages and the agent's answers since the last `/new`.
    pub messages: usize,
    /// About how many tokens they take, and the window a turn fills.
    pub tokens: usize,
    pub window: u32,
    /// Whether a `/new` began it.
    pub new: bool,
}

impl Status {
    /// Of the conversation's `events` since its last `/new`
    /// ([`conversation`]).
    pub fn of(events: &[Event], model: String, provider: String, chat: &str, window: u32) -> Self {
        let said = |e: &&Event| e.role == event::USER || e.role == event::ASSISTANT;
        Self {
            model,
            provider,
            chat: chat.into(),
            messages: events.iter().filter(said).count(),
            tokens: events.iter().map(crate::window::tokens).sum(),
            window,
            new: events.first().is_some_and(is_new),
        }
    }

    /// What the owner is told.
    pub fn told(&self) -> String {
        let since = match self.new {
            true => "since /chat:create",
            false => "since it began",
        };
        format!(
            "Model: {} ({})\nChat: {}\nConversation: {} messages {since}, about {} of {} tokens",
            self.model, self.provider, self.chat, self.messages, self.tokens, self.window,
        )
    }
}

/// What `/model:show` says of `name` on `provider`: what Link knows of
/// it, `known`, if anything.
pub fn shown(name: &str, provider: &str, known: Option<&link_model_contract::Known>) -> String {
    let mut told = format!("Model: {name} ({provider})");
    let Some(facts) = known.map(|k| &k.facts) else {
        told.push_str("\nLink knows nothing more of it.");
        return told;
    };
    let tokens = |n: Option<u32>| n.map_or("?".to_string(), |n| n.to_string());
    told.push_str(&format!(
        "\nWindow: {} tokens, answers up to {}",
        tokens(facts.window),
        tokens(facts.answer)
    ));
    if let Some(thinks) = facts
        .thinking
        .and_then(|t| serde_json::to_value(t).ok())
        .and_then(|t| t.as_str().map(str::to_string))
    {
        told.push_str(&format!("\nThinks: {thinks}"));
    }
    if let Some(price) = facts.price {
        told.push_str(&format!(
            "\nPrice: ${} in, ${} out, a million tokens",
            price.input, price.output
        ));
    }
    for jump in &facts.above {
        told.push_str(&format!(
            "\nPast {} tokens of prompt: ${} in, ${} out",
            jump.tokens, jump.price.input, jump.price.output
        ));
    }
    told
}

/// What `/model:list` says: what its provider offers that its spec
/// allows, `offered`, the model `current` marked.
pub fn listed(current: &str, offered: &[String]) -> String {
    if offered.is_empty() {
        return "Its provider offers none its spec allows.".into();
    }
    let mut told = String::from("Its provider's models:");
    for model in offered {
        told.push_str(&format!("\n{model}"));
        if model == current {
            told.push_str(" (on)");
        }
    }
    told
}

/// What `/model:set` says: the model `current`, given nothing; switched
/// to `asked`, when it's of `offered`, what its provider offers that its
/// spec allows, for this conversation or, `agent`, every chat; else
/// refused, naming those. The model to switch to comes back with what's
/// said.
pub fn model(
    current: &str,
    asked: &str,
    offered: &[String],
    agent: bool,
) -> (Option<String>, String) {
    let others: Vec<&str> = offered
        .iter()
        .map(String::as_str)
        .filter(|m| *m != current)
        .collect();
    let others = match others.is_empty() {
        true => "none".to_string(),
        false => others.join(", "),
    };
    if asked.is_empty() {
        return (
            None,
            format!("Model: {current}\nIts provider's others: {others}"),
        );
    }
    if asked == current && !agent {
        return (None, format!("Already on {current}."));
    }
    match offered.iter().any(|m| m == asked) {
        true => (
            Some(asked.into()),
            format!("Model: {asked}, {}.", whose(agent)),
        ),
        false => (
            None,
            format!(
                "No {asked} of its provider's that its spec allows: on {current}; its others: {others}"
            ),
        ),
    }
}

/// Where a `/model:set` holds: this conversation, or
/// with `--every-chat` every chat.
fn whose(agent: bool) -> &'static str {
    match agent {
        true => "in every chat, until the agent starts again with its own",
        false => "in this conversation",
    }
}

/// What `/chat:verbose` says once this conversation's chat shows each
/// tool call, `on`, or doesn't.
pub fn verbose(on: bool) -> &'static str {
    match on {
        true => "Verbose: each tool it calls shows here, in this conversation.",
        false => "Quiet: the tools it calls don't show here, in this conversation.",
    }
}

/// What `/model:set --fast` says where `model`'s provider offers no
/// faster service.
pub fn no_fast(model: &str) -> String {
    format!("Its provider offers no faster service for {model}, so it stays as it is.")
}

/// What `/model:set --fast` says once it's on, or off, for `model`.
pub fn fast(model: &str, on: bool) -> String {
    match on {
        true => {
            format!("Fast: {model} asked of its provider's faster service, in this conversation.")
        }
        false => format!("Fast off: {model} asked as usual, in this conversation."),
    }
}

/// What `/model:set --effort` says of `level`: alone (`agent` none), how
/// hard the model thinks; set, where.
pub fn effort(level: Option<link_agent_spec::Thinking>, agent: Option<bool>) -> String {
    let level = level.map_or("off", |l| l.word());
    match agent {
        None => format!("Effort: {level}; --effort off, low, medium or high sets it."),
        Some(agent) => format!("Effort: {level}, {}.", whose(agent)),
    }
}

/// The name and what follows it of `message`, when it's one a command
/// or a skill may run: the owner's own, with no images or documents,
/// its text `/` and a name.
pub(crate) fn owners_slash(message: &Event) -> Option<(&str, &str)> {
    if message.role != event::USER || message.from.is_some() || !message.parts.is_empty() {
        return None;
    }
    let text = message.text.trim().strip_prefix('/')?;
    Some(
        text.split_once(char::is_whitespace)
            .map_or((text, ""), |(name, rest)| (name, rest.trim())),
    )
}

/// A command a message runs, and what follows its name.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub command: Command,
    pub arguments: String,
}

/// The command of `commands` that `message` runs: a message of the
/// owner's own, with no images or documents, whose text is `/`, a
/// command's name, and what it takes after it. None for any other,
/// which is text.
pub fn asked(message: &Event, commands: &[Command]) -> Option<Run> {
    let (name, arguments) = owners_slash(message)?;
    let command = commands
        .iter()
        .find(|c| c.name == name || c.aliases.iter().any(|a| a == name))?;
    if !arguments.is_empty() && !command.does.takes_arguments() {
        return None;
    }
    Some(Run {
        command: command.clone(),
        arguments: arguments.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plumbing_namespace_actions_and_their_aliases_are_not_offered() {
        let mut actions = link_component::actions::parse(
            "schema = 1\n[actions.'chat.chat.create']\ndescription = 'New chat'\n\
             role = 'secondary'\noutput = 'core.report'\nsurfaces = []\n\
             route = { bridge = 'harness', callers = ['agent'], command = true }\n",
        )
        .unwrap();
        actions
            .get_mut("chat create")
            .unwrap()
            .aliases
            .push("/new".into());
        let visible = namespace(actions.clone());
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "chat:create");
        actions.get_mut("chat create").unwrap().role = link_component::actions::Role::Plumbing;
        assert!(
            namespace(actions).is_empty(),
            "plumbing never reaches the real command reader"
        );
    }

    fn owner(text: &str) -> Event {
        Event::user(text)
    }

    /// Its spec's `[commands]`: Link's turns `/new` on; one leaving it
    /// out or false leaves it text.
    #[test]
    fn a_command_runs_only_where_its_spec_turns_it_on() {
        let config = |spec: &str| crate::config::Config {
            spec: Some(link_agent_spec::parse(&format!("schema = 1\n{spec}")).unwrap()),
            ..Default::default()
        };
        // Link's four, the model's five, /goal for its [goals], the
        // chat's nine more and /skill:list; /learn waits on skills/.
        assert_eq!(list(&crate::config::Config::default()).len(), 20);
        assert_eq!(list(&config("[commands]\ncreate = true\n")).len(), 1);
        for off in [
            "[commands]\ncreate = false\nhelp = false\nstatus = false\nstop = false\nmodel = false\n",
            "",
        ] {
            let commands = list(&config(off));
            assert!(commands.is_empty(), "{off}");
            assert_eq!(asked(&owner("/new"), &commands), None);
        }
    }

    #[test]
    fn only_the_owners_own_message_runs_one() {
        let commands = list(&crate::config::Config::default());
        let asked = asked(&owner("/new"), &commands).expect("the owner's /new");
        assert_eq!(asked.command.does, Does::New);
        assert_eq!(super::asked(&owner("  /new\n"), &commands), Some(asked));
        // From anyone else, it's text.
        for from in ["member:telegram:42", "schedule:daily", "task:t1", "agent:a"] {
            let message = Event {
                from: Some(from.into()),
                ..owner("/new")
            };
            assert_eq!(super::asked(&message, &commands), None, "{from}");
        }
        // With a photo, or a word after it, or another name: text.
        let photo = Event {
            parts: vec![event::Part {
                kind: event::Kind::Image,
                media: "image/png".into(),
                name: None,
                file: "parts/1-0.png".into(),
                bytes: 1,
            }],
            ..owner("/new")
        };
        for message in [
            photo,
            owner("/new please"),
            owner("/newer"),
            owner("new"),
            owner("/"),
        ] {
            assert_eq!(super::asked(&message, &commands), None, "{}", message.text);
        }
        // An answer of the agent's isn't the owner's.
        let answer = Event {
            role: event::ASSISTANT.into(),
            ..owner("/new")
        };
        assert_eq!(super::asked(&answer, &commands), None);
    }

    #[test]
    fn models_and_tasks_aliases_have_their_own_switches_and_require_grants() {
        let mut config = crate::config::Config::default();
        config.actions.actions.push("task list".into());
        let commands = list(&config);
        assert_eq!(
            asked(&owner("/models"), &commands).unwrap().command.does,
            Does::ModelList
        );
        for alias in ["/tasks", "/ps", "/task:list"] {
            assert_eq!(
                asked(&owner(alias), &commands).unwrap().command.does,
                Does::Action("task list".into())
            );
        }
        config.spec = Some(
            link_agent_spec::parse(
                "schema = 1\n[commands]\nmodel = true\nmodels = false\ntasks = false\n",
            )
            .unwrap(),
        );
        let commands = list(&config);
        assert!(asked(&owner("/model"), &commands).is_some());
        assert!(asked(&owner("/models"), &commands).is_none());
        assert!(asked(&owner("/ps"), &commands).is_none());
        config.spec = None;
        config.actions.actions.clear();
        assert!(asked(&owner("/tasks"), &list(&config)).is_none());
    }

    fn numbered(seq: u64, event: Event) -> Event {
        Event { seq, ..event }
    }

    fn command(seq: u64) -> Event {
        numbered(
            seq,
            Event {
                role: event::COMMAND.into(),
                ..owner("/new")
            },
        )
    }

    /// `/learn` is on only where the agent may write `skills/` with its
    /// file tool, and asks a turn with the skill and what the owner said.
    #[test]
    fn learn_is_offered_only_where_it_may_write_skills() {
        let learns = |write: &[&str], spec: &str| {
            let mut config = crate::config::Config {
                spec: Some(link_agent_spec::parse(&format!("schema = 1\n{spec}")).unwrap()),
                ..Default::default()
            };
            let mut environment = link_harness_contract::harness::Environment::default();
            environment.folders.write = write.iter().map(|w| w.to_string()).collect();
            config.environment = Some(environment);
            list(&config).iter().any(|c| c.does == Does::Learn)
        };
        let on = "[commands]\nwrite = true\n";
        assert!(learns(&["work", "skills"], on));
        assert!(!learns(&["work"], on));
        assert!(!learns(&["."], on), "Home's, not its own folder's skills");
        assert!(!learns(
            &["skills"],
            "[commands]\nwrite = true\n[tool.file]\nenabled = false\n"
        ));
        assert!(!learns(&["skills"], "[commands]\nhelp = true\n"));
        assert!(
            !list(&crate::config::Config::default())
                .iter()
                .any(|c| c.does == Does::Learn)
        );
        let asked = learn("the deploy");
        assert!(
            asked.starts_with("/learn the deploy\n\nYour owner asked you"),
            "{asked}"
        );
        assert!(
            asked.contains("`skills/<name>/SKILL.md`")
                && asked.ends_with("if they said:\n\nthe deploy")
        );
        assert!(learn("").ends_with("if they said: nothing more."));
    }

    #[test]
    fn help_names_each_command_that_is_on() {
        let commands = list(&crate::config::Config::default());
        let told = help(&commands);
        for name in [
            "/help",
            "/chat:status",
            "/chat:create",
            "/chat:stop",
            "/model:show",
            "/model:list",
            "/model:set <name>, --fast, --no-fast, --effort <level>, --every-chat",
        ] {
            assert!(told.contains(name), "{told}");
        }
        let config = crate::config::Config {
            spec: Some(link_agent_spec::parse("schema = 1\n[commands]\nhelp = true\n").unwrap()),
            ..Default::default()
        };
        let told = help(&list(&config));
        assert!(
            told.contains("/help") && !told.contains("/chat:create"),
            "{told}"
        );
        // Each runs by its name and by the root-level name it had.
        let commands = list(&crate::config::Config::default());
        for (text, does) in [
            ("/chat:create", Does::New),
            ("/new", Does::New),
            ("/chat:stop", Does::Stop),
            ("/stop", Does::Stop),
            ("/status", Does::Status),
        ] {
            assert_eq!(
                asked(&owner(text), &commands).map(|r| r.command.does),
                Some(does)
            );
        }
        // A `/chat:create` that ran starts a conversation, as a `/new` does.
        let ran = |text: &str| Event {
            role: event::COMMAND.into(),
            text: text.into(),
            ..Event::default()
        };
        assert!(is_new(&ran("/chat:create")) && is_new(&ran("/new")));
    }

    #[test]
    fn model_switches_only_to_one_its_provider_offers() {
        let offered = vec!["a".to_string(), "b".to_string()];
        let (to, told) = model("a", "", &offered, false);
        assert_eq!(to, None);
        assert!(told.contains("Model: a") && told.contains("b"), "{told}");
        assert_eq!(model("a", "b", &offered, false).0.as_deref(), Some("b"));
        assert!(model("a", "b", &offered, true).1.contains("every chat"));
        assert!(
            model("a", "b", &offered, false)
                .1
                .contains("this conversation")
        );
        assert_eq!(model("a", "a", &offered, true).0.as_deref(), Some("a"));
        assert_eq!(model("a", "a", &offered, false).0, None);
        let (to, told) = model("a", "c", &offered, false);
        assert_eq!(to, None);
        assert!(told.contains("No c") && told.contains("b"), "{told}");
        // Its arguments are taken: `/model:set b` runs it, and `/model b`,
        // its alias; `/model:show` and `/model:list` take none.
        let commands = list(&crate::config::Config::default());
        for text in ["/model:set  b ", "/model b"] {
            let run = asked(&owner(text), &commands).expect(text);
            assert_eq!(
                (run.command.does, run.arguments.as_str()),
                (Does::ModelSet, "b")
            );
            assert_eq!(run.command.name, "model:set");
        }
        assert_eq!(
            asked(&owner("/model:show"), &commands).map(|r| r.command.does),
            Some(Does::ModelShow)
        );
        assert_eq!(asked(&owner("/model:list b"), &commands), None);
        assert!(listed("a", &offered).contains("a (on)\nb"));
    }

    #[test]
    fn a_subagent_is_given_the_conversations_model_unless_it_names_one() {
        let given = |arguments, model| {
            serde_json::from_str::<serde_json::Value>(&with_model(arguments, model)).unwrap()
        };
        assert_eq!(given("", Some("m")), serde_json::json!({"model": "m"}));
        assert_eq!(
            given(r#"{"prompt": "Go.", "model": "high"}"#, Some("m"))["model"],
            "high"
        );
        assert_eq!(
            given(r#"{"prompt": "Go."}"#, None),
            serde_json::json!({"prompt": "Go."})
        );
        assert_eq!(with_model("not json", Some("m")), "not json");
    }

    /// `/chat:queue` or `/chat:steer` alone applies to the owner's next
    /// message only: once another message of theirs comes, it's spent.
    #[test]
    fn a_queue_or_steer_alone_applies_to_the_next_message() {
        let ran = |text: &str| Event {
            role: event::COMMAND.into(),
            ..owner(text)
        };
        let busy = link_agent_spec::Busy::Steer;
        assert_eq!(busy_asked(&[ran("/chat:steer")]), Some(busy));
        assert_eq!(
            busy_asked(&[ran("/chat:queue")]),
            Some(link_agent_spec::Busy::Queue)
        );
        assert_eq!(busy_asked(&[ran("/chat:steer"), owner("next")]), None);
        let mut refused = ran("/chat:steer");
        refused.failed = true;
        assert_eq!(busy_asked(&[refused]), None);
        // An answer between leaves it for the next message.
        let answer = Event {
            role: event::ASSISTANT.into(),
            ..owner("done")
        };
        assert_eq!(busy_asked(&[ran("/chat:steer"), answer]), Some(busy));
    }

    /// `/chat:steer` is offered only where the spec's `[turns] busy`
    /// steers: a running turn reads what joins it only then.
    #[test]
    fn steer_is_offered_only_where_the_spec_steers() {
        let steers = |spec: &str| {
            let config = crate::config::Config {
                spec: Some(link_agent_spec::parse(&format!("schema = 1\n{spec}")).unwrap()),
                ..Default::default()
            };
            asked(&owner("/chat:steer"), &list(&config)).is_some()
        };
        let on = "[commands]\nsteer = true\nqueue = true\n";
        assert!(steers(&format!("{on}[turns]\nbusy = \"steer\"\n")));
        assert!(!steers(&format!("{on}[turns]\nbusy = \"queue\"\n")));
    }

    #[test]
    fn model_show_says_what_link_knows_of_it() {
        let told = shown("m", "local", None);
        assert!(
            told.contains("Model: m (local)") && told.contains("nothing more"),
            "{told}"
        );
        let known = link_model_contract::Known {
            name: "m".into(),
            facts: link_model_contract::Facts {
                window: Some(1000),
                answer: Some(100),
                ..Default::default()
            },
            yours: Vec::new(),
        };
        let told = shown("m", "local", Some(&known));
        assert!(
            told.contains("Window: 1000 tokens, answers up to 100"),
            "{told}"
        );
    }

    #[test]
    fn status_counts_the_conversation_since_new() {
        let events = vec![
            command(4),
            numbered(5, owner("two")),
            numbered(
                6,
                Event {
                    role: event::ASSISTANT.into(),
                    ..owner("answered")
                },
            ),
        ];
        let status = Status::of(&events, "m".into(), "local".into(), "default", 1000);
        assert_eq!((status.messages, status.new), (2, true));
        assert!(status.tokens > 0);
        let told = status.told();
        assert!(
            told.contains("Model: m (local)") && told.contains("since /chat:create"),
            "{told}"
        );
        assert!(!Status::of(&events[1..], "m".into(), "local".into(), "default", 1).new);
    }
}

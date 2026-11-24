//! `lnk agent`'s command line, generated from the module's declarations
//! (`actions.toml`) over the shapes its spec contract generates, and
//! what its commands share once read: how an agent starts, and a chat's
//! message.

use std::path::PathBuf;

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line: its command and the field flags of
/// the spec it binds.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk agent`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

/// How `lnk agent start`, `create` and `restore` start an agent: its spec,
/// a file with the agent spec's field flags over it, and what only this
/// start chooses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Starting {
    pub spec: Option<PathBuf>,
    /// In its sandbox, from a sandbox file, or with none: each start's
    /// own, never the spec's. Empty when `--sandbox` names no file, so it
    /// only runs in its sandbox again.
    pub sandbox: Option<PathBuf>,
    pub no_sandbox: bool,
    pub foreground: bool,
    /// This start alone lets the machine sleep, whatever its spec's
    /// `[run] let_sleep` says: until the next start.
    pub let_sleep: bool,
    /// The agent spec's field flags.
    pub fields: link_spec::FieldFlags,
}

/// What `--sandbox` alone means: its own sandbox again, no file.
pub const OWN_SANDBOX: &str = "own";

impl Starting {
    fn of(
        spec: &Option<PathBuf>,
        sandbox: &Option<PathBuf>,
        flags: [bool; 3],
        fields: &link_spec::FieldFlags,
    ) -> Self {
        let [no_sandbox, foreground, let_sleep] = flags;
        Self {
            spec: spec.clone(),
            sandbox: sandbox
                .clone()
                .map(|path| match path.as_os_str() == OWN_SANDBOX {
                    true => PathBuf::new(),
                    false => path,
                }),
            no_sandbox,
            foreground,
            let_sleep,
            fields: fields.clone(),
        }
    }

    pub fn start(a: &AgentStartArgs, fields: &link_spec::FieldFlags) -> Self {
        Self::of(
            &a.spec,
            &a.sandbox,
            [a.no_sandbox, a.foreground, a.let_sleep],
            fields,
        )
    }

    pub fn create(a: &AgentCreateArgs, fields: &link_spec::FieldFlags) -> Self {
        Self::of(
            &a.spec,
            &a.sandbox,
            [a.no_sandbox, a.foreground, a.let_sleep],
            fields,
        )
    }

    pub fn restore(a: &AgentRestoreArgs, fields: &link_spec::FieldFlags) -> Self {
        Self::of(
            &a.spec,
            &a.sandbox,
            [a.no_sandbox, a.foreground, a.let_sleep],
            fields,
        )
    }

    /// Its sandbox file, when `--sandbox` names one.
    pub fn sandbox_file(&self) -> Option<&std::path::Path> {
        self.sandbox
            .as_deref()
            .filter(|path| !path.as_os_str().is_empty())
    }

    /// Whether a spec is supplied: a file, or field flags.
    pub fn supplied(&self) -> bool {
        self.spec.is_some() || !self.fields.is_empty()
    }

    /// The text a field flag set at `path`, where one did.
    pub fn given(&self, path: &[&str]) -> Option<String> {
        match self.fields.get(path)? {
            toml::Value::String(text) => Some(text.clone()),
            other => Some(other.to_string()),
        }
    }
}

/// Where work goes: a command's `--on`, `--cloud`, `--zone`, `--here` and
/// its place.
pub fn slots(
    on: &Option<String>,
    cloud: &Option<String>,
    zone: &Option<String>,
    here: bool,
    place: &Option<String>,
) -> link_workplace::Slots {
    link_workplace::Slots {
        on: on.clone(),
        cloud: cloud.clone(),
        zone: zone.clone(),
        here,
        place: place.clone(),
    }
}

/// One message to the agent, or a live chat with it: `chat send`, `chat
/// start` (bare `chat`), and each chat command a harness answers, sent as
/// its message.
#[derive(Clone, Debug, Default)]
pub struct Chat {
    pub live: bool,
    pub chat: Option<String>,
    pub message: Option<String>,
    pub action: Option<String>,
    pub json: bool,
    pub stream_json: bool,
    /// Whether `chat` must name a conversation the agent already has: a
    /// live chat continuing one, or a chat command asking about one. A
    /// message sent in a new name starts that conversation.
    pub existing: bool,
}

impl Chat {
    /// The conversation this message is in: the named one, a new one for a
    /// live chat, else the agent's main conversation.
    pub fn thread(&self) -> anyhow::Result<String> {
        match self.chat.as_deref() {
            Some(name) => crate::chat::thread(name),
            None if self.live => crate::chat::thread(&link_files::random_token()?),
            None => crate::chat::thread(crate::chat::MAIN),
        }
    }

    /// The message: an action as its command, else the text.
    pub fn message(&self) -> Option<String> {
        self.action
            .as_ref()
            .map(|action| format!("/{}", action.trim().trim_start_matches('/')))
            .or_else(|| self.message.clone())
    }

    /// This chat's arguments, to run it where the agent is.
    pub fn args(&self) -> Vec<String> {
        let mut args = vec![
            "chat".to_string(),
            if self.live { "start" } else { "send" }.to_string(),
        ];
        if let Some(chat) = &self.chat {
            args.extend(["--chat".into(), chat.clone()]);
        }
        if self.json {
            args.push("--json".into());
        }
        if self.stream_json {
            args.push("--stream-json".into());
        }
        if self.live
            && let Some(message) = &self.message
        {
            args.extend(["--message".into(), message.clone()]);
        }
        args
    }

    /// Whether its message goes on the stdin of the chat run where the
    /// agent is: one message's, never a live chat's, whose stdin is the
    /// terminal.
    pub fn piped(&self) -> bool {
        !self.live && self.message().is_some()
    }

    /// A team chat's message, to its member.
    pub fn team(args: &ChatSendArgs) -> Self {
        Self {
            live: false,
            chat: args.chat.clone(),
            message: args.message.clone(),
            action: args.action.clone(),
            json: args.json,
            stream_json: args.stream_json,
            existing: false,
        }
    }

    /// A chat command a harness answers, `path` (`chat ask`), sent as its
    /// message with `text` after it.
    fn request(chat: &Option<String>, json: bool, path: &str, text: Option<String>) -> Self {
        let action = link_agent_contract::action_registry::command_name(path);
        Self {
            chat: chat.clone(),
            json,
            message: Some(match text {
                Some(text) => format!("/{action} {text}"),
                None => format!("/{action}"),
            }),
            ..Self::default()
        }
    }

    /// The chat `command` is, where it is one: `chat send` and `chat
    /// start`, each chat command a harness answers, sent as its message,
    /// and `model set --chat`, one chat's model.
    pub fn of(command: &Command) -> Option<Self> {
        use Chat as C;
        Some(match command {
            // A send as the agent, or to a team, is no message to the agent.
            Command::ChatSend(a) if a.to.is_some() || a.team.is_some() => return None,
            Command::ChatSend(a) if let Some(path) = a.way() => {
                C::request(&a.chat, a.json, path, a.message.clone())
            }
            Command::ChatSend(a) => C {
                live: false,
                chat: a.chat.clone(),
                message: a.message.clone(),
                action: a.action.clone(),
                json: a.json,
                stream_json: a.stream_json,
                existing: false,
            },
            Command::ChatStart(a) => C {
                live: true,
                chat: a.chat.clone(),
                message: a.message.clone(),
                json: a.json,
                existing: a.chat.is_some(),
                ..C::default()
            },
            Command::ChatCompact(a) => C::request(&a.chat, a.json, "chat compact", None),
            // With a format, every conversation is rewritten here instead.
            Command::ChatExport(a) if a.format.is_some() => return None,
            Command::ChatExport(a) => C::request(&a.chat, a.json, "chat export", None),
            Command::ChatStop(a) => C::request(&a.chat, a.json, "chat stop", None),
            Command::ChatGet(a) => C::request(&a.chat, a.json, a.field.path(), None),
            Command::ChatSet(a) => C::request(
                &a.chat,
                a.json,
                a.field.path(),
                match a.clear {
                    true => Some("--clear".into()),
                    false => a.value.clone(),
                },
            ),
            Command::ChatShow(a) if a.status => C {
                existing: true,
                ..C::request(&Some(a.chat.clone()), a.json, "chat status", None)
            },
            Command::ModelSet(a) if a.chat.is_some() => C::request(
                &a.chat,
                a.json,
                "model set",
                Some(a.model.trim().to_string()),
            ),
            _ => return None,
        })
    }
}

/// To whom `lnk agent chat send --to` sends as the agent: `owner`, or a
/// teammate's name or ID (`teammate:<name>`).
pub fn send_to(args: &ChatSendArgs) -> anyhow::Result<String> {
    match args.to.as_deref() {
        Some("owner") => Ok("owner".into()),
        Some(to) => match to.strip_prefix("teammate:") {
            Some(teammate) if !teammate.is_empty() => Ok(teammate.to_string()),
            _ => anyhow::bail!("--to is owner, or teammate:<name>"),
        },
        None => anyhow::bail!("give --to owner or --to teammate:<name>"),
    }
}

/// The conversation `lnk agent chat send --to` sends in: to the owner,
/// the `link` thread `--chat` names, as every other `--chat` takes it,
/// else `link:main`.
pub fn send_thread(args: &ChatSendArgs) -> anyhow::Result<String> {
    let owner = args.to.as_deref() == Some("owner");
    match (&args.chat, &args.connection) {
        (Some(chat), None) if owner => crate::chat::thread(chat),
        (Some(chat), None) => Ok(chat.clone()),
        (None, Some(_)) if owner => crate::chat::thread(crate::chat::MAIN),
        (None, Some(_)) => {
            anyhow::bail!("--connection chooses where to message the owner: give --to owner")
        }
        _ => anyhow::bail!("give one of --chat <chat> or --connection <connection>"),
    }
}

impl ChatSetArgsField {
    /// The chat command a harness answers that sets this value.
    pub fn path(self) -> &'static str {
        match self {
            Self::Goal => "chat goal",
            Self::Name => "chat rename",
            Self::Verbose => "chat verbose",
        }
    }
}

impl ChatGetArgsField {
    /// The chat command a harness answers that says this value.
    pub fn path(self) -> &'static str {
        match self {
            Self::Goal => "chat goal",
        }
    }
}

impl ChatSetArgs {
    /// Refuses a set that gives no value, or clears one only a goal has.
    pub fn check(&self) -> anyhow::Result<()> {
        match (self.clear, &self.value, self.field) {
            (true, _, ChatSetArgsField::Goal) | (false, Some(_), _) => Ok(()),
            (true, _, _) => anyhow::bail!(
                "--clear clears a goal: give the {} its new value",
                self.field.word()
            ),
            (false, None, _) => anyhow::bail!("give the {} its new value", self.field.word()),
        }
    }
}

impl ChatSetArgsField {
    fn word(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Name => "name",
            Self::Verbose => "verbose",
        }
    }
}

impl ChatSendArgs {
    /// The chat command a harness answers that this send is, by its flag:
    /// `--aside`, `--queue` or `--steer`; none for a plain message.
    pub fn way(&self) -> Option<&'static str> {
        [
            (self.aside, "chat ask"),
            (self.queue, "chat queue"),
            (self.steer, "chat steer"),
        ]
        .into_iter()
        .find_map(|(given, path)| given.then_some(path))
    }
}

/// Where `lnk agent route` sends the agent's traffic: through a SOCKS
/// proxy, through an HTTP proxy on this machine, directly, or nowhere.
/// Each flag takes only its own kind of proxy.
pub fn upstream(args: &AgentRouteArgs) -> anyhow::Result<String> {
    use link_sandbox_contract::net::Upstream;
    let given = [
        args.socks.is_some(),
        args.http.is_some(),
        args.direct,
        args.none,
    ];
    anyhow::ensure!(
        given.iter().filter(|g| **g).count() == 1,
        "give one of --socks <address>, --http <address>, --direct or --none"
    );
    Ok(match (&args.socks, &args.http, args.direct) {
        (Some(address), _, _) => {
            anyhow::ensure!(
                matches!(
                    Upstream::parse(address)?,
                    Upstream::Socks5(_) | Upstream::Socks5Unix(_)
                ),
                "--socks takes a SOCKS proxy (socks5://...), not {address:?}"
            );
            address.clone()
        }
        (_, Some(address), _) => {
            let text = format!("http://{address}");
            anyhow::ensure!(
                matches!(Upstream::parse(&text)?, Upstream::Http(_)),
                "--http takes an address and port, not {address:?}"
            );
            text
        }
        (_, _, true) => "direct".into(),
        _ => "none".into(),
    })
}

/// A team's name, `<team>@<cloud>` for one a cloud hosts.
pub fn hosted_name(team: &str, cloud: Option<&str>) -> String {
    match cloud {
        Some(cloud) => format!("{team}@{cloud}"),
        None => team.to_string(),
    }
}

/// Whether `name` is one of `lnk agent`'s own commands, which no harness
/// can be named.
pub fn agent_command(name: &str) -> bool {
    name == "help" || command().get_subcommands().any(|c| c.get_name() == name)
}

#[cfg(test)]
mod tests;

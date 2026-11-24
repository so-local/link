//! What the user chose, handed to the active harness ([`Settings`]): its
//! model, its runtime and the connections it speaks.

use super::*;

/// What the user chose, handed to the active harness.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Proved action endpoints and their discoverable names, scoped to this agent.
    #[serde(
        default,
        skip_serializing_if = "link_sandbox_client::namespace::Endpoints::is_empty"
    )]
    pub actions: link_sandbox_client::namespace::Endpoints,
    /// The folder every harness works in: the agent's Home
    /// (`~/Link/Agents/<name>/home`, or the folder it was given).
    pub files: PathBuf,
    /// The agent's own folder (`~/Link/Agents/<name>/agent`), for a
    /// harness that keeps Link's collections (Link Harness): each one
    /// kept at `file` a folder of it ([`crate::folder::Folder::of`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<PathBuf>,
    #[duplicate]
    pub model: Model,
    /// A second model (`lnk agent start --model.fallback_model`), tried for one turn
    /// when `model` can't answer it: its runtime is off (it refuses the
    /// connection, or doesn't answer within a time the harness sets), the
    /// turn's prompt is past its window, or its provider answers 429 or a
    /// 5xx first. Reached as `model` is: with keys on the wire, at the
    /// proxy with a placeholder key. A harness that can't fall back ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub fallback: Option<Model>,
    /// What Link and you know of `fallback`, as `model_facts` of `model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_facts: Option<crate::models::Known>,
    // Each connected connection's section, under its name, as its module's
    // `connect` made it: Link's own connections typed here for the adapters
    // that speak them (their shapes are in `docs/architecture/modules/contracts.md`), any
    // other in `connections`. The harness module builds these from its map
    // of connections without knowing them by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub telegram: Option<Telegram>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub slack: Option<Slack>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub discord: Option<Discord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<Signal>,
    /// This start's binding secret, also used for authenticated storage
    /// calls by a harness that keeps Link collections. Keep it in
    /// private configuration, never in `Launch.env`.
    /// For an adapter with an `endpoint`: the key Link's connections send it
    /// (`Authorization: Bearer <key>`). Its endpoint refuses any other
    /// request: other users of this machine can reach its loopback too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub endpoint_key: Option<Secret>,
    /// Where its store bridge listens, when its spec keeps conversations
    /// with a driver (`[storage] conversations`): a harness that keeps
    /// them calls the driver through it ([`crate::store::Bridged`]),
    /// proving each call with `endpoint_key`, and never runs that driver
    /// itself. None: no bridge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<crate::store::BridgeAt>,
    /// Where each of its collections is kept, as its machine resolved
    /// them ([`crate::storage`]), and the storage bridge that reaches
    /// every place off this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<crate::storage::StorageAt>,
    /// The agent's memory (`lnk agent memory`): the address of its MCP
    /// server over HTTP, through the harness's proxy, as in
    /// [`crate::memory::MEMORY_ENV`]. An adapter adds it to its harness's
    /// MCP servers, and removes it when there's none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    /// The tools that run outside it may call (`lnk agent permission add tool`),
    /// by name, each the address of its MCP server over HTTP on its proxy
    /// (`http://127.0.0.1:<port>/_link/url/<secret>/mcp/<tool>`). An adapter adds each
    /// to its harness's MCP servers, with `memory`, and removes those it
    /// added before that are no longer here ([`Settings::mcp_servers`],
    /// [`tools_gone`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, String>,
    /// Managed reader identities verified against their actual definitions.
    /// Their endpoint is private configuration, never execution evidence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_sources: Vec<link_harness_wire::evidence::ReadRegistration>,
    /// The risks its `needs` named ([`Needs::risks`]) that the user
    /// allowed, by name. An adapter leaves out what a risk not here would
    /// open, or refuses to run when it can't.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
    /// It runs with no sandbox at all: the user's choice, or what its
    /// `needs` said. Its ports are then on the machine's loopback, where
    /// every user and program of the machine reaches them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsandboxed: bool,
    /// It runs behind its proxy: sandboxed, with `network` but not `lan`
    /// (or routed through an exit). On Linux its ports other than those
    /// the proxy serves ([`Launch::served`]) then stay in the sandbox's
    /// own network; without it (no sandbox, `lan`, or no network) they're
    /// on the machine's loopback, as they always are on macOS. An adapter
    /// names a risk such a port opens ([`Needs::risks`]) wherever this is
    /// false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub proxied: bool,
    /// The agent's opinions, its spec as it runs it
    /// ([`crate::agent_spec::Resolved`]): how many rounds a turn may
    /// take, how much of the window to use, its default instructions.
    /// A harness that has opinions of its own takes these in their place;
    /// one without any ignores them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<crate::agent_spec::Spec>,
    /// Who the agent is, as its spec says: its persona, owner, standing
    /// instructions and skills ([`super::identity`]), for a harness whose
    /// contract says `identity`. None: the harness's own files stay as
    /// they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<super::identity::Identity>,
    /// What the agent remembers, as notes in ASP's form, each with its
    /// source: Link's notes (`link`) and each harness's exported memory
    /// files (`hermes`, `openclaw`), for a harness whose contract says
    /// `identity` to write into its own memory files those not its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remembered: Option<Vec<crate::asp::forms::Note>>,
    /// The keys of `spec` that are Link's: set by no team of the agent's
    /// and not by you (`window.max`). A harness holds a default of Link's
    /// to what the model takes, and takes a key you set as you set it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defaults: Vec<String>,
    /// What Link and you know of the model ([`crate::models`]): its
    /// window, longest answer, how it thinks and what it costs, and
    /// which of these your models file set. None for a model neither
    /// knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_facts: Option<crate::models::Known>,
    /// What the agent runs with here, as Link resolved it: the connections
    /// it's reached on and what its sandbox file lets it do in its files
    /// folder. A harness may tell its model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    /// Every other connection's section, under its name (a connection module
    /// from outside Link): an adapter that speaks it reads its own.
    #[serde(flatten)]
    pub connections: BTreeMap<String, serde_json::Value>,
}

/// The names in [`Settings`] that aren't connections: no connection may take
/// one.
pub const NOT_CONNECTIONS: &[&str] = &[
    "files",
    "model",
    "endpoint_key",
    "store",
    "storage",
    "agent",
    "memory",
    "tools",
    "allowed",
    "unsandboxed",
    "proxied",
    "spec",
    "defaults",
    "model_facts",
    "environment",
    "fallback",
    "fallback_facts",
    "identity",
    "remembered",
];

/// What an agent runs with here, as Link resolved it at its start
/// ([`Settings::environment`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    /// The connections it's reached on, by their modules' names (`telegram`),
    /// whichever carries them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<String>,
    /// What its sandbox file lets it do in its folder.
    #[serde(default)]
    pub folders: Folders,
    /// The programs its `run` tool may start here: those of its sandbox
    /// file's `[commands]` this machine has, by name, or `*` for any.
    /// Empty: it has no `run`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    /// The teams it's on: who else is on each, and the team's shared
    /// folder, which `folders.teams` says what of it may reach.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<TeamPlace>,
}

/// A team an agent is on, as Link resolved it at its start: its name in
/// the team, the others, and the folder the team shares.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamPlace {
    /// The team's name (`research`) and ID.
    pub name: String,
    pub id: String,
    /// The agent's own name in it, and what it's for on the team.
    pub member: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    /// The others, by their names in it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teammates: Vec<Teammate>,
    /// The folder the team shares, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<PathBuf>,
}

/// Another member of a team: its name in it, its agent's ID, and what it's
/// for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Teammate {
    pub name: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

impl Settings {
    /// The MCP servers an adapter adds to its harness's settings, by
    /// name: each of `tools`, and `memory` as `memory`.
    pub fn mcp_servers(&self) -> BTreeMap<String, String> {
        let mut servers = self.tools.clone();
        if let Some(memory) = &self.memory {
            servers.insert("memory".into(), memory.clone());
        }
        servers
    }
}

/// What an adapter without the `connections` command speaks: Telegram, the
/// only connection before there were more.
pub const FIRST_CONNECTIONS: &[&str] = &["telegram"];

#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    /// A hosted provider (`anthropic`, `openai`, `openrouter`), or a model
    /// runtime on this machine: `ollama`, or `local` for any other
    /// OpenAI-compatible one (LM Studio, llama.cpp, vLLM, ...), as `lnk
    /// models` finds them.
    pub provider: String,
    /// Empty for a local runtime.
    #[serde(default)]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub api_key: Secret,
    /// The model id as the provider names it (`qwen3:8b`, ...). Adapters
    /// add their own prefixes. Unset for a hosted provider: Link's
    /// default for it ([`default_model`]), the same in every harness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A local runtime's address, without `/v1` (`http://localhost:11434`).
    /// For a hosted provider, where to reach it in place of its own
    /// address, as its client takes it: Link's proxy, which adds the key
    /// (then `api_key` is a placeholder).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The granted typed model namespace, when supplied by the agent proxy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions_url: Option<String>,
}

impl Model {
    /// Whether the model runs on this machine (no key).
    pub fn is_local(&self) -> bool {
        matches!(self.provider.as_str(), "ollama" | "local")
    }

    /// The model's id: its own, or Link's default for a hosted provider.
    pub fn named(&self) -> Option<String> {
        self.name.clone().or_else(|| default_model(&self.provider))
    }

    /// The environment variable both harnesses read a hosted provider's
    /// key from.
    pub fn key_env(&self) -> Option<&'static str> {
        match self.provider.as_str() {
            "anthropic" => Some("ANTHROPIC_API_KEY"),
            "openai" => Some("OPENAI_API_KEY"),
            "openrouter" => Some("OPENROUTER_API_KEY"),
            _ => None,
        }
    }
}

/// The model a hosted provider gets when the agent names none, in every
/// harness: named, so you know what it costs, and the same when you
/// switch. An opinion, so Link's spec's (`[model.<provider>] default`,
/// [`crate::agent_spec::LINK_SPEC`]); an agent's own spec may name
/// another, which the harness module hands its harness as the model's
/// name.
pub fn default_model(provider: &str) -> Option<String> {
    let link = crate::agent_spec::Resolved {
        spec: crate::agent_spec::link(),
        from: Default::default(),
    };
    link.model_for(provider).default
}

/// A model runtime running on this machine, as `lnk model runtime list --json` lists
/// it (the models module).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Runtime {
    /// What usually runs at this address: `Ollama`, `LM Studio`, ...
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
    /// Whether the owner added it (`lnk model runtime add`), rather than Link
    /// finding it at a runtime's usual port.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub added: bool,
}

#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Telegram {
    /// Empty in an agent's settings file on a Mac, where it is in the
    /// Keychain.
    #[serde(default)]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub bot_token: Secret,
    /// Telegram user ids allowed to talk to the bot. Everyone else is
    /// refused.
    pub allowed_users: Vec<i64>,
    /// Where the harness reaches Telegram's Bot API in place of
    /// `https://api.telegram.org`, when set: its proxy
    /// ([`crate::sandbox::secret_url`], by `lnk-telegram rule --json`),
    /// which adds the real token.
    /// `bot_token` is then a placeholder. An adapter whose harness speaks
    /// Telegram must use it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_root: Option<String>,
}

/// A Slack app of the user's own, in Socket Mode: the harness connects out
/// to Slack, so it needs no public address.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Slack {
    /// Its bot token (`xoxb-...`), which reads and posts messages. Empty
    /// in an agent's settings file on a Mac, where it is in the Keychain.
    #[serde(default)]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub bot_token: Secret,
    /// Its app-level token (`xapp-...`), which opens its Socket Mode
    /// connection. Empty on a Mac, as `bot_token`.
    #[serde(default)]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub app_token: Secret,
    /// Slack member ids (`U01ABC2DEF3`) allowed to talk to it, in direct
    /// messages. Everyone else is refused.
    pub allowed_users: Vec<String>,
    /// The id of the owner's direct conversation with the bot (`D...`),
    /// where their pairing code arrived: the only one a message nobody
    /// asked for goes to. Absent for an app connected by a Link that
    /// didn't keep it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dm_channel: Option<String>,
}

/// A Discord bot of the user's own, which they talk to in direct
/// messages (it shares a server with them).
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Discord {
    /// Its bot token. Empty in an agent's settings file on a Mac, where it
    /// is in the Keychain.
    #[serde(default)]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub bot_token: Secret,
    /// The bot's user id, which Discord's `/users/@me` answered at
    /// connect: how it knows its own messages and mentions without
    /// reading its token.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bot_id: String,
    /// Discord user ids (`200000000000000002`, as strings: they don't fit
    /// every JSON reader's numbers) allowed to talk to it. Everyone else
    /// is refused.
    pub allowed_users: Vec<String>,
    /// The channel id of the owner's direct message with the bot, where
    /// their pairing code arrived: a harness's home channel. Absent for a
    /// bot connected by a Link that didn't keep it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dm_channel: Option<String>,
}

/// A Signal number of its own the harness links to as a device, as Signal
/// Desktop does, through `signal-cli` ([`crate::signal`]). What links it is
/// the harness's, in its home; there is no token here.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    /// The agent's number (`+15557654321`), the account `signal-cli` links.
    pub account: String,
    /// Phone numbers allowed to talk to it, `+` and digits. Everyone else
    /// is ignored.
    pub allowed_users: Vec<String>,
    /// The `signal-cli` the harness runs, installed in its home by
    /// `lnk-signal cli install`: a wrapper keeping the account in the home
    /// and passing the proxy on. Where it is, and what of the home is the
    /// account, are `lnk-signal`'s to say (`cli install` prints both),
    /// never a harness's to know.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<PathBuf>,
}

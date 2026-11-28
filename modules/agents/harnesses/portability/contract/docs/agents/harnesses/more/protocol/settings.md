# Settings

What a harness program is told of the agent it runs, and what it says
its harness needs: `lnk-<name> configure` writes the harness's own
settings from the agent's configuration, and `lnk-<name> needs` reads them to say what
the harness needs beyond Link's defaults. `lnk-agent` runs both.

## `lnk-<name> configure`

Reads `link_harness_contract::harness::Settings` as JSON on stdin
(`{"files", "agent"?, "model": {"provider", "api_key", "name"?,
"base_url"?}, "<connection>"?: {...}, "endpoint_key"?, "store"?,
"storage"?, "memory"?, "tools"?, "read_sources"?, "allowed"?, "unsandboxed"?, "proxied"?,
"spec"?, "defaults"?, "model_facts"?, "environment"?, "fallback"?,
"fallback_facts"?, "identity"?, "remembered"?}`) and writes the
harness's own settings, leaving out a connection it doesn't speak. It
prints nothing unless it fails.

### Each connection's section

Each connected connection's section is under its name, as its module's
`connect` made it, typed in `link_harness_contract::harness`. Link's
own are `"telegram": {"bot_token", "allowed_users", "api_root"?}`,
`"slack": {"bot_token", "app_token", "allowed_users", "dm_channel"?}`,
`"discord": {"bot_token", "allowed_users", "dm_channel"?}` and
`"signal": {"account", "allowed_users", "program"?}`, `program` the
`signal-cli` `lnk-signal cli install` put in the home.

### A connection from outside Link

A connection from outside Link is in `Settings::connections`, and no
connection is named as one of the settings' own keys, such as `files`,
`model`, `endpoint_key`, `store`, `storage`, `memory`, `tools` or
`spec`.

## Keys and bridges

What the adapter is handed to reach the harness and the agent's things:
its endpoint's key and the storage bridges.

### `endpoint_key`

For an adapter with an `endpoint`: what its endpoint takes
(`Authorization: Bearer <key>`), refusing anything else, and a
connection Link carries for it is left out of the settings. It's also
the agent's key for this start, which proves the agent's calls to its
[bridges](../../../../../../../../../security/sandbox/bridge/docs/tooling/sandbox/more/bridges.md).

### `store`

For an agent whose specification keeps its conversations with a driver:
`{"driver", "socket"}`, where the agent's store bridge listens
(`link_store_contract::store::BridgeAt`). A harness that keeps
conversations calls that driver through it
(`link_store_contract::store::Bridged`) and never runs it itself.

### `storage`

Where the agent's machine resolved each of its collections is kept as `link_store_contract::storage::StorageAt`, `{"places": {"<collection>": {"place", "overridden"?}}, "socket"?}`, for each collection the [Agent Specification](../../../../../../../../agent/contract/spec/docs/config/specs/guides/agent.md#storage) names.
Left out, each is in the files folder.

### A collection kept elsewhere

A collection at a place other than `file` is the driver of that
place's, reached only through the storage bridge at `socket`: its files
with the `file_*` calls, its conversations with the store's calls, and
the tools each was narrowed to with `tools_read` and `tools_narrow`.
The driver owns its physical storage: SQLite can keep it on this
machine, while a bucket keeps it elsewhere. The harness reaches it
through typed bridge calls.

### What the harness holds in memory

The driver persists the conversation. The harness's transient turn
controls stay in memory; choosing a driver does not promise that no
conversation bytes reach this machine's disk.

### A collection at `file`

One at `file` is in the agent's own folder (`agent`, `agent/`), kept as
`lnk-file-store` keeps it, which the harness reaches itself, as its
sandbox lets it.

### How Link hands it

[`lnk agent start`](../../../../../../../../agent/docs/agents/README.md#start-your-agent) hands it on every machine to a harness that keeps
Link's collections (its contract says `conversations`), each place
resolved from the agent's `[storage]` and the machine's `~/.lnk/safe/agents/policy.toml`.
It installs the needed drivers and serves the bridge, each call
answered by the driver of the
place its collection is kept at
(`link_store_contract::store::serve_storage`, `Request::collection`).

### `telegram.api_root`

With keys on the wire: where the harness must reach Telegram's Bot API,
its proxy, which adds the real token to `bot_token`'s placeholder
(`lnk-telegram rule --json`).

## Tools and grants

The agent's tools, and what the harness may do beyond Link's defaults.

### `memory`

The address of the agent's memory, an MCP server over HTTP through the
harness's proxy, which [`lnk agent memory`](../../../../../../../../tools/memory-tool/docs/agents/tools/memory/guide.md#inspect-memory) inspects, there only while the agent has
one and its harness runs behind its proxy.

### `tools`

Each tool that runs outside the harness that it may call (`lnk agent
permission add --tools.names`), by name, at its address on its proxy, an MCP server
over streamable HTTP. The adapter adds each, and `memory` as `memory`,
to its harness's MCP servers (`Settings::mcp_servers`), and removes
those it added before that are gone
(`link_harness_contract::harness::tools_gone`).

### Namespaces

An agent's own tools reach its harness as namespaces, so its model calls
`task__run`, `cron__create` or `agent__send` wherever the agent runs.
Each namespace is one MCP server among `tools`, at
`$LNK_TOOL_<NAMESPACE>`, one each for `task`, `cron`, `tool`, `file`,
`agent`, and `question`.

### Providers

A namespace does nothing of its own. Each action forwards a call to a
provider, the tool that does the work: Link run, Link scheduler, Link
file, Link code, or the agent's context service. A provider keeps its
own name (`scheduler-<agent ID>`), confinement, and approval rules, and
the harness is never handed it directly.

### Each action's providers

The contract crate's [`namespaces.toml`](../../../../../namespaces.toml)
declares every namespace, its actions, and the provider's own tool each
action calls. A target's `kind` names the provider: `run`, `schedule`,
`file`, `code`, or `context`. This is `cron__create`, which calls the
scheduler's `schedule`:

```toml
# namespaces.toml
[cron.create]
targets = [{ kind = "schedule", native = "schedule" }]
```

An action with two targets takes a `kind` argument that picks one.
`select` keeps that argument with one target, and `broadcast` lets a
call without it reach both, as `task__list` does. `otherwise` names
targets reached only where none of `targets` is served. That keeps
`agent__send` one action, sent by the context service on a machine and
by the scheduler's own `send` where no context service runs.

```toml
# namespaces.toml
[agent.send]
targets = [{ kind = "context", native = "send" }]
otherwise = [{ kind = "schedule", native = "send" }]
```

`harness = true` marks the harness's own call. The namespace server
lists it to no model and checks it as any other call when the harness
makes it. Link Harness keeps a quiet conversation's compaction this way,
through `cron`'s `compact`, as
[Decisions](decisions.md#why-may-a-namespace-hold-a-call-no-model-is-offered)
explains.

### Building a namespace

`link_harness_contract::harness::namespaces::actions` reads the table
for one namespace, given which tool serves each kind there. It leaves
out a target whose kind has no provider, and an action with none left.
A namespace with no action is not handed at all. The agent module
builds its namespaces this way on the owner's machine, with every kind
served, and any other place that runs an agent does the same with the
providers it has. The namespace server, `lnk-sandbox mcp-namespace`,
checks each provider's current definition and approval policy before
every call.

### `read_sources`

Managed read registrations identify a host-verified File or Memory process
and its read operation. The harness matches the final server address and
operation before using that identity. Missing, changed, custom, or
duplicate matching registrations leave the service's claim unverified.
The endpoint is private configuration and never enters execution evidence.
Registration identifies the reader. A validated content receipt and the
final model view establish what it supplied and whether delivery was whole.

### `allowed`

The risks its `needs` named that the user allowed, by name. The adapter
leaves out what any other would open, or fails, saying how to allow it,
when the harness can't run without it.

### `unsandboxed`

`true` when the harness runs with no sandbox at all, its ports on the
machine's loopback.

### `proxied`

`true` when it runs behind its proxy: sandboxed and not on
`[network] mode = "host"`, or routed through an exit. Denying `[internet]`
retains the proxy for local services while refusing outbound hosts. On Linux its
ports other than those its proxy serves (`Launch.serve`) then stay in
the sandbox's own network; left out, they're on the machine's loopback,
as they always are on macOS, and an adapter names a risk such a port
opens.

## Opinions and models

The agent's specification, and what Link knows of its model and its fallback.

### `spec`

The agent's opinions, its
[specification](../../../../../../../../agent/docs/config/specs/agent.md) as it
runs it (Link's, its teams' and its own), which a harness takes in
place of any of its own. `model.name` is already its specification's default,
and one its `allow` lists.

### `defaults`

The keys of `spec` that are Link's, set by no team and not by the user
(`["window.max", ...]`): a harness holds a default of Link's to what the
model takes, and takes a key the user set as set.

### `model_facts`

What Link and the user know of `model.name`
(`link_model_contract::models::Known`, `{"name", "window"?, "answer"?,
"thinking"?, "price"?: {"input", "output", "cached"?}, "yours"?}`), left
out for a model neither knows, as [`lnk model show`](../../../../../../../../model/docs/agents/models/guide.md#where-things-live) prints it.

### What each fact is

`window` and `answer` are the model's whole window and longest answer
in tokens, `thinking` how it's asked to think (`adaptive`, `budget`,
`effort` or `no`), `price` dollars a million tokens, and `yours` which
of these the user's `~/.lnk/safe/models/facts.toml` set.

### `fallback`

A second model (`lnk agent start --model.fallback_model`), in `model`'s shape and
reached as it is, with keys on the wire at the proxy with a placeholder
key. A harness tries it for a turn `model` can't answer: its runtime
off, the turn's prompt past its window, or its provider busy or failing
(429, or a 5xx) before any of its answer. A harness that can't fall back
ignores it.

### `fallback_facts`

What Link and the user know of the fallback model, in `model_facts`'s
shape, left out for a model neither knows.

## Who it is and where it runs

Who the agent is, and what it runs with on this machine.

### `identity` and `remembered`

[Who the agent is and what it remembers](contract.md#who-the-agent-is), for a harness whose contract
says `identity`.

### `environment`

What the agent runs with here, as Link resolved it
(`link_harness_contract::harness::Environment`, `{"connections"?,
"folders": {"write", "read", "teams"?}, "commands"?, "teams"?}`), for a
harness to tell its model. `connections` names the connections it's
reached on, by their modules' names.

### Its folders

`folders` is its sandbox file's `[files]`: the folders it may write and
those it may only read, by name (`link_harness_contract::harness::layout`:
Home's `work/` and `memory/`, and the own folder's `instructions/`,
`skills/` and `chats/`; `.` all of Home), and of each of its
teams' shared folders (`folders.teams`, `{"<team>": {"write"?,
"read"?}}`, folders' names in it or `.` for all of it).

### Its programs

`commands` names the programs its `run` may start here, those of its
sandbox file's `[commands]` this machine has, by name (`["sh",
"python3"]`, or `["*"]` for any). Left out, it has no `run`.

### Its teams

`teams` is the teams it's on, `[{"name", "id", "member", "purpose"?,
"teammates"?: [{"name", "id", "purpose"?}], "shared"?}]`: its own name
in each, the others by name and agent ID, and the shared folder where
it's granted some of it. A harness whose sandbox Link makes is given
its teams' folders as granted, whatever harness it is.

## `lnk-<name> needs`

Reads `link_harness_contract::harness::Settings` on stdin as
`configure` does, but with no key or token (the model's `api_key` and
each connection's secrets empty), and prints
`link_harness_contract::harness::Needs` as JSON: `{"needs"?: [{"spec",
"why"}], "unsandboxed"?: {"why"}, "risks"?: [{"name", "why",
"without"?}], "install_runs_downloads"?: true}`. Read by `lnk-agent`.

### What it says it needs

What its harness needs beyond Link's defaults with those settings, each
with why in its own words, one line of at most 200 characters. The
settings carry `unsandboxed`, `true` when the user runs it with no
sandbox, and `proxied`, with the grants it has so far, so a harness
that asks for `[network] mode = "host"` itself counts it.

### Each need is a specification

Each of `needs` is a [sandbox's specification's](../../../../../../../../../security/sandbox/docs/config/specs/sandbox.md) sections,
such as `{"spec": {"network": {"mode": "host"}}, "why": "Slack's
websocket ignores Link's proxy (HTTPS_PROXY)"}`.

### The programs it runs

Its harness runs no program but the one its launch names (and that
one's `#!` interpreter) and those `[commands]` allows: a harness that
starts another (`node`, `git`, a program it installed) names it,
`{"spec": {"commands": {"allow": ["node"]}}, ...}`, or any, `["*"]`. Its
commands that read the agent's files (`instructions`, `chats`)
run only its own program and the store drivers it's handed.

### What a need may not say

Each need is a specification with no `name`, `version`, relative path or limit
of calls (`per_minute`, the agent's own), opening something on some OS,
and taken as it is on this OS: one that opens nothing here is passed
over. It's shown and taken back as the flags [`lnk agent permission add`](../../../../../../../../agent/docs/tooling/sandbox/permissions/README.md#choose-what-your-agent-can-reach) takes
(`--network.mode host`), and a key Link doesn't know is refused, the
harness not started.

### `unsandboxed`

`unsandboxed` is only for a harness that can't run in any sandbox: it's
never asked or granted, and a start is refused unless the agent runs
with no sandbox (`lnk agent start --no-sandbox`).

### `risks`

`risks` names what running it risks that no specification does (a port of its
own that asks for no credential), each by a name of its own: lowercase
letters, digits and dashes, at most 32, and none that `unsandboxed` or
a name of a sandbox setting (`network`, `lan`, ...).

### `install_runs_downloads`

`install_runs_downloads` is `true` for a harness whose `install` runs
programs it downloads into its own folder, such as a Node or uv it
unpacks there and then runs. The sandbox runs nothing in a folder the
run writes, so without it that install is refused. With it, Link lets
the `install` step alone run what it writes in the folder it installs
into, and nothing else that step writes. Link says so once as it
installs. Once installed, that folder is read only to every run, the
harness's included. Left out, it's `false`.

### A risk it runs without

One with `without`, a line saying what it does without it, runs either
way, and only a yes in a terminal allows it. One without `without` is
asked as a need is. `configure` gets those allowed in `allowed`.

### How each is asked

[`lnk agent harness set`](../../../../../../../../agent/docs/agents/harnesses/use/README.md#switch-in-one-command), a start and [`lnk agent move`](../../../../../../../../agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) ask the user for each not
answered yet, and grant what they allow to the harness
(`~/.lnk/safe/agents/harness-grants/<name>.toml`), held only while it runs, remembering
the answer. What the agent has of its own (`lnk agent permission add`) is had,
and not asked.

### An adapter without it

An adapter without `needs` needs nothing more: one that exits with an
error (its answer to a command it doesn't know) or prints nothing. An
answer that isn't `Needs` is refused.

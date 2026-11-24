# Harness Contract

What a harness program says of itself when asked `lnk-<name> contract`:
the version of this contract it speaks, and each part of the contract
its harness takes. Read by `lnk-agent`, which asks once a process.

## `lnk-<name> contract`

Prints `link_harness_contract::harness::Contract` as JSON: `{"version",
"proxy", "proof"?, "agent"?, "ports"?, "instructions"?,
"from"?, "recover"?, "tools"?, "parts"?, "folders"?, "prompt"?,
"conversations"?, "shared"?, "blank"?, "acp"?, "state"?, "notes"?,
"native_memory"?, "identity"?, "features"?}`. Each key left out is no,
unless it says otherwise.

## Its version and its model

What contract it speaks, how it reaches its model, and how many ports
it takes. The `version` is the version of this contract it speaks, the
harness protocol's (`harness/protocol.toml`): 2. [`lnk agent start`](../../../../../../../../agent/docs/agents/README.md#create-an-agent)
refuses an adapter stating one below the protocol's `oldest`, by name,
naming [`lnk upgrade`](../../../../../../../../../../docs/more/modules/guide.md#upgrade).

### `proxy`

Whether its harness reaches a hosted model at `model.base_url`, Link's
proxy, which then holds the key: keys on the wire, by default.

### `ports`

How many ports from `LNK_PORT` its harness takes. Link gives the agent
that many and one more, for its proxy on a Mac; left out, 200.

## Its endpoint

What [its `endpoint`](endpoint.md) does beyond answering a message.

### `proof`

Whether its `endpoint` proves it's the harness before it's sent the
key, answering a challenge with an HMAC made with the key.

### `agent`

Whether its `endpoint` reads which agent a request is for, so one
process can answer for many agents.

### `from`

Whether its `endpoint` reads who a message is from. Left out, it's
never sent a message from anyone but the owner, and gets no scheduler.

### `recover`

Whether its `endpoint` hands over the answers of turns it finished
after a crash. Left out, they stay in the conversation.

### `tools`

Whether its `endpoint` keeps a conversation's tools. Left out, a
subagent has all its parent's tools, and the bridge refuses to send a
list.

### `parts`

Whether its `endpoint` takes a message's images and documents as
OpenAI's content parts. Each connection Link carries then downloads the
photos and documents a message holds, PNG, JPEG, GIF or WebP images and
PDF or text documents of at most 10 MiB and 8 a message
(`link_connection_contract::{MEDIA, PART_MAX, PARTS_MAX}`).

### How the parts are sent

They're sent as `image_url` and `file` parts with `data:` URLs, the
message's text naming each one left out and why. Left out, none is ever
sent, and a message holding only them is answered that the agent reads
text only.

## Its folders and conversations

What of the agent's folder it keeps to, and what it answers of the
agent's prompt and conversations.

### `folders`

Whether it keeps to the agent's folder's layout: it writes only
`chats/` of the agent's own folder itself, so its sandbox reads
Home and the own folder's `instructions/` and `skills/`, and writes
only its `chats/` and the folders the agent's sandbox file's
`[files] write` names. With it, and its sandbox on, the agent gets
Link's `run` and `file`.

### Without it

Left out, its sandbox writes the whole of Home and never reaches the
agent's own folder, and a start says `[files]` holds only for Link's
tools.

### `instructions`

Whether it takes the agent specification's `[instructions]`. Left out, a specification
setting them is refused for it.

### `prompt`

Whether it [answers `instructions`](conversations.md).
Left out, [`lnk agent show --instructions`](../../../../../../../../agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks) says it can't.

### `conversations`

Whether it answers [`conversation`](conversations.md) and `conversations move`. Left out, [`lnk agent chat show`](../../../../../../../../agent/docs/agents/chat/README.md#talk-to-it-here)
and [`lnk agent chat export --format`](../../../../../../../../agent/docs/agents/chat/README.md#manage-history) say it can't.

## How it runs

Two ways one process of it may run: for many agents of one owner, or
started ahead with nothing of anyone's.

### `shared`

Whether one process of it may serve several agents of one owner:
`<command> --agents <file>`, its `command`'s argv with those two added,
serves each agent `link_harness_contract::harness::SharedAgents` names,
on its own port, as that agent. Link runs it so only where
`~/.lnk/safe/agents/policy.toml` says `shared_harness = true`, in one sandbox whose policy
includes those agents' runtime folders and individual proxy routes.

### What each agent is served with

`SharedAgents` is `{"agents": [{"name", "id", "owner", "home", "files",
"agent"?, "port", "env"}], "schema": 1}`, naming its schema: each
agent's own home, Home and own folder (`agent`; left out, everything in
`files`), and its tools from `env`, the environment it would have had
alone.

### `blank`

Whether it can start blank, for a machine that runs an agent only while
it answers: `<command> --control <control>`, its `command`'s argv with
those two added, loads nothing of any agent's and answers on no port,
listening on the Unix socket `<control>`, mode 0600, outside its
sandbox, until one line of `link_harness_contract::harness::Bind`
arrives.

### The binding

`Bind` is `{"agent": {"name", "id", "owner", "teams"?}, "key",
"settings", "store"?, "files"?}`: the agent, a key made for this binding
alone, its endpoint's and the one it proves its bridge calls with, its
settings as `configure` takes them with a placeholder for every secret,
and the sockets of its store bridge and files bridge.

### Its answer

It answers `{}` once it answers as that agent alone, on its usual port,
or `{"error"}` and stays blank (`link_harness_contract::harness::bind`).
A second `Bind` is refused, and nothing unbinds it: the process ends
when the agent sleeps.

## Without a contract

An unsuccessful optional `contract` command supplies no capabilities,
represented by the default version-zero record. A successful command
must return the typed, supported contract. Malformed output and unsupported
versions, including an explicitly returned version zero, refuse the read.
The adapter’s name never supplies inferred proxy support.

### Who reads the version

`lnk-agent` parses and checks successful capability output against the
harness protocol before caching or using it. Repeated reads retain that
typed success or its error. A failure to run the adapter is also an error;
it is distinct from an optional command exiting unsuccessfully.

## Features

A contract's `features` lists the features its harness honors, each by
its id (`link_harness_contract::harness::features::Feature`), as
`"spec"`, as the agent's specification says (it reads the feature's keys, or
Link does for it, as for the connections it carries), or `"native"`,
its own way, which Link leaves to it: `{"repo-instructions": "native",
"groups": "spec"}`.

### A specification it doesn't honor

`lnk agent start` refuses a specification whose own keys, or its profile's, turn
on [a feature the list doesn't hold](../../../../../../../../agent/contract/spec/docs/config/specs/guides/features.md), naming the feature and its key.
A feature or a way this Link doesn't know is left out of the list,
never the whole contract refused.

### Left out

Left out, the list is what the contract's flags say: `instructions`,
`repo-instructions`, `persona` and `skills`; `parts`, `images` and
`attachments`; `from`, `subagents`, `schedules` and `proactive`;
`folders`, Link's tools, `shell`, `file-read`, `file-edit`,
`code-search`, `diffs` and `lsp`; and `groups` and `streaming` for
every harness.

### Each harness's

Only a harness that tells its calls over the Agent Client Protocol (ACP) (`tool_call`) honors
`activity`. Link Harness honors every feature as the specification says. Hermes
and OpenClaw honor `persona` and `skills` as the specification says (their
`identity`) and the rest natively, and DeepSeek TUI its own natively,
with `groups` and `streaming` as the specification says on the connections Link
carries to it.

## What a switch leaves behind

A contract's `state` says what its endpoint's agent state exports and
imports over the Agent State Protocol (ASP), `notes` whether it reads the agent's notes, and
`native_memory` whether it keeps memory files of its own.

### `state`

`{"export", "import"}`, each a list of ASP's kinds (`sessions`,
`instructions`, `skills`, `tools`, `memory`, `settings`, `schedules`), as its
`initialize` advertises `agentState`. All four personal adapters give
and take portable sessions. Taking a session may preserve its archive
and supply bounded context while reporting that native state cannot
be reconstructed. Left out, none carries across this endpoint.

### `offline_state`

`true` says `state export` reads complete portable state while stopped,
including native history through the adapter's codec. Link uses it before
switching from a stopped harness. A failed live capture always refuses
the switch rather than silently substituting an older snapshot.

### `notes`

`true` says the harness reads the agent's notes (`memory/notes.md`) at
each turn, where a switch writes what the agent remembered on other
harnesses: Link Harness's.

### `native_memory`

`true` says the harness keeps memory files of its own, which no other
harness reads.

### What `lnk agent harness set` lists

[`lnk agent harness set`](../../../../../../../../agent/docs/agents/harnesses/use/README.md#switch-in-one-command) lists what the destination won't carry before it
switches (`Contract::left_behind`): its conversations unless the one
it leaves exports them and the other takes them in, the harness it
leaves's own memory files unless it carries
who the agent is and the other reads them, its skills and persona
unless both read them from the agent's specification, and each feature the
agent's specification turns on that it honors and the other doesn't
(`Contract::not_honored`).

### Agreeing to it

`use` asks, or `--yes` agrees ahead; a no, or no terminal to ask in,
changes nothing.

## Who the agent is

A contract's `identity`, `true`, says the harness carries who the agent
is both ways (`link_harness_contract::harness::identity`).

### Into the harness

`configure`'s settings hold `identity`, `{"persona"?, "owner"?,
"standing"?, "skills"?}`: each text the agent's specification turns on, read from
the agent's own folder, and its skills' names, which Link copies into
the harness's home at `.lnk/safe/agents/places/skills/<name>/` before it, the folder
there even when empty.

### Into its files

The adapter writes the texts into a block of its harness's own files,
between `<!-- link:begin ... -->` and `<!-- link:end -->`
(`identity::write_block`), the rest of each file kept, and points the
harness's skills setting at the copy.

### Out of the harness

`lnk-<name> state export` prints what the harness holds of its own in
ASP's portable forms (`link_harness_contract::asp::State`,
`{"instructions"?, "skills"?, "memory"?}` for identity-only adapters,
with sessions and named omissions for `offline_state`), and `lnk-<name> state`
alone still prints the paths [`lnk agent move`](../../../../../../../../agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) carries.

### What it exports

One `AGENTS.md` of its files' text outside Link's block that isn't the
harness's own default, each text under the heading `Identity::all`
gives it (`## Who you are`, `## Your owner`, `## Standing
instructions`); each skill it made as an Agent Skills folder, `{"name",
"files"}`, its text files by their paths; and its memory files as
notes, `{"path", "text", "source"}`.

### Taken in a switch

At `lnk agent harness set` away from it, once the switch is agreed, Link writes
each text into the agent's file where that file is missing or empty
(`Identity::of_agents_md`), and adds each skill whose name the agent
has none of, saying what came. What it remembered there is kept with
the rest of its state, in the agent's folder.

### Into its memory

`configure`'s settings hold `remembered`, `[{"path", "text",
"source", "deleted"?}]`: what the agent remembers, as ASP's notes, Link's notes
(`source` `link`) and each harness's memory as a switch away from it
left it (`hermes`, `openclaw`). Deleted revisions remain sourced
tombstones, including the source's original path.

### Kept apart from its own

The adapter writes those not its own into its harness's memory files,
marked as Link's, and its export leaves what it wrote out, so nothing
comes back as the harness's own.

### Corrections on Return

The shared memory reconciler compares actual native text with the last
export before applying its source's correction or deletion. Divergent
native edits refuse configuration. Complete foreign documents stay under
`memory/portable/harnesses/`, while eager notes can be excerpts. Rebuilding
generated blocks removes deleted material instead of appending old copies.

## Session content

`session_content` lists non-text content the harness can use after a
session import: `tools`, `thinking`, `images`, `audio`, `resources` or
`other`. Left out, session support promises text only. Link names actual
content outside that list before a switch, retaining the source snapshot.

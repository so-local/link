# Agent Configuration

An agent's configuration is its specification, which describes the agent, and this machine's settings, which say what any agent here may do. Inspect what the agent uses at startup before you edit either.

## Inspect the Configuration

Inspect the resolved choices before editing an agent's TOML file. The command shows the opinions that apply to the selected agent, including defaults.

```sh
lnk agent spec show
```

Each value says whether it comes from Local Link, a built-in profile, a team, or
your own specification. Expanding a built-in profile into the agent's file keeps
the profile's default values attributed to that profile.

## This Machine's Rules

Machine-wide policy lives in `~/.lnk/safe/agents/policy.toml`. `[storage]` says,
for each of an agent's collections, the places this
machine supports, its default first. An agent's own `[storage]` [is a
request](../../../../../../../docs/config/README.md#where-an-agents-things-are-kept): it holds where the machine lists it, and the collection is
kept at the machine's default otherwise.

### Know Who Writes the Rules

Only a person or their script writes the `[storage]` section. Installation identity is
kept separately, and no agent's sandbox reaches the policy file.

### Restrict the Sandbox

`unsandboxed = false` forbids running an agent without a sandbox on
this machine. Computers and boxes both otherwise accept an explicit
`--no-sandbox`; sandboxed remains the default. The policy file and its
ancestors must be trusted, including a symlink's target. Malformed or
untrusted policy is refused rather than treated as permission.

### Share Processes

`shared_harness = true` shares one Link Harness process across an owner's
agents on Linux. `shared_bridge = true` shares one bridge across agents.
Both default to false and remain outside each agent's writable sandbox.

### Agents' Ports

`ports_from` sets where agents' blocks of ports start on this machine:
a new agent's, one arriving from another machine, and one moving off
ports another program holds. It is 20000 by default and accepts a port
from 1024 to 31768, which leaves a whole block below the system's range
for outgoing connections. Several machines' agents sharing one
computer's loopback, such as test fixtures standing in for boxes, each
take a start of their own.

### Keep Moved Files

`keep_moved` controls how long a moved agent's files, and a deleted
agent's archive, remain here. The default is
`"30d"`. The setting accepts a number of days ending in `d`, and and accepts
`"never"` to keep the files until you remove them.

### `home`, `instructions`, `skills`, `memory`, `conversations`, and `schedule`

Each is a list of places, at least one, no place twice, the default first. [The places](../../../../contract/spec/docs/config/specs/guides/agent.md#storage) are
Local Link's `file`, `sqlite`, and `bucket`, or
a place whose driver this machine has. Left out, the list holds Local Link's places that
serve the collection, `file` first.

### Avoid a Place That Can't Serve a Collection

Local Link refuses a place for a collection its driver doesn't serve, such as `home` on
`sqlite`, when it reads the file.

### Install a Missing Driver

A place listed whose driver isn't installed installs at an agent's
start, as a module a feature needs does.

### Run Agents Only While They Answer

A machine that runs agents only while they answer lists, for every collection, only places kept off the machine. An
agent woken there reads its instructions, skills, and conversations as it
left them before it slept.

## Agents (`lnk agent`)

[Agent Files](../../state/files.md) lists every file `lnk agent` keeps for an agent, its harness, and its moves. An agent's folder, specification, and grants are
below.

### Pass Variables to the Bridge

`lnk agent start` passes each `LNK_<NAME>_API` and `_CLI` variable it's
run with (never one naming a key, token, secret, password or auth) to
the agent's bridge service, or the one bridge every agent shares, for a
connection module's tests.

### Find What Each Command Reads

[`lnk help <command>`](../../../../../../../docs/more/modules/guide.md#get-help) lists the environment variables each command reads,
as [`module.toml`](../../../../module.toml) declares them.

## An Agent's Folder

`~/Link/Agents/<name>/` (700) holds `home/`, the person's files, and
`agent/`, the agent's own files, open to the person. Its specifications are in
`~/Link/Safe/Agents/<name>/`, outside the agent's writable tree. `lnk agent start
--run.files <dir>` makes another folder its Home (a repository it works in);
`agent/` stays here.

```text
~/Link/Agents/<name>/
  home/                 the person's: documents, work
    memory/             documents memory searches, the person's to edit
  agent/                the agent's: its brain, open to the person
    instructions/       its instructions
    skills/
    memory/             its notes and memory's index
    chats/
    schedule/
```

### Reach Each Collection One Way

[Each collection](../../../../contract/spec/docs/config/specs/guides/agent.md#storage) of the agent's
kept on `file` is one folder here, so nothing has two ways in. A collection kept
elsewhere has no folder here, and the storage bridge is the only way to reach it.

### `home/`

The `home` collection: the person's documents and the agent's work
(`home/work/`, anything else it makes). `home/memory/` is the documents
its memory searches: Link Memory indexes this folder and the
conversations, and nothing else of `home/`. Read only by default.

### `agent/instructions/`

The `instructions` collection, plain files you edit, and the agent too
when you ask it, with [`[files] write`](../../../tooling/sandbox/permissions/README.md#write-and-read).
`instruction.md`, optional, is your standing instructions, a section of
Link Harness's prompt, read at each message (`lnk agent show --instructions`
prints the prompt).

#### Who it is, and you

`persona.md` and `owner.md`, optional: who it is and who you are, two
sections more. `models/<maker>/<model>/instruction.md`, optional: yours
for that model, the model named as OpenRouter names it, another section
of it.

### `agent/skills/<name>/SKILL.md`

The `skills` collection, plain files you and the agent edit: a skill, as the
Agent Skills standard writes one. `lnk agent skill add` copies its
folder here, and Link Harness lists its name and what it's for in its
prompt.

### `agent/memory/` (700)

The `memory` collection: `notes.md`, the agent's notes, written by
`remember` up to `[tool.memory] notes` and read into its prompt; and
`<engine>/`, its memory engine's index and what else it keeps, such as
Link Memory's `index.sqlite`, its words in `index.sqlite.words` and its
vectors in `index.sqlite.codes` and `index.sqlite.small`.

### `agent/chats/<thread>/`

The `conversations` collection: Link Harness's chats, each
`events.jsonl`, a line per message, only added to, and
`conversation.md`, a copy to read; beside them `tools.json`, and while a
turn runs `turn.json`.

### `agent/chats/events.sqlite` (600)

The collections the agent keeps on `sqlite` (`lnk-sqlite-store`), as rows,
with their leases: in `chats/`, the one folder of `agent/` its
harness writes, as a driver run in its sandbox writes it.

### `agent/schedule/` (700)

The `schedule` collection, as `lnk-file-store` keeps it: its jobs
(`jobs/`, `firing/`) written by its scheduler's tool and fired by its
bridge.

### Grant Write Access

[The agent's sandbox file](#sandbox-permissions) grants what the agent may write of its own folder, or another agent's.

## Know What Home Keeps

An agent is laid out as two folders wherever it is made, comes in a move or returns from a backup. Nothing is moved from Home into `agent/`: what Home holds, a repository's `skills/` or a folder the agent wrote there, stays Home's, and a move leaves such a folder behind, saying so.

## Saved Agent Specification

`~/Link/Safe/Agents/<name>/agent.toml`, the agent's specification, holds setup and behavior together.
Commands such as `start`, `connect`, `model set` and `tool add` save choices
here. You may also edit it, then run `lnk agent restart`. Credentials stay
in the secret store. New agents get Local Link's specification as concrete defaults,
with built-in tools off and no `extends` line.

### Recreate Files Automatically

Creation writes both specification files, `agent.toml` and `sandbox.toml`. When an existing agent's file
is missing, the next agent command recreates it with defaults and says
so. A deleted custom specification is not restored. The stable identity
is retained separately in private runtime state, and recreation leaves
stored credentials untouched. Invalid or unreadable files cause an error
rather than being replaced.

### `[behavior]`

The [Agent Specification](../../../config/specs/agent.md)
describes these keys. In this file they live beneath `behavior`, such
as `[behavior.tool.file] enabled` and `[behavior.model] thinking`.
The root `[model]` selects the provider and model ID, while
`[behavior.model]` sets how the model answers, such as its thinking. `command_response` belongs
directly in `[behavior]` and can label harness answers with
`"🤖 Automated Response"`.

### `tools_offered` and `meaning_offered`

Whether the agent was asked which of Link's tools it gets, and whether
it was asked for memory's embedding model, whatever it answered: not
asked again. Left out, false.

### `[webhooks.<name>]`

Each of its webhooks: `id`, its hook's id, the last part of its address;
`from`, `github` for one GitHub signs (left out, Link's scheme); and
while its secret rolls, `previous_until`, until when its previous secret
is taken. Its secrets are your secrets', in [`lnk secret list`](../../../../../../security/secret/contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list), never this
file's.

## Read the Machine Runtime Records

Local Link keeps these fields in this machine's private records, not in
`agent.toml`. Identity and ownership are in `identity.toml`; ports,
program paths and stopped services are in `here.toml`.
[Agent Files](../../state/files.md#this-machines-records) gives their full paths.

### `id`

The agent's ID, which never changes and moves with it, wherever the
agent goes. A move here refuses an agent whose name another agent of
another ID has, and one whose ID an agent of another name has.

### `owner`

Whose it is: the account signed in (`acct-<id>`), or before you sign in,
`<user>@<machine>`, which the account replaces at its next start. Left
out, the latter.

### `stopped_services`

The services of the install `lnk agent migrate` brought it in from,
each kept from starting in `~/.lnk/safe/agents/stopped-services`, by its path in your home
folder: `lnk agent delete` puts each back. Left out, none.

### `port` and `block`

Its first port and how many from there are its harness's (left out:
200). Every agent has its own, `main` too, given when it's made or
arrives here, or at its next start when it has none. Its proxy, where
it listens on this machine's loopback, takes the last port of the block.

### `webhook_port`

The port on 127.0.0.1 its bridge takes its webhooks' posts at, chosen at
its first `lnk agent webhook create` and kept.


## Sandbox Permissions

`~/Link/Safe/Agents/<name>/sandbox.toml` holds what you grant the agent,
whichever harness runs it, as a [sandbox's
specification](../../../../../../security/sandbox/docs/config/specs/sandbox.md)
in the agent's own terms. Changed by `lnk agent permission add` and `remove`, or
written whole by `lnk agent start --sandbox <file>`; `lnk agent show --permissions` shows them.

### Handle Missing or Invalid Permissions

A missing file grants no additional files, programs or network, even
before recreation runs or if writing the replacement fails. Agent
commands recreate it with closed permissions and report the path.
A section left out stays closed, and an unknown key is an error.
`Sandbox on` means OS isolation is enabled. The status also shows the
sandbox file and its saved grants. Restart a running agent after changing
permissions so the next process uses them.

### `[files]` `write` and `read`

The folders it may write, and those it may only read, by name, side by
side: one of its Home's (`work`), or its `instructions`, `skills` or
`chats`, in `agent/`. `write = ["."]` is the whole of Home,
beside which only those three are named, and `read = ["~"]` is your
whole home.

#### `[files]` without a file

No additional folders are granted. Tool setup can propose access to
`work` with your confirmation. The harness still accesses its own chat
storage, while memory writes notes through its separate `remember` tool.

#### Which harness it holds for

It holds for a harness that keeps to the layout (Link Harness), whose
sandbox then writes only these and `chats/`; a harness that
doesn't writes all of Home, and never reaches `agent/`.

### Where It Finds Programs

Its run and code tools find a program on the `PATH` of whoever last ran
`lnk agent start`, then the system's, so a toolchain your shell finds is
found: rustup's `cargo` and `rust-analyzer`, nvm's `node`, `uv`'s
`pyright`. `lnk agent start` keeps those folders in this machine's
record (`here.toml`'s `path`); start it again after installing one
elsewhere.

#### Give a Toolchain What It Needs

A toolchain in your home folder is read with what it needs beside it,
`~/.cargo` and `~/.rustup` for rustup, a credential there hidden
(`~/.cargo/credentials.toml`). A folder of your `PATH` no toolchain
names is read alone. Its package caches are its own, in
`~/.lnk/data/agents/<name>/caches/`. `[commands]` still says which
programs it may start: a coding agent's is `["*"]`, since a build or a
test run starts programs no list names, its compilers' helpers and
what the repository builds.

### `[files]` `write_dot_files`

The dot files at the top of a folder it writes that run outside its
sandbox, `.git`, `.envrc`, `.vscode` and `.idea`, stay read only to it
unless `write_dot_files` names that folder, which `write` names too.
A coding agent needs `write_dot_files = ["."]` to commit in the
repository it works in, since a commit writes `.git`. The grant holds
for its harness and for each call of its `run` and `code` alike, so git
run through `run` commits. Its `file` never writes them, whatever the
grant: it reads and edits files, and has no need to.

#### What It Opens

What it writes there runs as you, outside the sandbox, the next time
you run git in the folder or open it in your editor: a hook in
`.git/hooks`, git's settings in `.git/config` (an alias, a filter,
`core.fsmonitor`), direnv's `.envrc`. So grant it only to an agent whose
work you review, review a change's `.git/hooks` and `.git/config` before
you run git there, and take it back with `lnk agent permission remove --agent <name>
--files.write_dot_files .`. Import names it among what an archive's sandbox
file opens. A later sandbox keeps the hooks and settings closed while
the agent commits.

### `[internet]`

`hosts`, the hosts its proxy lets it reach (`["*"]`, any),
`tcp`, endpoints beyond HTTPS and HTTP (`host:port`), and
`per_minute`, the most calls a minute on its proxy.

### `[localhost]`, `[lan]` and `[network]`

`[localhost] ports` is this machine's localhost ports it may reach, each
(a database or a service running here); `["*"]` only with `[network]
mode = "host"`. `[lan] hosts` is machines of your network it reaches
through its proxy. `[network] mode = "host"` is the network as it is,
with no proxy.

### `[tools]` and `[browser]`

`[tools] names` is the tools that run outside it it may call. `[browser]
hosts` is the hosts whose pages it may open in your browser, asking you
each time.

### `[commands] allow`

The programs it may start, by its `run` tool or its harness (beyond the
harness's own program, which Link starts), each a name on the system's
`PATH` or an absolute path, or `["*"]` for any; `[]`, no `run`. Only
those this machine has are offered, or, with `["*"]`, any program on
its `PATH`. Link's template has `["*"]` beside its toolchains, so builds
and what they make run. Drop it to keep only the toolchains.

#### `[commands]` without a file

No programs are granted. Adding `run` when none is allowed first asks
which toolchains it may start: `shell` alone (the default, and what
`--yes` takes), `shell`, `git`, `python` and `node`, or every toolchain
Link's sandbox template grants. The choices are `run_choices.toml`,
beside the [tool setup's code](../../../../src/tools/setup.rs). Adding
`code` proposes installed language servers. Each proposal names the
programs it lets the agent run and is displayed for confirmation before
the tool is enabled.

### `[teams.<team>]`

`write` and `read`: the folders of a team's shared folder it may write,
and those it may only read, by name, or `"."` for all of it; written by
`lnk agent team add`. A team it's on that isn't here, it reaches none
of. Holds in whichever harness runs it, and in Link's tools.

## A Harness's Grants (`harnesses/<harness>.toml`)

What the harness said it needs (`lnk-<harness> needs`) and you allowed,
held while it runs, for every agent here that runs it. `lnk agent
harness-permission list` shows them. A harness without a file needs nothing.

### Change a Harness's Grants

By answering what it asks, by `lnk agent harness-permission add` and `remove` (only
for what it asks for), and by `reset`. This file holds harness needs
separately from the agent's own approved permissions.

### Name Paths in Grants

Each path in it is absolute or from `~`, and none is inside the Link
folder's `Agents`, `Teams` or `Archive`, where agents' specifications and
files are: a file naming one is refused.

### Read a Specification's Sections and `risks`

A specification's sections hold what it asked for that you allowed (`[network]
mode = "host"`), and `risks` the risks of its own it named that you
allowed (`browser`).

### `answered` and `[asked]`

`answered` is the risks it asked for that you answered. `[asked]` is a
specification: every need it asked for that you answered, allowed or not, not
asked again until `reset`.

## Where Things Live

Your agent's settings are a few of the files Local Link keeps for it.
[Agent Files](../../state/files.md) lists every file an agent keeps while it's here,
grouped by purpose, and who can reach each. [Move Files](../../state/moves.md) lists
what a move or a delete leaves behind and how long it stays.

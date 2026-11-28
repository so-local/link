# Create and Delete Agents

Create an additional agent with its own name, model, files and connections, and delete an agent when you no longer need it.

```sh
lnk agent create work
lnk agent delete work
```

## Run More Than One Agent

Run as many agents as you like, each with its own settings, files,
harness, ports and bot.

```sh
lnk agent create scout --harness hermes
lnk agent create writer --on aws-1
lnk agent connect telegram --agent work
```

[List and Show Agents](show.md#list-your-agents) lists every agent you made,
here and on your boxes.

### Kept Apart

The sandbox keeps each agent out of the others' folders. With one agent,
commands act on it, and with several you pass `--agent <name>`. The first
agent is `main`.

### Named by ID

Every agent action takes [the agent's ID](#agent-ids), `agent:<id>`, so a name you type is looked up in your
agents (`lnk agent list`: here, moved from here, or on your boxes and
accounts) and sent as its ID, once, before the command runs; the ID
itself is taken too. Two of your agents with one name are told apart by
their IDs.

### Grants per Agent

Each agent has [its own grants](../../tooling/sandbox/permissions/README.md#choose-what-your-agent-can-reach),
and agents on one harness share only what that harness needs.

## Create on a Hosting Account

`lnk agent create work --on <account>` makes a blank agent directly on
an account that hosts agents, using its harness and model defaults. It
asks for no model provider credential and makes no local copy. The account bills the
agent it runs, so `create` asks first. `--yes` answers yes, and with no
terminal and no `--yes` it makes nothing. `lnk agent list` shows
it there; `lnk agent connect telegram --agent work` connects it through
that account. A box name after `--on` still creates the agent on the box.

## Create With an Environment

`lnk agent create work --env env.toml` sets an [environment](../../../../../compute/env/docs/services/environments/guide.md)
up on this machine for the new agent, as [`lnk env setup here`](../../../../../compute/env/docs/services/environments/guide.md#set-a-machine-up-without-an-image) does. Its
stages run each in its sandbox, or directly while
the agent runs with `--no-sandbox`. Its services start with the agent and
stop with it. The environment is captured once, at creation, and kept in
`~/.lnk/safe/agents/environments/<name>.json`. An existing agent keeps
the environment it was created with.

### Captured Inputs

The agent's own specification may name `env`, relative to that file. A
specification never names a sandbox file or no sandbox, which are each
start's own. Command-line paths are relative to the caller. Creation
captures their content once, so later edits affect future creation
rather than the running agent's environment or approved network grant.

## Delete an Agent

Delete an agent and choose whether to keep its files for recovery. If it runs in a terminal, first stop it with Ctrl-C there or `lnk agent stop --agent work` from another terminal.

```sh
lnk agent delete work --yes
lnk agent delete work --yes --delete-files
```

## What a Delete Removes

`delete` stops the agent and deletes its settings, its harnesses with
their installs, memory and conversations, its log, its keys, its
sandbox rules and its memory's and scheduler's tools. The
[interaction store](../../../../../../packages/interactions/docs/more/libraries/interactions/interactions.md#a-subscriber-that-never-reads-again)
forgets its reader, so the events only it would have read go. A
question it asked you that's still open stays until it expires. If the
store can't be written, `delete` stops before anything else goes, and
you can run it again. An agent you only stop keeps its reader, and
reads what it missed when it starts.

## Files a Delete Can Keep

Its files (its Home and its own folder, with its instructions, skills,
conversations, memory's index and scheduled jobs) go only if you say so.
Kept, they're written with its settings and sandbox file to one
[archive](../../../../../tooling/archive/docs/tooling/archive/README.md),
`~/Link/Archive/<name>-<date>.tar.gz`, for 30 days, or as `~/.lnk/safe/agents/policy.toml`'s
`keep_moved` says, and a Home you chose yourself stays where it is. Its
buckets stay in your clouds, and this computer forgets their encryption.

### When an Archive Can't Hold the Files

A file no archive carries, such as a link or a file with a second name,
keeps its files as a folder in `~/.lnk/data/agents/move/<name>-<date>`
instead, for as long, and the delete says why. An agent on a harness
other than Link Harness kept its chats and memory in that harness's
home, which goes, so its archive holds its settings and files only.

### Bring a Deleted Agent Back

Importing that archive brings the agent back, with its files, chats and
memory, as a new agent with a new ID, since the old one is retired:

```sh
lnk archive import ~/Link/Archive/work-2026-11-10.tar.gz
```

## Agent IDs

Every agent has an ID, `agent:<id>`, reserved when it was created and
kept through renames and moves. `delete` retires the ID on this
computer, so it never names another agent here: an agent created again
with the same name gets a new ID, and nothing granted to the old one,
such as a secret, reaches it. Only that same agent, brought back from
its buckets, takes the old ID again, never one imported from its
archive.

## What Stays After a Delete

What [`lnk measure`](../../../../../tooling/measure/docs/tooling/measure/README.md#measure) recorded of it stays until it ages out. It leaves
every team it was on, and the teammates that run restart to take that.
With one agent left, commands act on it without `--agent`.

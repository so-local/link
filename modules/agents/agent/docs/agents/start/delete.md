# Delete Your Agent

Delete an agent and choose whether to keep its files for recovery. If it runs in a terminal, first stop it with Ctrl-C there or `lnk agent stop --agent work` from another terminal.

```sh
lnk agent delete work
lnk agent delete work --yes
lnk agent delete work --yes --delete-files
```

## What Goes

`delete` stops the agent and deletes its settings, its harnesses with
their installs, memory and conversations, its log, its keys, its
sandbox rules and its memory's and scheduler's tools. The
[interaction store](../../../../../../packages/interactions/docs/more/libraries/interactions/interactions.md#a-subscriber-that-never-reads-again)
forgets its reader, so the events only it would have read go. A
question it asked you that's still open stays until it expires. If the
store can't be written, `delete` stops before anything else goes, and
you can run it again. An agent you only stop keeps its reader, and
reads what it missed when it starts.

## Its Files

Its files (its Home and its own folder, with its instructions, skills,
conversations, memory's index and scheduled jobs) go only if you say so.
Kept, they're written with its settings and sandbox file to one
[archive](../../../../../tooling/archive/docs/tooling/archive/README.md),
`~/Link/Archive/<name>-<date>.tar.gz`, for 30 days, or as `~/.lnk/safe/agents/policy.toml`'s
`keep_moved` says, and a Home you chose yourself stays where it is. Its
buckets stay in your clouds, and this computer forgets their encryption.

### When It Can't Be One Archive

A file no archive carries, such as a link or a file with a second name,
keeps its files as a folder in `~/.lnk/data/agents/move/<name>-<date>`
instead, for as long, and the delete says why. An agent on a harness
other than Link Harness kept its chats and memory in that harness's
home, which goes, so its archive holds its settings and files only.

### Bring It Back

Importing that archive brings the agent back, with its files, chats and
memory, as a new agent with a new ID, since the old one is retired:

```sh
lnk archive import ~/Link/Archive/work-2026-11-10.tar.gz
```

## Its ID

Every agent has an ID, `agent:<id>`, reserved when it was created and
kept through renames and moves. `delete` retires the ID on this
computer, so it never names another agent here: an agent created again
with the same name gets a new ID, and nothing granted to the old one,
such as a secret, reaches it. Only that same agent, brought back from
its buckets, takes the old ID again, never one imported from its
archive.

## What Stays

What [`lnk measure`](../../../../../tooling/measure/docs/tooling/measure/README.md#measure) recorded of it stays until it ages out. It leaves
every team it was on, and the teammates that run restart to take that.
With one agent left, commands act on it without `--agent`.

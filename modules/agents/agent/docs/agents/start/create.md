# Create Another Agent

Create an additional agent with its own name, model, files and connections.

```sh
lnk agent create work
```

## Run More Than One Agent

Run as many agents as you like, each with its own settings, files,
harness, ports and bot.

```sh
lnk agent create work
lnk agent create scout --harness hermes
lnk agent create writer --on aws-1
lnk agent list
lnk agent list --refresh
lnk agent connect telegram --agent work
```

`lnk agent list` asks your boxes for their agents too. `--refresh` also
asks your clouds for boxes made on your other computers, and `--here`
lists only this machine's agents, asking nothing else.

```sh
lnk agent list --here
```

### Kept Apart

The sandbox keeps each agent out of the others' folders. With one agent,
commands act on it, and with several you pass `--agent <name>`. The first
agent is `main`.

### Named by ID

Every agent action takes [the agent's ID](delete.md#its-id), `agent:<id>`, so a name you type is looked up in your
agents (`lnk agent list`: here, moved from here, or on your boxes and
accounts) and sent as its ID, once, before the command runs; the ID
itself is taken too. Two of your agents with one name are told apart by
their IDs.

### Grants per Agent

Each agent has [its own grants](../../security/sandbox/permissions/README.md#choose-what-your-agent-can-reach),
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

# Migrate to Local Link

> Migrate Your Agent, Keep Its History

Bring an agent you already run with OpenClaw, Hermes, or DeepSeek under
Local Link. Migration copies the agent's files, imports supported secrets and connections,
and runs the same harness in Local Link's sandbox.

```sh
lnk agent migrate
```

## Choose a Harness

If you have more than one supported harness installed, name the one to
bring under Local Link. Naming a harness replaces automatic discovery.

```sh
lnk agent migrate --harness hermes
```

## Understand the Agent Protocol

Migration brings an existing installation into the management model
explained by the [Agent Protocol](../../../../harnesses/portability/contract/docs/agents/harnesses/more/agent-protocol.md).
The harness's adapter identifies native homes, configuration, and services, and
checks supported state versions before Local Link copies them. The managed
agent uses the same harness when started. Changing the harness afterward uses
its declared portable-transfer capabilities.

### Know the Supported Sources

OpenClaw, Hermes, and DeepSeek are migration sources. [Compare Harnesses](../../../../../../interop/agents/harnesses/docs/agents/harnesses/compare.md)
shows creation, control, migration, and transfer separately.

## Review the Migration Plan

Before anything happens, `migrate` says what it found and where each
part goes. The old background service is stopped and kept from starting
(two would answer one bot). The old home is copied into the agent's harness
home, the secrets are handed over, and the rest stays where it is. Then `migrate` says
each step as it ends.

### Check Environment Variables and Secrets

The plan lists every variable of the `.env` file in a table. The table says what Local Link makes
of each variable, and where it's sent. Check that each goes where you expect, as a
variable may be used other than its name says. Local Link knows the variables
in [`variables.toml`](../../../src/migrating/variables.toml), and
takes none of them plain.

#### Follow Each Secret's Destination

A hosted model's key becomes the agent's model provider credential, kept as `lnk agent
start` keeps one, and the proxy adds it for the provider. A key for a service
Local Link knows is given to the agent sealed for the API's one host. A
bot's token goes to its channel. Any other secret is left in the
original, and the plan names the [`lnk secret add`](../../../../../security/secret/docs/config/secrets/README.md#grant-access-to-a-secret) that gives it sealed.

#### Watch for Custom Provider Endpoints

`migrate` leaves a key in the original, and says so, when a variable beside it
sends the key elsewhere. An `OPENAI_API_KEY` with `OPENAI_BASE_URL` naming
another provider's API goes there, not to OpenAI. Every secret is taken
out of the copy's `.env`, so the harness never holds its value.

### Match Harness Versions

`migrate` refuses a state written by a newer harness than the one Local Link runs, before
anything is copied. It names the version and why, from OpenClaw's
`meta.lastTouchedVersion` in `openclaw.json` or Hermes's `_config_version`
in `config.yaml`. Updating Local Link first lets the state through.

## Migrate Multiple Agents

Every OpenClaw agent folder (`~/.openclaw/agents/<name>`, with its
`workspace-<name>`) and every Hermes profile (`~/.hermes/profiles/<name>`)
becomes a Local Link agent of the same name. The plan names each one and
asks about it, so you can leave any out. Each gets its own copy, holding no
other agent's folders, and each takes the install's model provider credential as its own.

## Reconnect Chat Bots

A bot's token (Telegram, Discord, Slack) goes to Local Link's connection
instead. In a terminal, `migrate` ends by connecting the bot with the token
given, so you only confirm the bot is yours.

## Rewrite Paths and Sandbox Access

`migrate` rewrites every path into the harness's home, in its settings, its sessions, and
any text file of the home, to the home under Local Link. SQLite copies a
database whole, with its write-ahead log in the copy,
and `migrate` rewrites the paths through SQL, never as text. `migrate` says
what the sandbox changes. The install
reached your whole computer, and under Local Link it writes only its home and
your agent's files. [A login of the harness's own that can't move](decisions.md), such
as Hermes's `auth.json`, keeps working.

## Keep the Original Installation

The original keeps working until you remove it, because `migrate` copies its files
and never moves them. Its service file is kept in `~/.lnk/safe/agents/stopped-services`, by its path
in your home folder, and put back when you remove the agent. `migrate` copies a home that is a link to a folder elsewhere, as a dotfiles
repository keeps one, from that folder.

### Remove the Original

Once the agent answers under Local Link, `--clean` removes the original
installation. The command names every file it will remove first, the kept service
among them, and removes nothing beside them.

```sh
lnk agent migrate --clean
```

## Resume or Undo Migration

If a migration stops partway, `lnk agent migrate` again carries on from the
step it stopped at. `lnk agent delete <name>` undoes what the migration did and
puts the service back. After a finished migration, `lnk agent delete`
turns the original's service back on too, so it runs as it did. In a terminal, `migrate` ends by offering to start the
agent under Local Link, with its harness installed in its home. Once the agent
answers under Local Link, `lnk agent harness set link --agent <name>` switches the agent to Link
Harness whenever you like.

## Understand Migration Decisions

Local Link copies state, rewrites paths, and keeps the original installation available for recovery. [Migration Decisions](decisions.md) explains why.

## Carry Native Schedules

OpenClaw and Hermes schedules become entries in the agent's own specification.
The plan names each job with its timing and destination, and names every
job that cannot be carried with its reason. Cron schedules retain their
explicit time zones. Intervals retain their phase rather than restarting
the interval when you switch harnesses.

### Keep One Firing Owner

Import is one way. Local Link records each native identity before clearing
only the managed copy's cron source, then activates its own scheduler.
The original installation keeps its jobs. An independently running
original installation can still fire those jobs, so keep its service
stopped when using the managed agent.

### Resume an Interrupted Import

If import stops partway, repeat migration to resume the recorded stage.
Malformed native state is refused rather than treated as an empty list.
Unsupported execution options, destinations and timing appear as named
omissions. The complete native records remain in the managed home's
`.lnk/schedule-imports/` archive after handoff.

<!-- docs-tree:start -->

## Docs

- [Decisions](decisions.md): Why bringing an existing agent under Link copies its state and rewrites paths in the forms that own them

<!-- docs-tree:end -->

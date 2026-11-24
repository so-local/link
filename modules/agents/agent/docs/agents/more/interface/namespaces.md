# Action Namespaces

An agent's actions have the same names across its surfaces: a command
is `/task:run`, a tool is `task__run`, and a route is
`/_link/task/run`. CLI actions are under `lnk agent`, for example
`lnk agent task run --agent main --request request.json`.

## Commands and Aliases

The [namespace command reference](../../../../../harnesses/link/docs/agents/link/commands/namespace-commands.md)
lists declared command actions. The separate [alias reference](../../../../../harnesses/link/docs/agents/link/commands/aliases.md)
maps familiar spellings to their targets. For example, `/new` and
`/clear` resolve to [`/chat:create`](../../../../../harnesses/link/docs/agents/link/commands/README.md#start-a-new-conversation). The target owns execution; aliases
add names without adding handlers or permissions. Root-level `/help`
is a discovery command outside the namespaces.

## Grants

The registry controls names, callers, surfaces, bridge and budgets.
The proxy proves the calling agent; no request selects another agent,
backend, program or credential. Routes without a provider are left out
of its grant, and discovery is filtered to it.

### Owner Model Calls

Owner model commands reach the running proxy through its existing Unix socket
when Linux isolates its network. On a shared network they use its loopback
listener. Both transports enforce the same grant without loading provider
credentials into the owner command.

## Models and Conversations

The [model API](../../../../../model/docs/agents/models/more/api.md) owns
inference and model details. Conversation management is in `chat`;
sending is `lnk agent chat send`. Skills remain in `skill`, and
owner management of tools in `lnk agent tool`.

### Questions

`question__create` asks the owner a related set of confirmation, text or choice questions. The host binds the original conversation, attempt and tool call, then waits outside the sandbox on the [interaction engine](../../../../../../../packages/interactions/docs/more/libraries/interactions/interactions.md). Results retain selected supplied wording, recommendations, Other text, notes and explicit skips. Unsupported harness attempt bindings are refused. Ordinary answers and automatic fallbacks carry no execution authority.

#### Timeout Opinions

The agent specification's `[approvals.domains.question]` table accepts `wait_secs`, shape-specific `confirmation`, `text` and `choice` fallback values, and `allow_request_overrides`. Fallbacks are `expire`, `use_default` or `use_recommended`. Absent opinions retain `[connections] ask_wait` and expire without a fallback. Parent domain opinions follow registered parentage. The effective policy is frozen when created, and authorization domains always prohibit positive automatic answers.

## Work and Files

`task` owns `run`, `start`, `list`, `read`, `stop`, `agent` and
`send`. It is one MCP server over the run and scheduler providers.
`cron` owns the scheduler's schedules: `create`, `delete` and `list`.
`task__list` combines their lists; `kind` selects `run` or `schedule`.
A mutation with multiple providers, such as `task__stop`, requires
`kind` and reaches one. `file` owns `read`, `write`, `edit`, `glob`,
`grep`, and the language server's `definition`, `references`, and
`diagnostics`. Which provider's tool each action calls is declared once,
in the harness contract's
[namespaces table](../../../../../harnesses/portability/contract/docs/agents/harnesses/more/protocol/settings.md#namespaces).

## Web and Screens

`web` owns `fetch` and `search`; opening a page stays approval-controlled.
The browser keeps `open`, `read`, `click`, `type`, `select`, `scroll`,
`screenshot` and `close`. The desktop keeps `screenshot`, `click`,
`type`, `key`, `scroll` and `drag`. Their providers' confinement and
approval rules still hold.

## Identity and Memory

`agent/show` returns its name, harness, model and model limits. The
`instructions` option includes its instructions. `memory/show`
returns notes and their sources without host paths; the memory
engine's tools retain their names. `tool/list` describes its granted
namespaces, also shown by `lnk agent tool list --json`. Link MCP
(`mcp`) lists the reads its grant opens (`manual`, `agent__list`); its other
calls ask first.

## Messaging

`agent/send` takes `to`, `message`, and a conversation `thread`; tools and
commands supply their own thread. With `to: "owner"`, it sends on that
thread's configured connection, whose recipient checks still apply. A Link
chat holds the message until its next read. Existing teammates use the
scheduler's selection, caps and approval checks. `task/schedule` also
takes a thread for its answer. Message text is limited to 16 KiB.

### Select a Connection

The agent's `send` tool also accepts `connection` with `to: "owner"`:
choose a connected service such as `discord`, `telegram`, `slack` or
`signal`, or `cli`. The tool's description lists the services the
agent's bridge carries, and each send checks them again, so a service
connected since is taken. A service it doesn't carry is refused with a
reason the model reads, naming those it does.

#### Where It Lands

The service resolves its
owner's direct chat; credentials and recipient IDs stay outside the
model. A message to `cli` appears in the next active or newly opened
CLI chat, and its receipt says `held`. An explicit thread still keeps
its own pending answers separate. `lnk agent chat send --to owner
--connection <connection>` sends the same way from a command line.

### One Scheduled Delivery

When a schedule targets a connection, a successful explicit owner send
on that connection satisfies its delivery. The turn's final acknowledgement
is kept in its chat without sending a second message. A send elsewhere
does not satisfy that destination, and the next recurring firing owes
a fresh delivery.

## Provider Checks

Namespace wrappers check each provider's current definition and
approval policy, and refuse unlisted arguments. Removing a provider
takes effect at the next call; reconnect after a running provider
changes. `web/fetch` and `web/search` share typed page and result
answers with their MCP tools.

## CLI and Commands

CLI actions read JSON from `--request <file>` or standard input. A
person reads the answer as fields, each by its name, with lists of
records as tables, and `--json` prints the JSON itself. Empty input
means no arguments. Commands take that same
JSON after their name. Link Harness and the hub run Link-owned
commands through proved routes. Messages from others are not owner
commands. Native harness commands use its advertised names and aliases.

## Limits

Calls have byte and time budgets. A timeout can leave a mutation's
outcome uncertain: inspect its state before retrying. Routes and
wrappers never retry a mutation. The internal move-stream command is
`lnk agent state-send`. An agent sends to its owner or a teammate with
`/agent:send`, which a shell says as `lnk agent chat send --to`.

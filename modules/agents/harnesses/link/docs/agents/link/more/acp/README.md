# Agent Client Protocol

> Agent Client Protocol (ACP) Seats Any Client


Link Harness speaks [ACP](https://agentclientprotocol.com), so an ACP client can open a session on one of its conversations, prompt
it, and receive the answer as it's written, with the tool calls beside
it. ACP is the only way in, so Local Link's connections, `lnk agent chat send`,
and the messages from schedules and subagents all use it.

## Connect to the Harness

A connection is a WebSocket on the agent's endpoint port, at `/acp`,
one JSON-RPC message to a text frame. Like any request to the endpoint,
it carries the agent's endpoint key as a bearer token, and
`Link-Agent` or `Link-Agent-Id` where the harness answers for more than
one agent. Without the key, the WebSocket is refused before it opens.

### `initialize`

`initialize` answers with ACP's first version, whatever version was
asked for, and says what Link Harness does. It loads, lists, and resumes
sessions, and takes images and embedded resources in a prompt.

## Sessions are conversations

A session is one of the agent's conversations, and its id is the
conversation's thread: `telegram:42`, `default`. A connection prompts
and cancels only the sessions it opened.

### `session/new`

`session/new` opens the thread `_meta.link.thread` names, or a new one, `acp:` and a
number, which is then the session's id.

### `session/load`

`session/load` replays a conversation as the updates that streamed it, then opens it.
A replay holds your messages, the agent's answers, its calls and their
answers, and each command as you typed it. Summaries and thinking are
the model's, and never replayed.

### `session/resume` and `session/list`

`session/resume` opens a conversation without replaying it, and
`session/list` lists the agent's conversations, kept any way.

### `session/prompt` and `session/cancel`

`session/prompt` sends a message, answered as a turn, and
`session/cancel` ends the session's running turn, and its prompt answers
`cancelled`.

### Commands in a Session

Once a session is open, the harness lists the commands that are on, in
an `available_commands_update`. A command is still the text of a prompt, such as [`/chat:create`](../../../../../../../agent/docs/agents/chat/README.md#manage-history), as it is on every connection, and [Commands](../../commands/README.md) explains each.

## Send a Prompt

A prompt's text blocks are its message. An image block, or an embedded
resource with its contents in base64 (`blob`) or as `text`, is an image
or a document of the message, taken as the agent's specification says
(`[connections] images` and `documents`). A link to a resource, or audio,
is refused, never dropped.

### While a Turn Runs

The harness sends the session three kinds of update. `agent_message_chunk`
carries each piece of the answer as it's written. `tool_call` carries each
call the model makes, with the tool's name as its title and its arguments
as `rawInput`. `tool_call_update` carries the call's end, `completed` or
`failed`, with the tool's answer.

### Answer a Prompt

The prompt answers `end_turn` once the turn ends, or `cancelled` when
it was cancelled. A prompt sent while another turn of the session runs
joins it, as the agent's specification says (`[turns] busy`). The prompt then answers at
once, `end_turn` with `_meta.link.steered` true, and the running turn's
answer answers it too.

## Steer, queue or interrupt

What a prompt sent while another turn of its session runs does is its
own to say, in `_meta.asp.busy`, as the [Agent State Protocol (ASP)](../../../../../../portability/agent-state-protocol/docs/agents/harnesses/more/agent-state-protocol.md#steering) has it. `steer` joins the answer under way, `queue` waits for its
own turn, and `interrupt` ends the running turn and starts its own.
Left out, the agent's specification says (`[turns] busy`).

<!-- json: link_agent_state_protocol::acp::schema::v1::PromptRequest -->
```json
{"sessionId": "telegram:42", "prompt": [{"type": "text", "text": "Skip the tests."}], "_meta": {"asp": {"busy": "interrupt"}}}
```

### Which Modes the Harness Takes

`initialize` says which modes the harness takes, in `_meta.asp.steering`.
It takes `queue` and `interrupt` always, and `steer` where the agent's
specification steers, since only then does a running turn read what joins it. The
harness refuses a mode it doesn't take.

## Local Link's Words beside a Prompt

What Local Link says of a message is in the message's `_meta`, under `link`. A
`_meta.link` holding anything else, or a sender that isn't one, is
refused.

<!-- json: link_agent_state_protocol::acp::schema::v1::PromptRequest -->
```json
{"sessionId": "telegram:42", "prompt": [{"type": "text", "text": "Find the note."}], "_meta": {"link": {"from": "member:telegram:42", "fromName": "Ada", "hops": 1, "tools": ["memory__search"]}}}
```

### `thread`

On `session/new`, `thread` names the conversation the session is, and the
harness opens it in place of a new one.

### `from`

`from` says who sent the message when the owner didn't: a schedule, a
subagent, another agent, or a person. A message from anyone but the
owner never runs a command.

### `fromName`

`fromName` is a person's name on their connection. It rides beside a person
only, as words for the model, and never says who the person is.

### `hops`

`hops` counts how many messages led to this one, each sent by an agent on
its own. The harness passes the count on to its tools.

### `tools`

`tools` lists the tools the conversation may use from this message on, never more
than it had. A later list narrows the conversation's tools.

## Move Conversations over ASP

Link Harness answers the agent state of [ASP](../../../../../../portability/agent-state-protocol/docs/agents/harnesses/more/agent-state-protocol.md)
for its conversations, and advertises them under `initialize`'s
`_meta.asp.agentState`. `_asp/state/export` gives each conversation as
`session/load` replays it; `_asp/state/import` writes each one that
came in under its thread's name, so `session/list` lists it and it
goes on. Who the agent is, its skills and its memory are its specification's
and Link's memory's, which Local Link carries itself, and an import says so of
each.

### Native Execution Evidence

Each exported session also carries the local exporter's versioned `evidence` manifest. It classifies native observations and imported update ranges, retains internal execution records, and reports snapshot gaps. The [execution evidence contract](../../../../../../portability/contract/wire/docs/agents/harnesses/more/vocabulary/wire.md#exported-sessions) defines its validation. The owner's [state export command](../../../../../../../agent/docs/agents/harnesses/use/state/README.md) uses this projection while running or stopped.

### Conversations a Thread Already Holds

A conversation that went on in another harness comes back to a thread
that holds its start. The import adds only the turns after what the
thread holds, when the owner's turns that came in open with the
thread's, each holding its words. Otherwise the conversation parted,
and goes on a thread beside it named for the harness it came from,
such as `link:main~hermes`, which [`lnk agent chat list`](../../../../../../../agent/docs/agents/harnesses/use/README.md#conversations-that-diverged) shows. One that
names no harness goes on a new thread of its own.

### Imported conversations

Session import keeps the exact exported updates beside native records
for text, completed tool calls, images and supported resources. Historical
tool calls never execute. Thinking without native signatures, audio and
unsupported metadata remain archival: export preserves them, while the
continuing conversation uses the content it can represent. The import
response names this limit.

## What Local Link Adds

Local Link adds two methods of its own beside ACP's. `_link/taken` is sent
as soon as the harness has a message, and `_link/recover` asks for the
answers a crash left.

### `_link/taken`

`_link/taken` is a notification the harness sends a session as soon as it
has a prompt's message, before the answer. A webhook's sender is told then.

### `_link/recover`

`_link/recover` is a request for the answers of turns finished after a
crash, or whose caller went before the answer. `{"two_phase": true}` gives
each with its `id`, kept until `{"done": ["<id>"]}` says it was sent. Local
Link's bridge asks as it starts and every half minute.

## Ask a Question from a Shell

`lnk-harness ask <thread>` asks the harness on this machine's port
one message, as a bridge does, and prints its answer. Its input's first
line is the endpoint key, and the rest is the message, so a program runs
`lnk-harness ask telegram:42` and writes the key, a newline and `What did
I say?` to it. `--from <sender>` sends the message from someone who isn't
the owner. The reasons are in [Decisions](decisions.md).

<!-- docs-tree:start -->

## Docs

- [Decisions](decisions.md): Why Link Harness speaks Agent Client Protocol (ACP) the way it does

<!-- docs-tree:end -->

# Agent State Protocol

The Agent State Protocol (ASP) is Link's extension to the Agent Client Protocol (ACP) for carrying an agent's state between harnesses, so a
conversation can continue after changing the program that runs it.
It also defines how to handle a prompt arriving during a running turn,
and forms the state part of the [Agent Protocol](../../../../../contract/docs/agents/harnesses/more/agent-protocol.md).

## ACP and ASP together

[ACP](../../../../../contract/docs/agents/harnesses/more/protocol/acp.md) is an externally maintained open standard for
communication between clients and agents. It provides sessions,
prompts, streamed responses and tool activity. ASP uses ACP's extension
mechanism to describe, export and import state across harnesses; it is
Link's interpretation of portable agent state, not an adopted part of
upstream ACP.

### A harness switch

A client uses ACP to connect to both harnesses and discover their
capabilities. ASP supplies the state exchange: the source exports
portable state, the destination imports what it supports, and the
client uses the returned session mapping to continue the chat.

1. Read the source's export kinds and destination's import kinds from their advertised capabilities.
2. With no turn running, export the supported kinds from the source and hand that state to the destination.
3. Read what was taken, what was not taken and each session's new identifier before prompting the destination over ACP.

### What portability guarantees

Transfer depends on both harnesses' capabilities. A defined state kind
is not a promise that every harness exports or imports it, or retains
its full native representation. A destination reports unsupported kinds
and why it could not take them. Link's actual switching workflow,
including what stays behind, is documented in
[Switch Harnesses](../../../../../../../agent/docs/agents/harnesses/use/README.md).

## On ACP

ASP adds optional behavior alongside ACP's existing session lifecycle.
A client that only speaks ACP can use an agent's advertised ACP
capabilities without understanding ASP. A client uses ASP only when
the agent advertises it; speaking ACP alone does not imply portable
state support.

### Written as ACP's extensions

ASP uses extension methods named `_asp/...` and metadata under
`_meta.asp`. These names identify Link's extension and leave ACP's
standard methods intact. The `link-agent-state-protocol` crate defines the typed forms
and methods using ACP's own Rust SDK.

### Saying which layers it speaks

An agent advertises ASP in its `initialize` answer's capabilities,
under `_meta.asp`. State transfer and steering are independent layers;
the agent-state layer names its export and import kinds separately. This
example advertises instructions for export only, alongside sessions
that can travel in both directions.

<!-- json: link_agent_state_protocol::acp::schema::v1::InitializeResponse -->
```json
{"protocolVersion": 1, "agentCapabilities": {"_meta": {"asp": {"steering": {"busy": ["queue", "interrupt"]}, "agentState": {"export": ["sessions", "instructions"], "import": ["sessions"]}}}}}
```

## Steering

What a prompt sent while a turn of its session runs does is the
prompt's to say, in its `_meta.asp.busy`. Link Harness speaks it over [ACP](../../../../../../link/docs/agents/link/more/acp/README.md#steer-queue-or-interrupt).

<!-- json: link_agent_state_protocol::acp::schema::v1::PromptRequest -->
```json
{"sessionId": "telegram:42", "prompt": [{"type": "text", "text": "Skip the tests."}], "_meta": {"asp": {"busy": "interrupt"}}}
```

### `steer`

The prompt joins the answer under way, at the agent's next step, and
that one answer answers both.

### `queue`

The prompt waits for the running turn to end, then gets a turn and an
answer of its own.

### `interrupt`

The prompt cancels the running turn where it is, and starts the next
one, its own.

### Which an agent takes

An agent says which it takes in `_meta.asp.steering`'s `busy`, and
refuses a prompt asking for another, never silently doing one it wasn't
asked for. Left out, the agent does what it does by default.

## Agent state

ASP defines portable state kinds using existing formats: ACP session updates and MCP descriptors, instructions, Agent Skills files, Markdown notes, session settings, and schedule definitions. These are portable representations rather
than copies of a harness's native database. Workspace files remain in
the workspace; transferring state does not copy the working directory.

### `sessions`

Each conversation as ACP's own `session/update` stream, as
`session/load` replays it, with its id and title. A conversation carried
from another harness also names that harness in `harness`. A
destination that keeps it beside a different version of its own names
the copy `<thread>~<harness>`.

#### Local Execution Evidence

A local exporter can attach the optional, versioned `evidence` manifest defined by the [shared execution vocabulary](../../../../../contract/wire/docs/agents/harnesses/more/vocabulary/wire.md#exported-sessions). Its native records sit outside imported updates, which may contain arbitrary metadata. Import never promotes an incoming manifest into local execution. `Session::native_evidence` validates the manifest's sequences, ranges, version, and conversation before a consumer uses it. Absence remains unverified.

### `instructions`

The agent's `AGENTS.md`, whole, the instructions a harness reads in
place of its own.

### `skills`

Agent Skills folders, each file by its path, `SKILL.md` among them, so
a harness that reads the standard takes them as they are.

### `tools`

The agent's MCP servers, as ACP's `session/new` takes them, so the
harness taking the state reaches the same tools.

### `memory`

Markdown notes by path, each with where it came from. A deleted one
says so, so the agent taking it forgets it too.

### `settings`

Each session option's value, by the option's id, as ACP's own
session settings name them.

### Named Omissions

State can include `omissions`, a list of kind and reason pairs, defaulting
to empty. An export names native state that has no supported readable
projection. Import carries these reasons into `notTaken`, alongside its
own limits. Retaining a complete portable record does not imply native
runtime state was reconstructed or consumed by a model.

## Ask an agent for its state

An agent says what it gives and takes in `_meta.asp.agentState`,
`{"export": [...], "import": [...]}`, and a client asks it three
things.

### `_asp/state/describe`

Asked `{}`, it says how many of each kind it holds now:
`{"kinds": {"sessions": 3, "instructions": 1}}`.

### `_asp/state/export`

Asked `{"kinds": [...]}`, with no turn running, it gives those kinds,
`{"state": {...}}`, each kind it gives under its name. A kind left out
is one not given, never one given empty.

### `_asp/state/import`

Handed `{"state": {...}}`, it says what it took, what it couldn't and
why, in words for the owner, and each session's new id by its old, so a
client that keeps a session per chat follows it.

<!-- json: link_agent_state_protocol::methods::Imported -->
```json
{"taken": ["sessions", "instructions"], "notTaken": [{"kind": "memory", "reason": "this agent keeps no memory"}], "sessions": {"sess_789xyz": "sess_a1b2c3"}}
```

### Who answers it

Link Harness advertises session import and export; its instructions,
skills and notes are carried through Link's agent configuration and
memory rather than its ASP endpoint. The hub advertises capabilities
from its harness or adapter, with session export also available from
ACP replay where supported:
[Link Harness's ACP](../../../../../../link/docs/agents/link/more/acp/README.md)
and [the hub](../../../../../hub/docs/agents/harnesses/more/protocol/hub.md).
Its methods and forms are the `link-agent-state-protocol` crate's, typed and tested.

## Schedules

The `schedules` state kind carries definitions independently of the
harness running an agent. Each definition has a stable `id`, display
`name`, `when`, `message`, `to`, `enabled`, `revision` and optional native
`origin` identifying the adapter and source job.

### Cadence and Destination

A cadence is tagged by `kind`. `cron` carries five cron fields and an
explicit IANA `zone`. `every` carries positive `seconds` and an `anchor`
in Unix seconds, preserving the interval's phase. `at` carries one Unix
instant. A destination is either `owner` or `thread` with the complete
connection/chat/thread identity. Native conversion must report ambiguous
or unsupported destinations instead of choosing one.

### Omitted Schedules

The export contains `carried` definitions and a `notCarried` list with
each omitted job's identity, name and reason. An opaque `sourceRevision`
lets an adapter refuse clearing a native source changed since export.
An optional `sourceContext` binds semantic inputs outside that file, such
as a profile time zone.
Portable import is one way into the agent's schedule definitions. It
does not authorize writing another harness's native cron jobs.

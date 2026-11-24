# Harness Wire

`link-harness-wire` holds the words a harness's messages and skills are
written in: Link's own connection's threads, what came of a message,
and a skill as the Agent Skills standard writes one. Its `runtime`, how a
host calls a harness in its own process, is in [Harness Prompts](prompts.md).
Its code is in [`link-harness-wire`](../../../../../README.md).

## Execution Evidence

The `evidence` vocabulary records what a runtime observed about a tool
request. A proposal identifies the calls, their original provider IDs,
the turn or summary that proposed them, and the admitted work. A call's
identity is its successfully appended proposal's sequence and position
within that proposal, scoped to the agent and conversation. A proposed
sequence is not an identity until the store accepts its append.

### Persistence Scope

Committed records are successful appends acknowledged by the owning Store before dependent work proceeds. Recovery covers process interruption while the host retains its storage and treats unfinished effects as unknown. Host failure and power loss remain unverified, since append acknowledgements establish no disk-flush guarantee. Snapshot completeness describes retained records at capture and supplies no stronger persistence guarantee; partial or missing tails remain explicit gaps.

### Dispatch Markers

A dispatch marker is persisted after local validation and session
startup, immediately before a request may leave the process. Failure to
persist that marker prevents sending. The marker proves possible
dispatch, and a crash between the marker and sending remains uncertain.
Monotonic offsets from the proposal's clock describe the request's
interval, independently of the order parallel requests finish.

### Matching Replies

A completed request received a usable MCP result with a matching
JSON-RPC ID. Both regular JSON and streamed responses must match and
contain exactly one usable result or error. A matching JSON-RPC error
is a remote error, a local refusal happened before dispatch, and loss
of a response after possible dispatch is unknown. A completed result's
`isError` and structured outcome remain separate from task success.

### Dispatch Parameters

A dispatch records the actual server label, operation, and bounded
arguments sent from the harness to MCP. Original model intent remains
in the proposal, so normalizing absent arguments to an empty object is
visible. Transport metadata, authorization, and endpoint URLs are excluded.
Later namespace routing belongs to its own implementation boundary.

### Captured Contents

Reply fields are retained whole or explicitly omitted with their byte
count, digest, and reason. Content blocks retain their original indexes.
Binary media uses the conversation's existing part references, with
decoding, unsupported contents, and storage omissions recorded. These
records describe retained contents and omissions, not complete raw JSON.
The model-facing text has its own digest, size, and clipping counts.

### Work Context

Work context uses the admitted `WorkRef`. Authorization tokens and
unrestricted request metadata are never execution evidence. Evidence
versions and unexpected fields are refused rather than inferred.

### Validating a Conversation

`evidence::index` joins records from one native conversation by appended
proposal and call position. It refuses orphaned, duplicate, out-of-order,
and impossible transitions, including normalized call IDs that collide
within a proposal. Missing transitions remain missing. Recovery can omit
a finish offset when a new process cannot reconstruct the original
monotonic clock.

### Skill Loading

Skill records attach either to their containing owner event or to a
tool execution. They retain the source read receipt, source and supplied
content identities, and the containing model view. A managed read source
is selected by the host's checked startup binding. An unregistered
service's receipt remains service-asserted. Neither a tool-side whole
read nor a familiar path proves that the whole skill reached the model.

### Model Requests

A model request's record holds digests of the system prompt, the tools
and every message it sent, with each message's sources. Its projection
is one of two kinds, named by its `kind`. A `whole` projection lists
every message. A `continued` projection names the conversation's model
request before it (`after`), keeps `kept` of that request's messages
from its `from`th, and lists only the messages it adds and their
sources, numbered as its own. A long window would otherwise be listed
again at every request.

### Making a Request Whole

`evidence::index` makes each request whole from the ones before it. It
refuses one naming another request than the one before it, or going on
from one whose projection was omitted. A continued request in a
snapshot missing its start stays unknown.

## Exported Sessions

A local export's versioned manifest classifies every portable update as
native or imported. Native records retain allowlisted metadata and the
runtime's evidence, including internal events with no visible update.
Imported updates remain archival even when their metadata claims native
execution. Importing a manifest never establishes local execution.

### Internal Media

Internal execution and summary events generate no portable message
updates. Their native records retain media part references and report
`InternalMedia` omissions because this export cannot represent their
bytes as neutral Agent Client Protocol (ACP) contents. A complete stored snapshot therefore
does not establish media fidelity or delivery; assertions requiring the
omitted bytes remain unknown.

### Snapshot Completeness

Snapshot completeness describes the stored records that were retained.
Partial log tails, incomplete compact frames, and a missing prefix are
explicit gaps. A store without a completeness guarantee reports unknown.
A complete snapshot can still lack evidence or hold unfinished calls.
Manifest validation refuses repeated sequences, overlapping or missing
update classifications, out-of-bounds ranges, and unsupported versions.

## Link's own threads

`link` is Link's own connection, [`lnk agent chat send`](../../../../../../../../../agent/docs/agents/chat/README.md#talk-to-it-here)'s, with no account
and no bot, from the agent's owner alone. Its threads are
`link:<name>`, and `main` unless a chat names one.

### A thread's name

`link_thread` accepts a name of 1 to 64 ASCII letters, digits, `-`, `_`, and `.`, starting with a letter or digit. A single `~` may separate it from a copy suffix with the same character and starting rules, as in `main~hermes`; the complete name still fits within 64 bytes. Other names are refused.

### Checking a thread

`is_link_thread` checks the `link:` connection prefix alone. `check_link_thread` additionally validates the thread name through `link_thread`, refusing another connection or an invalid name.

## What came of a message

`Asked` is what came of a message sent to the harness: its answer,
whole, or that it joined a turn already running, whose answer, on the
message that started it, answers it too.

## A skill

A skill is a folder named for it, holding `SKILL.md`, whose front
matter says its `name` and `description` above its instructions. Link
reads only those two: a harness lists them in its prompt, and its model
reads the rest when a task needs it.

### Its name

`check_name` takes the standard's names: 1 to 64 lowercase letters,
digits and `-`, with no `-` first, last or twice in a row.

### Reading one

`parse` reads a `SKILL.md` into a `Skill`, its name and its description
on one line, and refuses one with no front matter between two lines of
`---`, or whose `name` or `description` is missing or not as the
standard says. A description is at most 1,024 characters.

### Front Matter Values

The reader accepts plain text, quoted scalars, and indented block values
for `name` and `description`. It normalizes whitespace into one line and
cleans display-control characters before returning the metadata. A list,
map, or other unsupported value cannot supply either required field.

### Parser Scope

This is a focused metadata reader, rather than a general YAML document
loader. Other front matter fields and the instruction body remain outside
`Skill`; the harness owns loading the body when the skill is invoked.

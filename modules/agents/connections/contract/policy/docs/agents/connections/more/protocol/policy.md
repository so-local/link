# Connection Policy

`link-connection-policy` holds a connection's opinions as the agent's
specification writes them, and the checks a conversation's tools header takes.
The agent specification, the connection contract and the store contract share
it, so each reads them alike.

## A connection's opinions

`Connection` is one table of an agent's specification: `[connections]` for every
connection, `[connection.<name>]` for one, each key left out [where the
specification doesn't set it](../../../../../../../../agent/docs/config/specs/agent.md). A key or table the type does not know is refused before connection settings reach an adapter.

### What the table holds

`working` is a reaction on your message while the agent answers, and
`steered` one on a message that joined a running turn, both on a
connection that has reactions. `stream` shows the answer as it's
written, and `pace` how often it's shown again (`"1s"`), never faster
than the platform allows. `activity` says how each tool the agent calls
shows: `replace` above the answer being written, `keep` above the
finished answer too, or `off`. `activity_mark` leads each line and
`activity_separator` stands between the call and its detail.

### Groups and parts

`groups` says which of the owner's messages in a group the agent
answers: `mention`, those that mention it or answer one of its
messages, or `never`, direct messages alone. `images`, `documents` and
`voice` say whether a message's images, PDF or text documents and voice
notes reach the agent; left out or false, each is left out and the
agent told so.

### Durations

`duration_ms` reads a specification's durations, such as `"10m"`, as
milliseconds, the same way for every specification.

### Owner Wait

`ask_wait` supplies the shared owner-response wait used by agent setup and
webhook questions. The specification accepts it only in `[connections]`, from one
second to one day, so choosing a delivery connection does not change that
wait. Approval domains can supply their own wait policy. The
[owner-question contract](../../../../../../docs/agents/connections/more/protocol/owner-questions.md)
owns authenticated targeting and response delivery.

## A conversation's tools

A conversation may be narrowed to some of its tools by [the
`Link-Tools` header](../../../../../../../../harnesses/portability/contract/docs/agents/harnesses/more/protocol/endpoint.md#a-conversations-tools), a list of tools' names separated by commas.

### A tool's name

`is_tool_name` takes a name of 1 to 128 letters, digits, `_`, `-` and
`.`, and the header names at most `TOOLS_MAX`, 256 tools.

### Writing the header

`tools_header` writes a list of tools as the header, and refuses a name
it may not hold, or more than 256 of them, saying which.

### Reading the header

`tools_of` reads the header back: each tool once, in its order, and
none for an empty header. A header that isn't a list of tools' names,
or names more than 256, reads as nothing at all, so it's refused rather
than taken as every tool.

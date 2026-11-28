# Model Actions

An agent's model has one API, whatever serves it. Link Harness uses it
for inference and model discovery. An outside harness can keep its
provider's native carrier; provider requests and credentials stay in
the model bridge.

## The Grant

The proxy supplies `LNK_MODEL_URL`, including its URL secret. Append
an action to it. A request cannot supply a provider, address or key:
its proved grant selects those. Model names must be plain names and
fit the agent specification's allow list. A fallback has a separate grant and
URL, `LNK_FALLBACK_MODEL_URL`.

| Method | Action | Request | Answer |
|---|---|---|---|
| GET | `show` | Optional `?name=<name>` | [Model details](details.md) |
| GET | `list` | None | Sorted plain model names |
| GET | `status` | None | `ready`, and plain `loaded` names when known |
| POST | `chat` | `chat::Chat` | Newline-delimited `chat::Stream` events |
| POST | `embed` | `texts`, optional `name` | `vectors` |
| POST | `transcribe` | `name`, `audio` | `text` |
| POST | `count` | `text`, optional `name` | `tokens` |
| GET | `usage` | None | Requests, unmetered requests and token totals |
| POST | `context` | Optional `chat` | [Kept chats](../context.md) and one chat's parts |

`show` also includes known numeric `facts`, including answer limits,
thinking support and price, when available. The types are in `link_model_contract::chat` and `api`. Unknown fields
are refused. `chat` takes a system string, messages, optional tools,
answer and window token limits, and optional thinking settings. A
message has a `user` or `assistant` role, text, and optional calls,
tool answers, thinking blocks and media. Media bytes are base64, with
an explicit MIME type; they are checked before a provider sees them.
The bridge applies its known price instead of trusting a caller's.

## Streaming

A chat stream contains `text` events followed by one `answer`, or an
`error` with a closed failure kind. Its completed answer includes tool
calls, usage and thinking. A truncated stream has no completed answer.
Cancelling a client request ends its provider request; no inference is
retried automatically. Unsupported operations, including token
counting where the provider exposes no tokenizer, return HTTP 501.
Counts are never guessed. Usage is the calling agent's measured
lifetime total; it does not invent a daily budget or remaining tokens.

### Failure kinds

A chat that fails says one of four kinds, in Link's own words with what
shows or fixes it. `unreached` means nothing answered, such as a local
runtime that isn't running. `too_long` means the prompt is past the
model's window, `overloaded` that the model answered 429 or a 5xx, and
`refused` that its provider or runtime turned the request down. The
[Link Harness](../../../../../harnesses/link/docs/agents/harnesses/link/guide.md)
falls back to its second model on the first three.

#### Refused before the stream

A call refused before its stream keeps what the sandbox or bridge said,
with its status. A 429 or 503 is overloaded, and any other refusal is
refused. A 502 or 504 is unreached: the sandbox couldn't reach or start
the model module's bridge, and [`lnk mod list`](../../../../../../../docs/more/modules/guide.md#get-help) is named as the next
step.

## Limits

Requests are at most 32 MiB. Provider answers and chat streams are
bounded at 8 MiB; discovery and small reads have tighter registry
budgets. The bridge refuses redirects and uses action-specific time
limits. Provider errors returned through these routes do not include
host addresses, paths or credentials.

## CLI and Commands

`lnk model show`, `usage`, `context`, `chat`, `embed`, `transcribe`, and `count`
use the same API at `LNK_MODEL_URL`. Request actions read typed JSON
from standard input, or `--request <file>`. A person reads each answer
as fields, and chat's text as it streams, then the rest of its answer.
With `--json` they print the JSON, chat's a stream event a line.
Each [`lnk agent model`](../../../../../agent/docs/agents/README.md#models) command uses the grant of the agent its `--agent` names.
Agent commands spell the actions `/model:<action>` and take JSON for
structured requests. The registry generates their discovery names.

### Request Inference

`lnk model chat`, `lnk model embed`, `lnk model transcribe`, and `lnk model count` submit the typed request for their action. `lnk agent model chat`, `lnk agent model embed`, `lnk agent model transcribe`, and `lnk agent model count` select a named agent’s model grant instead of requiring the caller’s model endpoint.

### Inspect the Grant

`lnk model usage` reads measured usage, and `lnk model context` reads retained context. `lnk agent model usage`, `lnk agent model context`, `lnk agent model status`, `lnk agent model list`, and `lnk agent model show` inspect those values through the selected agent’s grant. Context details are explained in [Context Reporting](../context.md).

## Owner Discovery

`lnk model list --json` prints plain names, as the route does. The owner
runtime inventory, including addresses, is `lnk model runtime list --json`;
[`lnk agent start`](../../../../../agent/docs/agents/README.md#start-your-agent) uses it. `lnk model show <name>` returns the same
`Shown` type as the route to an agent, granted a runtime. Run by its
owner, it prints the model's known facts, including their overrides, as
blocks for a person and the public `Shown` object with `--json`.
[`lnk issue create`](../../../../../../../core/cli/docs/tooling/issue.md#report-a-bug) reads that JSON for a model named in a failure.

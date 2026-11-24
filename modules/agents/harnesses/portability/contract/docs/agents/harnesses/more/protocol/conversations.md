# Conversations

The commands that show what a harness tells its model and the
conversations it keeps, and the typed Link conversations that a
harness may answer as well.

## `lnk-<name> instructions [--json]`

For an adapter whose contract says `prompt`: prints the system prompt
its harness sends its model now, exactly, from what `configure` last
kept and the agent's Home and own folder as they are; with `--json`,
`{"system", "sections": [{"name", "owner", "changes", "from", "bytes",
"max", "cut"?}]}`, each section of it in order. Runs in its home, Home
and the own folder's instructions, skills and `memory/` (the agent's
notes) read, nothing else of the agent's, with no network. Read by `lnk
agent show --instructions`.

## `lnk-<name> conversation <thread>`

For an adapter whose contract says `conversations`: prints conversation
`<thread>` (`telegram:42`) as a person reads it, from its events
wherever its harness keeps them (Link Harness:
[`link_store_contract::store::copy`](https://github.com/so-local/link/blob/main/modules/agents/stores/contract/src/store/copy.rs),
what `conversation.md` holds). Runs as `instructions` does, the programs
of the [store
drivers](../../../../../../../../stores/contract/docs/agents/more/stores/protocol.md#store-drivers-lnk-name)
the agent's conversations are kept by read too. Read by `lnk agent
chat show`.

## `lnk-<name> conversations move <way>`

For the same adapters: moves each of the agent's conversations kept
another way than `<way>` (its specification's `[storage] conversations` now:
`log`, `compact` or a store driver's name) to that way, and prints each
it moved. The agent is stopped. Runs as `conversation` does, its
conversations written as its harness writes them. Read by `lnk agent
chat export --format`.

## Typed Link conversations

A harness's `contract` may additionally advertise `conversation_history`
and `conversation_fork`, false unless present. The types are generated
from [the shared remote schemas](../../../../../../../../../../packages/api/schemas/remote.yaml).

- `lnk-<name> conversations list --json`: `RemoteConversations`, the
  agent's `link:` threads and whether it can fork them.
- `lnk-<name> conversation <thread> --json`: `RemoteConversation`, the
  user and assistant text with original sequence numbers and times.
- `lnk-<name> conversations fork <thread> --before <seq>`:
  `RemoteConversation` for a fresh thread, containing the immutable
  events before that owner's message, with its tool restrictions.

[`lnk agent chat list --json`](../../../../../../../../agent/docs/agents/harnesses/use/README.md#conversations-that-diverged), `lnk agent chat show <thread>
--json` and `lnk agent chat fork <thread> --before <seq>` run
those commands inside the agent's confinement, with its store drivers.
A harness lacking the capability is refused explicitly.

### A chat as it streams

[`lnk agent chat send --stream-json`](../../../../../../../../agent/docs/agents/chat/README.md#talk-to-it-here) prints each event of its stream as a line
of JSON, flushing each as it comes: a `text` record for each piece of
the answer, then one `answered` record holding the whole answer, its
chat, and what was held. The command's exit status says whether it
finished, and a remote service adds a terminal done or error record.
This flag conflicts with `--json`, which prints the `answered` record
alone.

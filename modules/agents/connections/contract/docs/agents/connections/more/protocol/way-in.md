# Agent Way In

For a harness whose contract says `from`, `lnk agent bridge` listens at
`~/.lnk/safe/agents/<agent>/way-in.sock` (600, `link_connection_contract::way_in`)
for messages from others. Each is a bridge's call whose action is
`link_connection_contract::Firing`, proved as the agent whose scheduler
or `run` hands it (`link_connection_contract::fire`).

## A firing

`Firing` contains `thread`, `from`, `text`, `answer`, `hops` and optional `tools`, `model`, `occurrence`, `parent`, a subagent's [lineage](../../../../../../../harnesses/portability/contract/docs/agents/harnesses/more/protocol/endpoint.md#parent), and `delivery`;
`answer` one of `"chat"`, `{"thread": "<name>"}`,
`{"connection": "<service>"}`, `{"ownerthread": "<thread>"}` or `"nowhere"`. A connection selects the
owner's direct chat on that connected service; `cli` selects the CLI
inbox, delivered to an active or next opened CLI chat. The
bridge answers with one line, `link_connection_contract::Fired`
(`{"answer", "error"?}`), once the answer went.

## Delivery Correlation

Source turns opt into durable result receipts through Agent Client Protocol (ACP) metadata. A completed result returns `resultReceipt`, which remains recoverable until acknowledged through the recovery protocol. The caller supplies a stable `delivery` identity on retries; the host retains the original source input once and refuses changed content. Schedule `occurrence` identifies one exact definition revision and firing time. Neither identity grants execution authority.

## Whose firings are taken

Its own agent's are taken whoever they're from. Another agent's are
taken only from itself (`from` `agent:<its ID>`), a teammate, into the
receiver's conversation with it (`agent-<its ID>`) or a team's they
share (`team-<team ID>`, `link_connection_contract::teammates_thread`,
`teams_thread`), its `answer` `"nowhere"`. Any other is refused before
the harness sees it.

## A teammate's

A message from another agent (`agent:<id>`), whoever hands it, is
refused unless the two [share a team](../../../../../../../agent/contract/team/docs/agents/more/interface/contract/team.md#on-a-chat), as the teams' files say when it
comes, nor past its conversation's chain.

## A stranger's

A stranger's message (`link_core::access::Standing::Stranger`, a
sender neither the owner's own nor let in, `webhook:<name>`) is given
only the tools its specification's `[tools] strangers` names: its `tools`
narrowed to them, or set to them when it has none.

## What a stranger's is refused for

A stranger's message is refused for a harness whose contract doesn't
say `tools`, for a `thread` other than its sender (`webhook:<name>`),
and for an `answer` other than `"nowhere"`.

## Where the answer goes

The answer goes where `answer` says: to the chat its thread is
(`lnk-<connection> send`), to another conversation as a message from the
same sender, one hop further (a subagent's answer, to its parent's), or
nowhere.

# Connection Contract

`link-connection-contract` is the shared agreement between the agent module and each connection module. It defines a connection's settings, the protocol of its message bridge, the messages pushed to a machine that packs agents, and the terms a way in takes a message on.

<!-- docs-tree:start -->

## Docs

### Packages

- [Connection Policy](policy/README.md): `link-connection-policy` holds a connection's opinions (its reactions, streaming, pacing, and groups) and the checks of its settings, pacing, and tool headers

### More

- [Interface](docs/agents/connections/more/protocol/README.md): Every connection module answers a fixed set of commands for `lnk agent`
  - [Pushed Messages](docs/agents/connections/more/protocol/pushed.md): A machine that packs agents has a connection's messages pushed to it over HTTPS, or held for it, instead of a bridge polling them
  - [Agent Way In](docs/agents/connections/more/protocol/way-in.md): For a harness whose contract says `from`, `lnk agent bridge` listens at `~/.lnk/safe/agents/<agent>/way-in.sock` (600, `link_connection_contract::way_in`) for messages from others
  - [Owner Questions on a Connection](docs/agents/connections/more/protocol/owner-questions.md): A connection tells Link which of its chats are the owner's own, so the agent's bridge can ask the owner there what only they can answer
  - [Formatted Answers](docs/agents/connections/more/protocol/formatting.md): An agent writes its answers in Markdown, and each connection shows them in its chat's own formatting: bold, code, links, lists, and tables as the chat draws them

<!-- docs-tree:end -->

## Code

- [`connection.rs`](src/connection.rs): the connection contract
- [`connection/pushed.rs`](src/connection/pushed.rs): messages pushed, not polled
- [`connection/terms.rs`](src/connection/terms.rs): the terms a message is taken on
- [`connection/webhook.rs`](src/connection/webhook.rs): an agent's own webhooks

# Validator Protocol

The webhook validator receives signed posts from the agent bridge and returns an acceptance verdict. It runs outside the agent’s sandbox without acquiring a delivery endpoint of its own.

## How it runs

The [shared `sift` contract](../../../../../../contract/docs/agents/connections/more/protocol/pushed.md#lnk-connection-sift)
owns the request and verdict formats, including this adapter's settings.

### What it never reaches

It receives secrets on standard input, never through arguments or the
environment, and keeps none. It reads nothing else, opens no connection,
and receives neither an agent endpoint nor its proof secret. The bridge
hands an accepted post to the agent itself.

# Decisions

Why Link Harness speaks [Agent Client Protocol (ACP)](README.md) the way it does.

## Why a WebSocket on the endpoint's port?

The harness is one process serving many chats, long before any client
connects, and its endpoint is already where Link's bridges reach it,
proved and keyed. A WebSocket there takes the same headers and the
same key, and carries ACP's JSON-RPC both ways, so a cancel or a
question reaches a turn while it runs.

## Why is a session's id its thread?

A bridge knows the chat a message came from, and a chat is a thread:
naming the session by it lets a bridge take a chat's conversation up
again after any restart, on either side, with nothing kept to map one
to the other.

## Why does `initialize` always answer the first version?

It is the only version Link Harness speaks. ACP has a client that
can't speak the answered version close the connection, which says more
than a version it doesn't hold.

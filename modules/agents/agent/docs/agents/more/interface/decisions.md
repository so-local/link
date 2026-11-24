# Decisions

Why agent actions use one contract across command, tool and route surfaces.

## Actions

Why an agent's commands, tools, routes and CLI come from one registry of
[namespaced actions](namespaces.md), and how the registry is shaped.

### Why is there one kind of action?

An action is a typed request from a caller, answered outside the
caller's walls. A part that relays a protocol it does not type, such as
MCP or inference, is the same action with `carries` in place of
`answer`, and one of the two is required. A scheduled run, a webhook and
a connection's message are callers, not actions: the caller is `Trigger`,
which acts under the grant it was set up with, since it acts later with
no owner there.

#### What it costs

One registry, one way of proving a caller and one table for the proxy
replace a separate kind with its own plumbing for each. The server
branches once, between a typed answer and relayed bytes. Every place
typing cannot hold is one search for `carries`.

### Why are the names two parts?

A name is `<namespace>:<action>` as a command, `<namespace>__<action>`
as a tool and `/_link/<namespace>/<action>` as a route, so one action is
found by the same words everywhere. Another harness's familiar spellings
are aliases that resolve to an action and add no handler or permission.

### Why these namespaces?

Each holds one subject and is named in the singular. The ways a message
goes into a conversation, such as `goal`, are `chat`'s
rather than namespaces of their own. `memory` stays apart from `chat`
because it outlives conversations and is a store of its own, and
`browser` and `desktop` are separate from `web` so that two-part names
can hold each.

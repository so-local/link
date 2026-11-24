# Agent Access

`[access]` says who besides its owner may do something with the agent,
each verb a list: chats on its connections, which are groups, or
[principals](https://github.com/so-local/link/blob/main/packages/api/schemas/account.yaml), `service:<id>`, written as Link's access tokens write who
acts.

## One section, many specifications

It is the section every specification shares, each specification taking only the verbs
and kinds it names. An agent's takes one verb, and [a team's file](../../../../spec/docs/config/specs/guides/agent.md#where-its-kept)
another. It is the agent's
own: Link's specification holds none, and a team's file holds only its own verb,
`member`.

## `message`

Who may message it: chats on its connections, `chat:<connection>:<chat
id>`, the id the connection's own (Slack's connection id,
`chat:slack:C0TEAM`; Telegram's group id,
`chat:telegram:-1001234567890`; Discord's connection id). A chat is a
group of whoever is in it, never a principal: anything else is
refused, and so is a chat named twice.

### What a bridge is handed

A bridge is handed the chats of its connection named here only for a
harness that reads who a message is from (its contract's `from`), so
nobody's message could pass for its owner's. In those chats the agent
answers whoever is in them, as `member:<connection>:<user id>` in
`Link-From`, never as its owner.

## Who stands where

A message's sender stands at one of three levels, from its `Link-From`:
the owner, a member or a stranger.

### The owner

The owner, or the agent on their behalf (its schedules, subagents and
background commands), may do all the agent can.

### A member

A member is let in by its owner: someone in a shared chat (`user:`),
or a teammate (`agent:`). A member may do all the agent can, never as
its owner.

### A stranger

A stranger is anyone else (`webhook:<name>`), with only the tools
`[tools] strangers` names, by the names the harness offers them, none
when it's left out. Its message carries that list always (`Link-Tools`),
so its conversation never has more, and a harness that can't narrow a
conversation's tools (its contract's `tools`) never takes one.

### A stranger's conversation

A stranger's message goes only to its own conversation, named as the
sender is (`webhook:<name>`), and is answered nowhere.

### What `strangers` never names

`strangers` never names the scheduler or `run`, nor one of their tools:
a schedule, a subagent or a message the agent sends is its own, and a
command runs as it, so a stranger's standing would be lost through
them. A team's specification never sets `strangers`: it's the agent's own.

<!-- spec-reference:start -->

## Checked Reference

These fields are checked against the released type. An unset optional
value waits for its feature to resolve it.

```text
root: member, message
```

### Checked Defaults

These values come from the built-in defaults, including nested settings.
Omitted optional values have no stored default at this layer.

```text
message = []
member = []
```

<!-- spec-reference:end -->

# Schedule Definitions

`schedules` holds the agent's schedules: what fires when, and where it
goes. Each is the [Agent State Protocol
(ASP)](../../../../../../../harnesses/portability/agent-state-protocol/docs/agents/harnesses/more/agent-state-protocol.md#schedules)'s
schedule without its identity, which the agent's saved record keeps. In
an agent's saved configuration these entries are under `behavior`. A
standalone specification can define a recurring job like this.

<!-- spec-reference: link_agent_spec::Spec -->

```toml
schema = 1
[[schedules]]
name = "Morning inbox"
when = { kind = "cron", cron = "30 8 * * 1-5", zone = "Europe/Lisbon" }
message = "Summarise new client mail."
to = { kind = "owner", connection = "telegram" }
enabled = true
```

## Destinations and Cadences

`to` can name an owner's direct connection or an exact `thread`, using
its complete connection/chat identity. Without a connection, an owner
destination uses the CLI inbox. An `every` cadence has positive `seconds`
and an `anchor` in Unix seconds. An `at` cadence has one Unix instant.
Two schedules of one name and malformed destinations are refused.

## Identity

Which schedule each is to its runner is the agent's saved record's
(`~/.lnk/safe/agents/<agent>/schedules.json`), by its name: its id, its
revision and the native job it came from (`origin`), kept when migration
or a harness switch carries one in. One the record lacks is `spec:<name>`
at revision 1. A changed definition is published at the next revision,
so the scheduler tells a change from a retried import.

<!-- spec-reference:start -->

## Checked Reference

These fields are checked against the released type. An unset optional
value waits for its feature to resolve it.

```text
root: enabled, message, name, to, when
to: connection, kind, thread
when: anchor, at, cron, kind, seconds, zone
```

<!-- spec-reference:end -->

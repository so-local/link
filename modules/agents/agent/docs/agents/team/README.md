# Create an Agent Team

> Agent Teams Give Every Agent a Purpose

A team splits a job by purpose. Each agent keeps its own model, connections,
and files, sends the others messages, and shares a folder with them.

<!-- quickstart -->

## Make a Team

Create the member agents first with `lnk agent create <name>`, then name them when you create the team.
The [start guide](../start/README.md) explains how to create agents and select their models.
`--write` and `--read` grant each member named those folders of the team's folder, and `.` means all of it.

```sh
lnk agent create scout
lnk agent create writer
lnk agent create checker
lnk agent team create research --with scout --with writer --with checker --lead scout --write .
```

## Manage Members

Set a member's purpose and folder grants, inspect the team's members, or remove one from the team.
A member added with neither `--write` nor `--read` reaches none of the folder, and a member already on the team keeps its grant.
An agent keeps its own identity and files when its team membership ends.

```sh
lnk agent team add research --member checker --purpose "checks every claim" --read . --write notes
lnk agent team list
lnk agent team show research --member scout --json
lnk agent team remove research --member checker
lnk agent harness check --harness link --agent scout
```

On a cloud that hosts teams across accounts, a member can also be another
account's agent. `lnk agent team-member add` invites that agent, and its owner
joins, as [Team Members from Other Accounts](team-members/README.md)
explains.

## Send Messages Between Members

On Link Harness, a member sends another member a message with its scheduler.
The answer comes back as a message from that agent, never from
you. The scheduler's caps stop two members that keep answering each other. A
new agent's scheduler is off, so `lnk agent team create` and `lnk agent
team add` turn it on for a member whose harness honors it, and say so.
You talk to each member on the member's own connections.

## Talk to the Team

Address the team to reach its selected lead, or name a member explicitly.
The member name belongs to this team and resolves to that agent's permanent
ID. Local Link refuses a message to an unknown member, or to a team without a selected lead.

```sh
lnk agent chat send --team research --message "Plan the launch"
lnk agent chat send --team research --member writer --message "Draft the announcement"
```

### Talk to a Hosted Team

Use `research@my-cloud` or add `--cloud my-cloud` for a hosted team.
The hosting service resolves the current member for every message.
Owner chat reaches your own agents. Invited agents of another account
use the existing teammate messaging and their owner's connections.

## Share the Team's Folder

The team's folder is `~/Link/Teams/<team>/shared`. Each member reaches
only the parts of the folder the member is granted, and no other team's folder.

### Grant Folders Explicitly

Membership alone grants no file access. Each member's `sandbox.toml`
names the team's folders under `[teams.<team>]`, with separate `read`
and `write` lists. These explicit grants add to the member's own `[files]` grants
without opening the shared parent or its other children. Local Link refuses a folder named
by both `--write` and `--read`, since writing a folder
includes reading it.

### Know When Members Restart

A member takes a change at its own start, so a team command
restarts each running member it changed, once. A changed grant restarts
only that member, and a new purpose restarts none. A member joining,
leaving, or taking another name restarts every running member, since
each scheduler reads whom it may message as it starts.

## Share the Team's Memory

A team can have one memory over its whole folder, which its members
search on Link Memory. [Team Memory](team-memory/README.md) says how to
turn it on, who gets it, and what it needs installed.

## Its File

The team's file, `~/Link/Safe/Teams/<team>.toml`, holds the members and their
shared [opinions](../../../contract/spec/docs/config/specs/guides/agent.md#where-its-kept).
Shared opinions cannot grant chat access to a member. Each member's own agent specification
holds `[access] message`. A team's `[access] member` controls which other
accounts' agents may belong to that team.

### Reconcile, Start, and Stop

`lnk agent team create --spec <file>` reconciles declared members and settings.
`lnk agent team start <team>` and `stop <team>` preserve the members' identities and files.
For a local team, every member must be here before either command changes a service.
If a member needs a model and no terminal can answer, start gives
`lnk agent model set <model> --agent <member>` to choose that member's model
before trying the team again.
The [Team Specification](../../config/specs/team.md) explains these controls.

## Inspect or Delete a Hosted Team

`lnk agent team show research --cloud my-cloud` shows the team's members and purposes,
distinguishing accepted members from invitations. Add `--member scout` to select
one member or `--json` for structured output. `lnk agent team delete research
--cloud my-cloud --yes` ends membership while retaining the agents and shared
folder. Both commands also accept `research@my-cloud`. An explicit cloud selects
the hosted team even when a local team has the same name.

## Read Teams as JSON

`lnk agent team list --json` prints one line of JSON per team, `{"name",
"resource", "lead", "shared", "members", "memory"}`. `resource` is the
team's one ID, a group's, `group:<id>`. Each member is `{"name", "workload", "agent",
"purpose", "running", "write", "read"}`. `workload` is the member agent's ID,
`agent:<id>`, and `agent` is that agent's name here.

### Hosted Teams as JSON

A team a cloud account hosts is listed as `{"name", "resource",
"account", "members", "shared"}`. `lnk agent team show research --cloud
my-cloud --json` prints `{"name", "resource", "account", "version",
"members", "lead"}`, each member `{"name", "workload", "owner",
"purpose", "joined"}`.

## Keep Teams Together and Safe

Move members and their shared folder together across machines with [Move a Team](move.md). Team access is in [Security](security.md), and membership and shared work are in [Decisions](decisions.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Move a Team](move.md): Move a team and its shared folder to a cloud that hosts teams, or bring the complete team back to this computer
- [Team Memory](team-memory/README.md): A team's memory searches its whole folder for its members, on Link Memory
- [Team Members from Other Accounts](team-members/README.md): A team on a cloud that hosts teams across accounts can take an agent another account owns

### More

- [Decisions](decisions.md): Why teams share a roster, address a lead and retain one authoritative shared folder
- [Security](security.md): Who can change team membership and what a teammate's messages and shared-folder grants allow

<!-- docs-tree:end -->

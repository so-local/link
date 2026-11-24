# Team Specification Guide

How a team's file makes its roster, what its members share and how other accounts' agents join. Each field is in the [Team Specification](../../../../../../../docs/config/specs/team.md) reference.

## Roster and lifecycle

Member definitions describe existing agents or new managed agents, and
the same roster supplies lifecycle controls.

### `[member.<name>]`

Each member by its unique name in the file, with its agent's `id` and a
`purpose` of one line: what it's for on the team, for people and its
teammates (`link_agent_team::Member`). An unknown key is refused.
Removing the last member leaves the team in place with no member table;
its shared folder and opinions stay available for members added later.

### `[member.<name>.agent]`

A managed member's own partial Agent Specification, for example
`model = { default = "local/qwen3:8b" }`. A new managed member may omit its ID;
the desired definition must receive and persist permanent IDs before it
can authorize membership. Existing IDs cannot be reassigned. A member
definition cannot claim an owner or membership, name another agent, or
configure another account's agent.

### Apply and lifecycle

[`lnk agent team create --spec team.toml`](../../../../../../../docs/agents/team/README.md#make-a-team) validates the complete
roster against existing agents and other teams before saving it. It
assigns IDs to new managed members and saves those identities before
creating their settings. Repeating an apply resumes the same agents.
Managed members read their agent definition directly from this document
at each start or hosted dispatch. Their standalone configuration stays
available when they leave the team. Independent members without a
definition keep using their own configuration.

### Standing up and down

[`lnk agent team start research`](../../../../../../../docs/agents/team/README.md#its-file) starts the roster's local members.
`lnk agent team stop research` stops them and their bindings, preserving
agent identities, settings, conversations and shared files. Applying a
roster restarts each running member once; new members start when asked.
Removing a member detaches it without deleting its agent or files.
An away member must be moved here before changing its local lifecycle.

### Hosted lifecycle

Add `--cloud my-cloud` to apply a definition or start/stop a hosted team.
The host reserves new IDs before creating agents and returns the canonical
document. New members remain stopped until started. A team owner controls
their own members; invited agents retain their account's lifecycle
authority. Stopping is available after a hosting plan expires.

## Team settings

Shared opinions, resource policy and folder access are separate from
the agent definitions in the roster.

### `[settings.agent]`

Shared Agent Specification opinion tables sit beneath each member's own opinions.
Identity, harness, creation environment and membership remain each
agent's own. They are read only from here, so an opinion table at the
top of the file is refused as an unknown key. A schema 1 file kept them
at the top, and reads with them moved here.

### `[settings.memory] enabled`

`enabled` defaults to false: a team's folder has no memory over it until
[`lnk agent team-memory start research`](../../../../../../../docs/agents/team/team-memory/README.md#turn-it-on-and-off)
sets it, or `lnk agent team create --spec` with `enabled = true` or
`--settings.memory.enabled`. Off again, the
team's memory tool is removed and its members' next starts hand them
none.

### `[settings.memory]`

`semantic` defaults to false; enabling it requires positive
`daily_credits`, shared by members. `indexer = { kind = "auto" }`
selects an eligible keeper; `{ kind = "member", member = "scout" }`
selects one member. Current membership and whole-folder write access
remain required. Selection changes revoke writes on the next request.
`payer = { kind = "owner" }` selects the owner's account; the member
form selects a member's account. Naming a payer grants no spending
authority: the service must check that account's consent before billing.

### `lead`

The name of the member a message addressed to the team reaches. It must
name a member in this file. When omitted, a caller must select a member
explicitly; member ordering never selects a lead. Renaming that member
preserves the selection, and removing it clears the lead without
selecting a replacement. Delivery still requires current membership
and authorization for the selected agent's permanent ID.

### `[files] shared`

The folder its members share, an absolute path or under `~`, `~` made
the home folder; [`lnk agent team create`](../../../../../../../docs/agents/team/README.md#make-a-team) makes
`~/Link/Teams/<team>/Shared`. Left out, the team has no folder.

### What each member reaches

What each own member reaches of the folder is in [its own sandbox file](../../../../../../../docs/agents/more/settings/README.md#sandbox-permissions),
never in this file. Applying a specification grants a new member none of it:
[`lnk agent team add research --member scout --write .`](../../../../../../../docs/agents/team/README.md#manage-members) grants it.

### A folder kept by the hosting account

A team whose folder an account hosting agents keeps has one folder, on
that account. A member running on your machine reaches it through its
own files bridge at each call, never through `[files] shared` there.

### IAM group

A team is an IAM group: its roster supplies each member the
`team-<id>` role, granting `team.read`, `team.member` and messaging of
its members. These grants combine with the principal's own policy.
Folder access uses separate read/write grants, checked against current
membership at each shared-file request.

## On a chat

How a chat a connection carries reaches a team.

### The owner addressing a team

On a chat of an agent on a team, the owner's message starting `@<team>`
goes to the team's lead, and `@<team>/<member>` to that member by its
unique name in the team (`link_connection_contract::addressing`).

### Through the owner's team chat

It goes through the owner's own team chat (`lnk agent chat send --team <team>
--member <member> --message -`), in the team's conversation, and its
answer comes back on the chat, `@<team>/<member>: ` before it.

### What's refused

An unknown, removed or unaccepted target is refused there and said back
before anything is delivered. Nothing is broadcast, and nothing goes to
another member instead. Only the owner's messages are addressed: a
member's or a stranger's is the agent's own, as before.

### A chain of agents' messages

Each conversation counts the other agents' messages it took since a
person last spoke in it (`link_interactions::chains`, kept on the
machine beside its interactions).

### Past the hops

Past the receiver's `[tool.scheduler] hops`, another agent's message is
refused until a person speaks there, as one whose own `hops` are past it
is. A chain holds only on a chat a connection carries (`<connection>:<chat>`),
where a person reads along. A conversation only agents write in
(`agent-<id>`, `team-<id>`) keeps no chain, since no person speaks
there to end one. Each message's own hops bound it instead.
Either refusal is the way in's `refused` failure, final, so the sender
never tries it again.

### When it starts again

A person's message, the owner's or a member's, ends the chain. One the
harness never took counts nothing.

## Where it's kept

`~/Link/Safe/Teams/<team>.toml`, named after the team, made and changed by
[`lnk agent team`](../../../../../../../docs/agents/team/README.md#manage-members). Member definitions and team settings share this one
document; observed placement does not replace the desired roster.

### A member moved alone

The file, the shared folder and the conversation's relay stay where the
team is kept when one member moves. `link_agent_team::moving::check`
decides, from the current roster, whether a destination keeps the
member's ID, its place as lead, the folder and the conversation. It
returns the member's role and folder grant derived again, or a refusal
naming the whole team's move.

### Its ways back

The ways back from a destination are a `Paths` seam. `Direct` reaches
a team only where it's kept, as for a team kept on a machine. [`Ways`
reaches a cloud's team from a machine](../../../../../../../docs/agents/team/move.md#one-member-alone),
its folder where the cloud names a folder service and its conversation
where it serves a team relay.

## Another account's agent

On a cloud that hosts agents of many accounts, the team's owner names
another account's agent twice, each checked against the other: in
`[access] member` and in its `[member.<name>]`.

```toml
schema = 1
id = "t4m9q2x7k1ab"
name = "design"

[member.lead]
id = "a1b2c3d4e5f6"

[member.reviewer]
id = "k3m4n5p6q7r8"
owner = "acct-0b7c5e2d4f6a8b9c0d1e2f3a4b5c6d7e"
purpose = "reviews drafts"
read = ["."]

[access]
member = ["service:k3m4n5p6q7r8"]
```

### `[access] member`

It holds `service:<agent id>`, the service each such agent acts as,
[IAM's one list of who outside may be on it](../../../../../../identity/docs/config/specs/access.md).

### Its `[member.<name>]`

The same agent's `id` and `owner` (the account), its `purpose`, and
what of the folder it reaches at most, `write` and `read` as
`[teams.<team>]` names them. The team's owner decides that, since the
files are theirs.

### What's refused

A member with an `owner` whose agent `[access] member` doesn't name, an entry
there that no member has, and `write` or `read` on the owner's own
agent are each refused.

### Its own account's yes

It's on the team only once its own account joins as well: that yes is
the other account's, never in this file.

<!-- spec-reference: team -->

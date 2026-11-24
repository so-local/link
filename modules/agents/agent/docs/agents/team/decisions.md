# Decisions

Why teams share a roster, address a lead and retain one authoritative shared folder.

## Teams

What a team is, how its agents message each other, who answers you in
one, and where its folder stays when a member moves.

### What is a team?

A roster of agents working together, each with a purpose: a file of its
own (`~/Link/Safe/Teams/<team>.toml`) listing its members by ID, each with a
name in the team and a purpose, a folder they share, and the opinions
they share, a specification beneath each member's own.

#### Where it's kept

It's kept on the machine like an agent's settings, outside every
sandbox, and only `lnk agent team` writes it, so no member can put
itself on a team or another on its. The commands are `lnk agent team`,
not a group of their own: a team is agents' settings, which the harness
module keeps, and a module of its own would read another's files. A team
can also include explicitly invited agents on a host that supports
cross-account teams, with both owners' consent.

#### One document

A team's file holds two things, its members and its settings, and
nothing else configures it. Start, stop, a hosting account and a move
each read the roster from it by permanent IDs. Where a member happens
to run is observed, never written back as a second roster, so the file
you edit is always the team you get.

### Why does a member have a purpose, not a role?

Because what a member is for is a line in words, for people and its
teammates' prompts, and grants nothing. In access control a role is a
policy, what a holder may do, so the word is kept for that. A team's
file with a member's `role` is refused, naming `purpose`, the key it
takes.

### How do a team's agents message each other?

Through Link Scheduler, as subagents and schedules do: `send`, to a
teammate by name or to the whole team. Its runner hands the message to
the teammate's bridge, in that agent's conversation with the sender (or
the team's), marked as from `agent:<id>`, never from you, and the
teammate's answer comes back to the conversation it was sent from, as a
message from it.

#### Its answer

The answer comes back as a job of its own, so failing to deliver it
never sends the message twice.

#### Who may send to whom

Who may send to whom is the teams' files': the scheduler is told an
agent's teammates at its start, and each bridge reads the files again as
a message comes, taking one from another agent only when the two share a
team. No message goes through a harness, and no process waits on
another.

#### A teammate elsewhere

A teammate the runner reaches on none of its ways in, such as one on
another machine of a team a cloud hosts, is reached through that
cloud's [team relay](../../../../tools/scheduler-tool/docs/agents/tools/scheduler/more/developing.md#the-team-relay).
The member signs in to it as itself, with its own agent token, never as
its machine, so the relay takes the sender from the token and checks
membership at each message. Agents never hear each other through a
connection such as Slack, where every other bot stays refused: a bot's
message there can't prove which agent sent it.

### What stops two teammates answering each other for ever?

The scheduler's caps, as for subagents: each message carries its hop
count, a message and its answer each one more, and `send` refuses past
`[tool.scheduler] hops` (Link's 8), saying so to the model; the cap a
day (`per_day`, Link's 100) bounds how many messages an agent sends in
all. A team has no cap of its own: its file can set the scheduler's keys
for its members, as any opinion.

#### On a chat

A chat a connection carries, such as Slack or the owner's direct chat
where a teammate's answer lands, has a person reading along, so a chat
[counts the other agents' messages](../../../contract/team/docs/agents/more/interface/contract/team.md#a-chain-of-agents-messages) it took since a person last spoke in
it, and refuses one past `hops`.
A person speaking ends the chain, since a person reading along is what
the cap stands in for.

#### Who keeps the count

The count is the receiving machine's, outside
the model, so no agent can reset it. A conversation only agents write
in (`agent-<id>`, `team-<id>`) has no person to end a chain, so only
each message's own hops bound it, checked by `send`. A refusal is final
either way: the sender's scheduler never tries it again and tells its
model why.

### When does a change to a team take effect?

At each member's next start, which `lnk agent team` makes for the
members that run. What a team changes (an agent's specification, its tools, the
folders its sandbox reaches, whom its scheduler may send to) is fixed
for a start, so the prompt and the tools never change inside a turn, and
the provider's cache holds between them. The one thing read again each
time is who may message whom, by the bridge, so a member taken off a
team is refused at once.

### Who answers you in a team?

Each member, on its own connections: a team has no connection of its
own. You give a task to the member you text, it passes parts to the
others with `send`, and their answers come back to it, so the one you
asked answers you.

#### The team's address

Addressing the team reaches its `lead`, and naming a member reaches
that member alone, by its unique name in the team. A target that is
unknown, ambiguous, removed or not yet accepted is refused before
anything is delivered, never broadcast and never handed to another
member. A task given to the wrong agent acts in its name, so a refusal
you can see is safer than a guess. With no lead, a message must name a
member: the roster's order never picks one.

### Where is the team's folder when its members are apart?

Where the team is kept, never on a member's machine, so the folder
doesn't move when a member does. A team a cloud hosts keeps its folder
in the owner's storage, and a member elsewhere reaches it through [that
cloud's folder service](../../../../stores/contract/docs/agents/more/stores/files-bridge.md#a-teams-folder-elsewhere), one call at a time, with its own agent token.
A storage credential handed to the member's machine couldn't be held to
the member's folders or taken back at its next call, and the service
checks both.

#### The owner's own member

The owner's own member holds the whole folder at the service, as the
owner does. Its own machine's files bridge holds each call to its
sandbox file before it's sent, since only that machine keeps the file.

### What happens to a team when one member moves?

One check, from the team's current file, decides before any cutover
whether the destination keeps the member's ID, its place as lead, the
folder and the conversation. `lnk agent move` and a cloud hosting the
team ask the same one. A destination that can't keep them is refused,
naming the whole team's move, rather than moving the team silently or
leaving the member cut off. The member's grants are derived again from
the roster there, never copied, and messages held during the move reach
only the agent that arrived by it.

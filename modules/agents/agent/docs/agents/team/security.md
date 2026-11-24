# Security

Who can change team membership and what a teammate's messages and shared-folder grants allow.

## Teams

Who may put an agent on a team, and what a teammate's message may do.

- A team's file is in Link's settings (`~/Link/Safe/Teams/`), which
  no sandbox reaches. Desired member definitions grant no membership
  before stable IDs are kept, and cannot claim an owner or another
  agent's identity. A payer setting grants no spending consent.
- Explicit apply checks every local member's effective specification before
  saving its resolved roster. Managed behavior is read from that file;
  standalone agent settings and files survive standing the team down.
- A message from another agent (`agent:<id>`) is taken only when the two
  share a team, as the teams' files say when it comes: the bridge reads
  them again for each, so an agent taken off a team is refused at once,
  whatever its scheduler was told at its start.
- Each team is a role its members hold, granting `agent.message` on
  each of them: the way in asks `link-policy`'s `allows` whether the sender
  may message this agent, as every check is asked.
- A teammate's message is [a call proved with the sender's own key](../more/interface/security.md#every-way-out-of-its-sandbox): it's taken only as from
  that agent, never as from another or from a schedule, only into the
  receiver's conversation with it (`agent-<its ID>`) or a team's they
  share (`team-<team ID>`), and its answer goes only back to the
  sender's scheduler, never to a chat.
- A teammate's answer lands in the conversation it was asked from,
  handed to the asker's own way in by the asker's own scheduler. It's
  kept as that agent's, quoted, never yours, and bounded by the same
  hops and cap a day as any message from others.
- The cap a day bounds both ways: what a member sends, each message
  counted once however often it's tried again, and what it takes from
  each teammate, at its own way in, up to its own `per_day` a day (UTC)
  from one teammate, answers to its messages included; past that a
  teammate's message is refused, and none is taken at all when the
  receiver's specification sets no `per_day`.
- A message at the hop cap is delivered, but its answer, one hop past
  it, isn't sent back. A teammate's message is answered with all the
  receiver's tools but `remember`, and the answer goes back to the
  teammate, so a teammate can ask for what the receiver can read or do,
  never have it note something for every conversation after.
- Where the harness is sent others' messages (Link Harness), memory's
  `remember` writes the notes only in a conversation of the owner's, as
  the call names it (`_meta`, the same word the scheduler takes): never
  a teammate's (`agent-<id>`), a team's (`team-<id>`), a stranger's
  (named as its sender) or a subagent's (`task-<id>`), and never in a
  call that names none; it tells the model why.
- A chat the owner shared (`lnk agent share`) is the owner's
  conversation, so its members can have the agent remember, as they can
  make it do all else it can. A subagent can't show that its parent
  conversation was the owner's, so the owner's own subagents don't
  remember either: the conversation that started one notes what it
  found.
- The scheduler sends only to the teammates it's given, through their
  bridges' ways in, Unix sockets only you reach.
- A member reaches only what its sandbox file grants it of a team's
  folder (`[teams.<team>]`), in its harness's sandbox, in `run`'s and
  `file`'s, and nothing of another team's; every other sandbox hides
  `~/Link/Teams`. A grant names folders one level inside the team's
  folder, or all of it, and a folder only read is never inside one
  written.
- Team membership alone opens no shared folder. Folder grants remain
  explicit in each member's sandbox file, separately from the team's
  message permissions, and a grant for a team the agent is not on adds
  no access.
- A folder granted there is bound only where it really is: one a member
  that writes the folder replaced with a link is left out, wherever the
  link points. A team's shared folder is checked as a files folder is:
  one a hand-written team's file names where writing leads out of the
  sandbox (`~/.local/bin`, `~/.ssh`, Link's settings), or in agents'
  folders, grants nothing.
- Every sandbox hides each team's folder kept elsewhere, so no agent
  starts while a team's file can't be read. A team's memory
  (`team-<id>-memory`) can't be granted as a tool, only handed to
  members.
- A team's memory is off until its owner turns it on, so a shared
  folder alone runs no engine over it.
- A team's memory is a shared tool: one server, outside every sandbox,
  started by the first member's proxy that needs it and stopped when
  idle, in a sandbox of its own that reads the team's folder and its
  program, and writes only its index (`~/Link/Teams/<team>/memory`).
- Only the proxies of members granted all of the team's folder (`.`,
  read or written) name it, since it searches all of it: a member
  granted only some of the folder isn't handed it. Every member's search
  reads the same index, so what one member's files say, the others can
  find: that's what the team's folder is for, and what each member
  writes there is the team's.

# Move a Team

Move a team and its shared folder to a cloud that hosts teams, or bring the complete team back to this computer.

## Move a Team to a Cloud That Hosts Agents

A cloud that hosts teams can move a team and its shared folder together:

```sh
lnk agent team move design my-cloud
lnk agent team move design@my-cloud here
```

### Its Copy and Switch

Run the first command on the machine holding all the team's own members.
The copy runs while they answer. The switch pauses every member, prepares
each member and the shared folder, then activates the complete team. A
failed preparation leaves the source authoritative. A message sent to a
member from the switch until the move is done waits, and its member
answers it once, where it landed.

### One Member Alone

A member, the lead too, moves alone only where its team's guarantees
hold: its ID and place on the roster, the team's one folder where it
stays, and the team's conversation. Before anything moves, the machine
running the move and the cloud ask each team kept there whether the
destination reaches its folder and conversation. A destination that
can't is refused with the whole team's command, such as
`lnk agent team move research <cloud>`, and nothing is paused.

### What Reaches a Team

A team kept on a machine is reached only there, so its members move
only with it. A team a cloud keeps reaches a member on a machine or box
through what that cloud's `agent-team list` names for it. Its folder
service keeps the folder, and its team relay keeps the conversation. A
copy of that team's file on a machine is never taken for a team kept
there. Another cloud that hosts agents runs neither for it, so a move
there is refused.

### The Team Goes Along

A move to a machine or box takes each such team with the agent, as the
machine running the move reads it from its cloud. The destination
writes the team's file to `~/Link/Safe/Teams/` and records its relay
and folder service in `~/.lnk/safe/agents/places/cloud-teams.toml`. Its runner
and files bridge there reach the team through them, and its grants come
from that file. A cloud that doesn't answer for a team recorded here
refuses the move, unless the agent goes to that cloud.

### Grants Derived Again

A member arriving where its team is kept keeps its place, and its grant
of the team's folder is derived again there from the team's current
file. An outside member gets what its roster entry grants. An own
member's grant is never taken from what the machine it came from says,
and the move says how to give it again with `lnk agent team add`.
Coming back, it gets again what this machine granted it when it left,
which the note of where it went keeps.

### Coming to a Machine Whole

`lnk agent team move <team>@<cloud> here` writes the team's file and
puts its shared folder in place before its members arrive, so each
member's grant is derived from that file. The cloud lists a team that's
moving out no more, so a member starting here reaches the team's folder
here, never through the cloud's folder service.

### Coming to a Machine

A team coming to a machine must contain only that account's agents. Remove
outside members first. The receiving machine chooses its own shared-folder
path under `~/Link/Teams/<team>/`; it never trusts a remote absolute path.

### Its Folder There

[`lnk storage push`](../../../../../storage/storage/docs/services/storage/guide.md#back-up) and [`lnk storage pull`](../../../../../storage/storage/docs/services/storage/guide.md#get-files-back) reach a hosted team's folder through
its hosting adapter, using its local `~/Link/Teams/<team>/shared` folder.
The cloud keeps a folder backup with `lnk agent team backup design --cloud my-cloud`
and restores it with `lnk agent team restore design --cloud my-cloud`.

### Who Reads Its Files

Anyone who can talk to a member can receive the shared files it reads;
an outside member's owner can read those conversations.

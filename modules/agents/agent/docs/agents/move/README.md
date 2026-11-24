# Move Your Agent

> Move Your Agent Between Machines

**Move your agent to a box in your own cloud, and back.** Your agent brings its
supported conversations, memory, files, settings, and connections along.

<!-- quickstart -->

## Move Your Agent to the Cloud

`lnk agent move` takes your agent to a box in your own cloud and brings
the agent back.

```sh
lnk cloud connect aws|gcp
lnk agent move aws|gcp
```

### Bring Your Agent Back

Return your agent to this computer when you want to run here again. The files, conversations, and connections come along.

```sh
lnk agent move here
```

### Choose a Machine or Image

Name one of your boxes, or a connected cloud account, and optionally a prepared image for the new box there. Replace the example names with your own.

```sh
lnk agent move gcp-1 --agent work
lnk agent move aws --agent work --image web-ready
```

A box's name is short for `--on gcp-1`, and a cloud account's name is short for `--cloud aws`. Local Link refuses a name that is both a box and a cloud account, and the refusal gives both strict forms to choose from.

### Check What a Move Needs

You need a cloud account connected with
[`lnk box`](../../../../../compute/box/docs/services/boxes/README.md#boxes), and the box costs what your
cloud charges. [Cloud](../../../../../../docs/services/README.md) covers where an agent
can run.

### Which Box

A cloud account's name moves your agent to your one box there. With no
box there, `move` makes one for you, as [`lnk box create`](../../../../../compute/box/docs/services/boxes/guide.md#create-a-box) does. With
several boxes, `move` lists them and you name the one you want. Run `move` on this
computer, wherever the agent is.

### Choose the Size of the Box

`move` makes a box at the cloud's default size. To choose another size,
such as a small machine for an agent that runs no programs, make the
box first with `lnk box create`. Then move your agent to your one
box there.

```sh
lnk box create gcp --name gcp-1 --machine.gcp.size e2-small
lnk agent move --cloud gcp --agent main
```

### See What Goes with Your Agent

The agent's memory, conversations, files, settings, and connections go
with the agent. So do its keys (the model's, the fallback's, and the
connections'), sealed to the machine they go to. This computer notes
each box [the keys went to](../../../../../security/secret/docs/config/secrets/README.md#copies-elsewhere), for [`lnk secret delete`](../../../../../security/secret/docs/config/secrets/README.md#an-agent-that-leaves).
The harness is installed fresh where it lands.

### Finish When the Agent Answers

Once the agent answers on the new machine, the machine it left deletes its copy and
keeps its files for 30 days. That machine's
[interaction store](../../../../../../packages/interactions/docs/more/libraries/interactions/interactions.md#a-subscriber-that-never-reads-again)
forgets the agent's reader, as when it's deleted. If the agent doesn't answer, the old machine
keeps its copy, stopped.

### Move from Your Image with the Buckets

A move with `--image` starts a new box from
[your image](../../../../../compute/box/docs/services/boxes/guide.md#save-a-box-as-an-image), even
where you already have a box. The agent's
[buckets](../../../../../storage/storage/docs/services/storage/guide.md#back-up-an-owners-folder) go
with the agent, so [`lnk storage push`](../../../../../storage/storage/docs/services/storage/guide.md#back-up) of its files works there.

### Chat with Your Agent on a Box

On a box, chat with your agent through [`lnk box run`](../../../../../compute/box/docs/services/boxes/ssh.md#run-lnk-on-a-box),
which runs `lnk agent chat send` there. The agent's `link` conversations came
along. From this computer, `lnk agent chat send` itself names that
command rather than send the message.

```sh
lnk box run gcp-1 -- agent chat send --agent main --message "hi"
```

## Agent Protocol

The [Agent Protocol](../../../../harnesses/portability/contract/docs/agents/harnesses/more/agent-protocol.md)
explains how Local Link controls the harness and carries supported state.
A machine move also transfers workspace files, shared collections,
and the native paths the harness's adapter declares. The destination installs
and configures the same harness, which can resume native history
without importing it from another harness. The [comparison](../../../../../../interop/agents/harnesses/docs/agents/harnesses/compare.md#move-between-machines)
distinguishes native-data moves from conversation transfer.

## Keep Your Agent Answering During a Move

A move copies most state while the agent runs, then pauses the agent for the switch.

### Copy While the Agent Runs

The agent's files and its memory's index go to the new machine while the agent runs
as before. To a box or this computer, what the harness knows goes too.
The move shows how many files and bytes it has copied.

### Switch to the New Machine

The agent pauses. Its connections stop reading, and the agent finishes the
turn it's in, for a minute at most. What changed since the copy goes next,
with the agent's settings, connections, scheduled jobs, and the day's count.
Then the agent starts on the new machine. The move reports the switch duration, which depends on the changed state,
destination startup, and network.

### Move an Agent Running in a Terminal

An agent running in a terminal (`lnk agent start --foreground`) moves the
same way: the pause stops it there, as `lnk agent stop` would, and it
starts on the new machine in the background. A move that fails after
the pause starts it again here in the background.

### Move Through Each Place's Storage

Conversations, their tool restrictions, and memory's notes move through each
place's storage contract. The receiving machine uses the places it supports,
and the agent's storage requests stay in its specification for the next move.
Local Link streams Home with bounded buffers, including a large individual file.

### Send a Message During the Switch

A message sent during the switch, on a connection Local Link carries for the agent,
follows that connection's retention and reconnect limits. The agent answers it
where the agent resumes, when the platform can deliver it. A
harness that reads a connection itself takes what comes there as it
always does.

### Chat During the Switch

`lnk agent chat send` and `lnk agent chat send --team` follow the agent. A
message sent while the agent is paused for the switch, or that the cloud it's
moving to or from refuses for now, waits for up to ten minutes. Local Link sends it again where the
agent is once the agent lands. Nothing was taken until then,
so the agent answers once, seconds later, wherever the agent ran.

### Recover from a Failed Move

A move can fail, be stopped (Ctrl-C, or the terminal
closed), or end with the agent not answering where it went. Each leaves the agent
answering where it was, leaves nothing of the agent where it was going, and says the move
failed. One move of an agent runs at a time.

### Recover from a Move Cut Off

A move can be cut off before it could say so (`lnk` killed, this
computer off or asleep too long). The next
`lnk agent move` or `lnk agent start` of the agent on this computer finishes
or takes back the move. It finishes the move when the agent answers where it went.
Otherwise it takes the move back and removes the copy there.

### Recover from a Box That Stops Answering

Local Link removes the agent's copy on a box that stopped answering once
the box answers again. The agent may have started on a box that
doesn't answer. Then it stays stopped here, so it never answers in both places.
On a box that's gone, the agent answers here.

### Try a Move Again

A copy tried again sends only what's missing. A move back to a machine the
agent left in the last 30 days sends only what changed since.

## Move Your Agent to a Cloud That Hosts Agents

A cloud account whose adapter hosts agents runs your agent itself, with
no box of yours. The cloud bills the agent it runs at the cloud's rates,
so the move asks first. `--yes` answers yes. With no terminal and no
`--yes`, the move makes nothing there and says to add `--yes`.

### Hosted Files and Chat

While your agent is hosted, push new local files to its Home, pull its files back to
this computer, and chat through the same account. Bring the agent back after
those operations. Once the agent returns, its Home is local again.

```sh
lnk agent move my-cloud
lnk storage push ~/Link/Agents/main/home/taxes
lnk storage pull ~/Link/Agents/main/home
lnk agent chat send --agent main
lnk agent move here --agent main
```

### Move in Two Phases

A hosted move goes in the same two phases, through the account's adapter.
The cloud may refuse a move, and the move then says why and when the
next one is allowed. `lnk agent list` shows the agents each such account
hosts.

### Reach the Hosted Home

While your agent is hosted, its Home is there too, and [`lnk storage push`](../../../../../storage/storage/docs/services/storage/guide.md#back-up)
and `pull` reach it through the same adapter, sending only what's new.

### Deliver Messages Held During the Pause

Messages that came while the agent was paused are handed to the agent
where it went, only from the senders its connection is paired with. Local Link keeps them with
the agent's settings, only yours to read, until the agent is given each one. An agent
moved with `--no-start` answers them when it starts. A message the agent is never
given there (a connection Local Link doesn't carry for the agent) is shown to you
whole, in the move or in `lnk agent deliver`.

### Deliver Only to the New Generation

Each held message names the move whose switch made the agent's new
generation, and the agent records the move it arrived by. `lnk agent
deliver` gives a message only to that generation. A message held for another
move, such as a copy the move left or a later move's, is shown and
never kept.

### Chat with a Hosted Agent

`lnk agent chat send --agent main` talks to the hosted agent through the same
adapter, on the same `link` threads, and shows the answer as it comes. An
answer nobody waited for (one you stopped waiting for) stays in
the conversation there and isn't handed to your next chat. An account
whose adapter doesn't carry chats says so.

### Find the Adapter Reference

[Adding a cloud](../../../../../../docs/more/developing/modules.md#adding-a-cloud) lists
what such an adapter answers, and [Accounts That Host
Agents](../../../contract/place/docs/agents/more/interface/contract/hosting.md) each command.

## Fix a Move That Goes Wrong

When a move doesn't go as it should, or an agent on a box has no
internet, see [Troubleshooting](troubleshooting.md).

## Understand Transfer Details

Transferred credentials, sealed state, and the destination's authority are in [Security](security.md). The copy and switch phases are in [Decisions](decisions.md). An interrupted move is recovered with [Troubleshooting](troubleshooting.md).

<!-- docs-tree:start -->

## Docs

- [Decisions](decisions.md): Why moves, cloud placement, backups and recovery preserve the agent this way
- [Security](security.md): What moves and backups protect, which credentials they carry and what recovery may restore
- [Troubleshooting](troubleshooting.md): What to do when a move doesn't go as it should, or your agent on a box has no internet

<!-- docs-tree:end -->

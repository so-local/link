# Chat Commands

Chat commands manage a chat's conversations and what its next turn is
sent: the `chat` namespace's actions beyond `/chat:create`, `/chat:stop`,
`/chat:status` and `/chat:goal`, which [Commands](README.md) explains.

## Control Retained Work

`/work list` shows registered conversations, children, schedule definitions and occurrences. `/work status` shows this conversation's desired and observed state. Add `--target=ID` after a control verb to address a listed child, schedule or attempt. `--subtree` includes the selected work and its descendants; `--descendants` excludes the root. Each command records its membership once and returns a batch ID for `/work batch ID`.

### Queue, Steer and Redirect

`/work queue TEXT` waits for a new turn. `/work steer TEXT` updates the next safe model context and invalidates tools generated under older instructions. `/work redirect TEXT` first stops the old path, then promotes that exact input. `/stop` stops current execution and preserves progress. Accepted, Delivered and Included receipts distinguish retained input from input the provider actually used.
With no retained work in the current conversation, a bare `/stop` says
there is nothing to stop and makes no model call. Explicit `/work` controls
still refuse a target that does not exist.

### Pause and Return

`/work pause` finishes the current model response without follow-on calls. `--immediate` interrupts the current step. `/work resume` releases the selected work's pause reasons. `/work start --target=ID` starts prepared work. `/work archive` retains progress after shutdown; `/work unarchive` makes it available while leaving it paused. A child's partial answer stays retained until execution can complete.

### Hold and Delete

`/work hold` blocks outgoing child creation, starts and child messages while the coordinator can keep thinking. The returned hold ID is required by `/work release ID`; release still requires proposal revalidation against the current plan. Only the owner can release a hold. `/work delete conversation`, `/work delete task` and `/work delete artifacts ID,ID` remove only their explicit scope after shutdown and effect reconciliation. `/work receipt ID` reads a member's evidence.

## Keep several conversations

A chat holds conversations one after another, all in its one log.
[`/chat:create`](../../../../../../../agent/docs/agents/chat/README.md#manage-history) begins one, and the commands below go back to one, copy one
or name one. The model is sent only the conversation the chat is on.

### Resume an earlier one

[`/chat:resume`](../../../../../../../agent/docs/agents/chat/README.md#manage-history) lists the chat's conversations, by number, with their
names or first messages. `/chat:resume 1`, or `/chat:resume work` by its
name, takes one up where it left off. Its alias is `/resume`.

### Fork this one

[`/chat:fork`](../../../../../../../agent/docs/agents/chat/README.md#fork-a-conversation) begins a conversation holding this one as it stands, and
the chat goes on in it. The one it came from stays unchanged, to
resume. `/chat:fork aside` names the new one.

### Name this one

[`/chat:rename work`](../../../../../../../agent/docs/agents/chat/README.md#manage-history) names the conversation. A name is never a number,
nor another conversation's, so `/chat:resume` takes either without
doubt. Alone, it says the conversation's name.

### Write one out

[`/chat:export`](../../../../../../../agent/docs/agents/chat/README.md#manage-history) writes the conversation, as `conversation.md` reads,
to `exports/` in the agent's folder, named by its chat and number:
`exports/telegram-42-1.md`. It answers the file's path.

### Each is a command event

Each one kept is an event of the role `command`, like `/chat:create`, and
the chat's conversations are read back from those events. Nothing is
copied or moved, and one refused, such as a resume of the conversation
the chat is on, is kept marked failed and begins nothing.

## Queue or steer your next message

A message you send while the agent answers does as the specification's `[turns]
busy` says, `steer` by default. `/chat:queue` has your next message wait
for a turn of its own instead, and [`/chat:steer`](../../../../../../../agent/docs/agents/chat/README.md#control-a-running-turn) has it join the answer
under way. Given a message, [`/chat:queue check the logs`](../../../../../../../agent/docs/agents/chat/README.md#control-a-running-turn), it applies to
that one. `/chat:steer` is offered only where `[turns] busy` steers.

## Compact it now

[`/chat:compact`](../../../../../../../agent/docs/agents/chat/README.md#manage-history) summarizes what's before your last message now, as
happens on its own once the window fills to `[window] compact_at`. It is
a turn of its own, so it waits for an answer being written. Your last
message and what follows it stay, and the next turn starts with the
summary. Where `compact_at` is `"never"`, it says nothing is summarized.

## Show its tool calls

[`/chat:verbose on`](../../../../../../../agent/docs/agents/chat/README.md#goals-and-visibility) shows each tool the agent calls above its answer on
this chat, and `/chat:verbose off` shows none, for the conversation,
in place of the specification's `[connections] activity`. Alone, `/chat:verbose`,
or `/verbose`, does the other of what the chat does now. The bridge
reads the choice from each call's `_meta.link.activity`.

## Ask on the side

[`/chat:ask <question>`](../../../../../../../agent/docs/agents/chat/README.md#control-a-running-turn) asks the model with the conversation as it
stands and the question after it, calling no tool. Neither the question
nor its answer is kept, so the conversation goes on as if it hadn't been
asked. It answers while a turn runs too. Its aliases are `/btw` and
`/side`.

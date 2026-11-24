# Decisions

Why Link Harness's [commands](README.md) work the way they do.

## Why is a command an event?

So a conversation's own settings are its events, kept the way every
event is: in the log, a compact conversation, a store driver or a fork,
with no change to what a store keeps. A turn reads no further back than
it does already, since the window's mark goes on from the `/new`. A
file of settings beside the conversation would need every driver to
keep it too.

## Why does `/new` keep what came before?

So nothing you said is lost by starting again. The chat's folder and
thread stay, the events before the `/new` stay searchable, and the
model is only no longer sent them. A new thread for the same chat would
need each connection to know which thread is current.

## Why does only the owner's message run one?

A command changes what the agent remembers and does next, which is
yours to decide. Anyone else in a shared chat, and every message the
agent sends itself, is already kept and quoted as theirs: a command
from them would let them reset your conversation.

## Why is the reply not an answer?

The model wrote none of it, and a conversation that starts with
anything of the agent's would not begin with you speaking, as every
window does. So the harness's reply goes to the chat and is kept
nowhere, and a reply nobody was there to take is not sent again.

## Why is a model the conversation's?

A conversation you steer to a stronger model, or a faster one, is
usually one task, and the agent's other chats shouldn't change with
it. So `/model:set` is an event of the conversation
it's sent in, read back at each of its turns. A resumed conversation
runs on what it chose, and a new one on the agent's.

### Why does `--every-chat` last only until the agent starts again?

The model an agent starts with is the one you named to Link, and Link
gives it to the harness at each start. A switch inside the harness
that outlived the start would leave Link and the harness naming two
models. The agent's specification is also out of the harness's reach, so a
harness can't rewrite what Link starts it with.

### Why does `/model:set` switch only within the provider?

The agent's key and the proxy that adds it are its provider's, set when
the agent starts. A model of the same provider needs nothing more, so
the switch is one setting the next turn reads; another provider's would
need its key granted and its proxy started, which only a start does.

## Why do a fork and a resume copy nothing?

A chat keeps one log, and each conversation is read back from the
command events that began it: [`/chat:create`](../../../../../../agent/docs/agents/chat/README.md#manage-history), [`/chat:fork`](../../../../../../agent/docs/agents/chat/README.md#fork-a-conversation) and
[`/chat:resume`](../../../../../../agent/docs/agents/chat/README.md#manage-history). Copying events would renumber them, which breaks the
window's marks and the summaries that cover them. Reading them back
keeps every event as it was written, and a fork's shared start is the
same events, so the provider's cache holds for it too.

## Why can't a repository's skill replace a command?

A repository is often someone else's, cloned to work in. If its skills
could take a command's name, cloning one could change what `/stop` or
`/chat:create` does. Your own skills are yours to trust, so one of yours
replaces a command of its name, and `/skill:<name>` reaches every
skill whatever its name.

## Why does each tool call carry its chat's verbosity?

The bridge shows or hides tool calls as the specification's `[connections]
activity` says, before a turn starts. A choice made in the
conversation is the harness's to know, so the harness marks each call
it streams, and the bridge follows the mark. A harness that marks
nothing keeps the specification's behavior.

## Why keep namespace commands and aliases separate?

A namespace command names a capability and owns its implementation.
Aliases preserve familiar spellings from other harnesses by resolving
to that command. Keeping mappings in the action declaration gives every
spelling the same handler and permission checks. Separate references
let a newcomer learn the capabilities while an experienced user finds
the spelling they already know.

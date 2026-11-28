# Talk to Your Agent

> Chat Here, Keep the Conversation

Talk to your running agent in a terminal, send a message from a script, or text your agent from your phone. If your agent is stopped, start it first with `lnk agent start`.

```sh
lnk agent chat
```

## Talk to It Here

Talk to your agent in this terminal, or from a script, with no account
or bot. `lnk agent chat send` reads its message on its input, so a
script pipes one in and reads the answer as JSON with `--json`.

```sh
lnk agent chat -m "What can you do?"
lnk agent chat start --chat work
lnk agent chat show link:main
lnk agent chat send --json
```

### Chat on Local Link's Own Connection

`lnk agent chat` talks to your agent on Local Link's own connection, `link`,
with no account or bot. Only you, as the agent's user on its machine, reach
your agent there. In a terminal, each answer shows as it's written. Ctrl-C
stops waiting for the answer, and Ctrl-D or `/quit` leaves.

### Threads and Scripts

`lnk agent chat` is short for `lnk agent chat start`, which takes
`--chat`. Each opens a fresh conversation, prints the conversation's
`link:<name>` ID, and keeps follow-ups together until you leave. The
conversation's folder is `agent/chats/link-<name>/`. Use `--chat <name>` to
select a particular conversation, including one from an earlier session. The
agent refuses a name it has no conversation by, and names it. The refusal
comes where the harness lists its conversations (`lnk agent chat list`), as
Link Harness does. `lnk agent chat show <name> --status` is refused the same
way.

#### Open a Chat with a Message

`lnk agent chat -m "What can you do?"` opens the same live chat with
that message already sent. The message shows after the first prompt as though
you typed it, and the chat keeps going after the answer. The command suits
a line you copy from a guide. The message is an argument, so other processes
here can read it, as they can read `lnk agent chat send --message`'s.

#### Send One Message from a Script

`lnk agent chat send` is for a program, such as a script or another
agent, that sends one message and reads one answer with no conversation
after it. The command takes `--message` or piped stdin and prints the answer.
It exits nonzero for empty input, a stopped or failing agent, or no answer.
Empty input shows how to give a message. The default thread is `link:main`.
`--chat <name>` selects another conversation, and a new name starts one.

#### Chat in Discord and Slack Threads

Each Discord or Slack thread has its own conversation. Replies in a
thread share context, and another thread starts separately. Saved memory
notes belong to the agent and are available across these conversations.

### Read an Answer Nobody Waited For

Some answers arrive when nobody waits. A reminder the agent set in a thread, a
background command's end, and an answer you stopped waiting for all count.
The answer comes at your next chat on that thread, under "While you were
away:" before the answer to your message. If a chat is open, it comes as it
is written. A script reads the two apart with `--json`, whose `held` holds the
first and `answer` the second, in the `answered` event that ends the
answer's stream.

### Chat with an Agent Elsewhere

An agent on a box answers from there. So does an agent on [a cloud account
that hosts agents](../move/README.md#move-your-agent-to-a-cloud-that-hosts-agents) through the account's adapter.

### Check Which Harnesses Answer

Terminal chat needs a harness that answers an endpoint, as Link Harness,
DeepSeek Harness, OpenClaw, and Hermes Agent do, the last two through Local
Link's bridge. Local Link refuses a harness that speaks only its own
connections, and says so. Talk to that harness on a connection instead.

## Text Your Agent From Your Phone

Connect a messaging app, and you can text your agent from anywhere.
[Connect Messaging Apps](../connections/connect/README.md) shows how to create
a bot, supply its credentials, and pair the bot with your agent.

```sh
lnk agent connect telegram|slack|discord|signal
```

## Talk Somewhere Else

Ask your agent to message you elsewhere, in your own words. Say "Send me a
message on Slack" in this terminal, or "Send me a message on the CLI"
on Slack. The agent sends to your own direct chat on a connected service, or
to `cli`, your terminal on this computer. Connect the service first, with
`lnk agent connect slack`.

### Send a Message to the Terminal

A message to `cli` shows in an open `lnk agent chat`, or waits for the
next one under "While you were away:". The agent knows the message is waiting
and can tell you so. Local Link refuses a message for a service that isn't
connected, and tells the agent which places it can reach.

### Send a Message from a Script

A script sends the same way, as the agent. `--to owner` sends to you,
and `--to teammate:<name>` sends to a teammate. `--connection` names where the
message goes, and the message comes from your terminal's `link:main`
conversation.

```sh
lnk agent chat send --message "The build finished" --to owner --connection slack
```

### Keep Conversations Separate

Each place keeps its own conversation. When you answer on Slack, the
Slack conversation doesn't yet know what you said in the terminal, so
say what you're answering. Every conversation shares the saved memory notes.

## Read Its Conversations

Read a chat, or move the old ones to the way your agent's specification keeps them
now. Only Link Harness prints and moves its conversations. On another
harness, these commands are refused and name `lnk agent harness set link`.

```sh
lnk agent chat show telegram:123456789
lnk agent chat show link:main
lnk agent chat export --format sqlite
```

Link Harness keeps each chat in its files, as a log you can `cat`, as a
smaller file, or in a database. The specification's `[storage] conversations` [says
which](../../../../harnesses/link/docs/agents/harnesses/link/conversations.md#keep-them-smaller-or-in-a-database), for a new chat.
`lnk agent chat show` prints a chat however the chat is kept. `lnk agent chat
export --format <way>`, with the agent stopped, rewrites the old ones in
that way. Set the specification's `[storage] conversations` to the same way, so
new ones start that way too.

## Control a Conversation

Owner controls use `--chat <name>` to select a conversation, and Local Link forwards them to the harness. The harness's advertised commands determine which controls it supports. [Link Harness Commands](../../../../harnesses/link/docs/agents/harnesses/link/commands/README.md) explains what Link Harness supports.

### Manage History

`lnk agent chat start` opens a fresh conversation, `lnk agent chat start --chat <name>` returns to an earlier one, and `lnk agent chat set name <name>` names the current one. `lnk agent chat export` writes a readable copy, while `lnk agent chat compact` requests a summary of older turns. These controls keep the harness's distinction between saved history and the context sent to the model.

### Control a Running Turn

`lnk agent chat send --aside --message <question>` asks a side question without adding it to history. `lnk agent chat send --queue` gives a message its own later turn. `lnk agent chat send --steer` joins a running turn where the harness supports it. In a live chat, Ctrl-C stops the current answer. From another terminal or a script, `lnk agent chat stop --chat <name>` stops that chat's answer. A control the harness does not offer cannot work through this route.

### Goals and Visibility

`lnk agent chat set goal "the tests pass"` gives the conversation a goal to work toward until the goal holds. `lnk agent chat get goal` says the goal, and `lnk agent chat set goal --clear` removes it. `lnk agent chat show <chat> --status` inspects the conversation's current model and context, and `lnk agent chat set verbose on` or `off` changes whether the conversation shows tool activity.

### Fork a Conversation

`lnk agent chat fork` creates a fresh thread from the history before the owner message that `--before` selects. `lnk agent chat list` identifies saved threads. A fork keeps some restrictions of its source, and sequence numbers order the history. [Link Harness Conversations](../../../../harnesses/link/docs/agents/harnesses/link/conversations.md) explains both.

## Understand Conversation Details

Terminal routing, busy turns, and retained answers each follow a [decision](decisions.md). Messages from other agents [keep their source and authority](security.md).

<!-- docs-tree:start -->

## Docs

- [Decisions](decisions.md): Why conversation routing, turn handling and terminal chat work this way
- [Security](security.md): How messages from other agents retain their source and authority when delivered to your agent

<!-- docs-tree:end -->

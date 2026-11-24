# Harness Prompts

`link-harness-wire`'s `runtime` is how an agent's host calls its harness
when both run in one process. The host hands the harness a
message and hears its answer through direct Rust calls, with no
protocol between them. The Agent Client Protocol (ACP) and Agent State Protocol (ASP) translate it at their own boundaries, for clients
and harnesses outside Link.

## Prompts

A `Prompt` is a message for one `Conversation`: an agent, named by its
owner and ID, and one of its threads, such as `link:main`. It carries
its text and media, its `Sender`, how many agent-sent messages led to
it, and optional choices for the conversation from now on: the tools it
may use and the model it answers with.

### Senders

A `Sender` is the agent's owner, or an `Origin` with the transport event
that carried it. An `Origin` is a person in a shared chat, a schedule
with the occurrence it fired for, a task, another agent, a background
run or a webhook. The host authenticates the sender before it builds
the prompt, and a message's text never changes it. A schedule's
occurrence exists only on a schedule's origin, and a transport event
only on a sender other than the owner, so neither can be attached to the
wrong sender.

### Busy conversations

`Busy` says what a message sent while a turn runs does. It can wait for
a turn of its own (`queue`), join the running turn at its next round
(`steer`), or end the running turn and start its own (`interrupt`). A
prompt's choice overrides the agent's specification for that message.

## Answers

While it answers, a harness tells the prompt's `Listener` what happened,
as a `Streamed` value that borrows its text for the call. It reports
when the message is sure to get a turn of its own, each piece of text,
each tool call and tool answer, and the final answer. The final answer
is reported as soon as it's recorded, so the host can deliver it while
the turn still finishes work such as summarizing.

### Outcomes

The prompt then returns an `Outcome`: an `Answer`, or `Steered` when the
message joined a turn already running. An answer says whether it was
cut short, and carries a `Receipt` when the prompt asked for one.

### Delivery recovery

A prompt that asks for a receipt has its answer kept until the host
confirms it delivered, through `Runtime::delivered`. After a lost
connection or a restart, `Runtime::recover` returns each answer still
unconfirmed, so the host can deliver it again.

## Runtime

`Runtime` is the trait a harness implements. Every method takes `&self`,
because one harness serves many conversations at once. `prompt` is
generic over its listener, so a call compiles to a direct call with no
dynamic dispatch. `cancel` asks a conversation's running turn to end.
A harness implementing it is built into its host's program, so the
interface has no version of its own: what crosses between programs is
the harness protocol's, whose version an adapter's `contract` states.

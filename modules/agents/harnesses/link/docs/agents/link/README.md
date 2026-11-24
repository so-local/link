# Link Harness

> Link Harness Is Small and Mighty

Link Harness is Local Link's own agent runner. You edit its instructions
and skills as files, inspect the prompt it sends, and keep your
conversations in an event log you control.
Choose Link Harness when you want a native harness
whose behavior follows your agent's specification and whose saved state you can
inspect without a separate harness runtime.
The [controlled benchmarks](benchmarks.md) compare startup and idle
memory, with their measurement conditions and scope stated explicitly.

<!-- quickstart -->

## Start an Agent on Link Harness

Create or start your agent and choose its model. The [Link Harness guide](guide.md) explains its instructions, tools and saved conversations.

```sh
lnk agent start
```

## Switch to Link Harness

Local Link keeps your model, files, and connections when you switch. The complete workflow is in [Switch to it, and back](guide.md#switch-to-it-and-back).

```sh
lnk agent harness set link
```

## Send a Message to Your Agent

Send one message to the running agent and print its answer. The [terminal chat guide](../../../../../agent/docs/agents/chat/README.md#talk-to-it-here) also covers interactive conversations.

```sh
lnk agent chat send --message "Hello"
```

## Show the Agent's Instructions

The agent owns its editable instruction and skill files. Link Harness reads them to build each turn's prompt. The complete workflow is in [Make it yours](guide.md#make-it-yours).

```sh
lnk agent show --instructions --json
```

## Conversations and Clients

Storage, context limits, and recovery are in [Conversations](conversations.md). Session and prompt behavior are in [Agent Client Protocol](more/acp/README.md), owner commands are in [Commands](commands/README.md), and images, documents, and voice notes are in [Media](media/README.md).

## Inspect the Harness

The [guide](guide.md) explains instructions and tools, and [Decisions](more/decisions.md) explains their design. To find the implementation and its tests, read [Developing](more/developing.md). When answers go missing, [Troubleshooting](troubleshooting.md) helps you diagnose them. [Harness Benchmarks](benchmarks.md) records the measured scope and conditions.

<!-- docs-tree:start -->

## Docs

### Pages

- [Link Harness Guide](guide.md): Configure Link Harness’s instructions, skills, tools, and model fallback, then inspect the prompt and conversation it keeps
- [Agent Client Protocol](more/acp/README.md): Link Harness speaks ACP, so an ACP client can open a session on one of its conversations, prompt it, and receive the answer as it's written, with the tool calls beside it
- [Commands](commands/README.md): A command is an owner's slash message that Link Harness handles
- [Images, Documents and Voice Notes](media/README.md): Link Harness reads the images, documents and voice notes a message holds, from the Agent Client Protocol (ACP), or from any of Local Link's connections, and keeps each as a file in the conversation's folder
- [Conversations](conversations.md): Link Harness retains conversations in your agent’s files or through its storage driver
- [Harness Benchmarks](benchmarks.md): A controlled snapshot compares Link Harness's startup and idle memory with Hermes and OpenClaw

### More

- [Developing](more/developing.md): `lnk-harness` is Link Harness, the harness a Link agent runs by default: a small router in Rust that answers you, keeps the thread, and calls the tools Link runs for it
- [Decisions](more/decisions.md): Why Link Harness works the way it does
- [Troubleshooting](troubleshooting.md): What to do when Link Harness doesn't answer, or answers wrong

<!-- docs-tree:end -->

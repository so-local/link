# OpenClaw

> OpenClaw Has Personality and a Firm Grip

[OpenClaw](https://openclaw.ai) is the personal AI assistant, with its own
personality files and commands you send from your phone. Local Link
installs it, keeps it in a sandbox and up to date, and runs it as your
agent's harness.

<!-- quickstart -->

## Start an Agent on OpenClaw

Start the agent that will run OpenClaw. The [guide](guide.md) explains the supported model providers and the credentials they need.

```sh
lnk agent start
```

## Switch to OpenClaw

Select `openclaw` as the harness. Local Link installs its pinned runtime on first use. Read the [switching and state details](guide.md) before you transfer conversations from another harness.

```sh
lnk agent harness set openclaw
```

## Connect OpenClaw to a Chat App

Connect OpenClaw to a chat app through Local Link's bridge. The [guide](guide.md) explains how messages reach the harness and what state each chat keeps.

```sh
lnk agent connect telegram|slack|discord|signal
```

## Create an Agent on OpenClaw

Create a separate agent named `work` that uses OpenClaw from the start. [Run Another Agent](guide.md#run-another-agent-on-it) covers the new agent's own model, files, and connections.

```sh
lnk agent create work --harness openclaw
```

## Move and Migrate Agents

Start fresh on OpenClaw, or adopt existing OpenClaw agents through migration. The [guide](guide.md#bring-an-existing-openclaw-agent) explains native resources and bounded conversation continuation, and [Harness Adapters](../../../../../docs/agents/harnesses/adapters/README.md) links the other choices.

## Understand the Adapter

The openclaw adapter's runtime and configuration choices are in [Decisions](decisions.md). [Security](security.md) lists the programs it installs, the credentials it uses, and the limits on what it runs. When OpenClaw won't find its model or start, [Troubleshooting](troubleshooting.md) has the fix.

## Build the Adapter

[Runtime](more/runtime.md) explains how the openclaw adapter installs the harness, writes its settings, and draws its process boundary. [Developing](more/developing.md) gives the local build and test commands.

<!-- docs-tree:start -->

## Docs

### Pages

- [OpenClaw Guide](guide.md): Run OpenClaw with a hosted model or Ollama, then use its personality and memory from your existing chats
- [Runtime](more/runtime.md): The openclaw adapter installs, configures, and runs the native harness for Link

### More

- [Developing](more/developing.md): Build and inspect the openclaw adapter from its workspace package
- [Decisions](decisions.md): Why `lnk-openclaw` runs OpenClaw the way it does
- [Security](security.md): What `lnk-openclaw` installs and runs, beyond what Link's sandbox guarantees every harness
- [Troubleshooting](troubleshooting.md): What to do when OpenClaw doesn't find its model, loses a turn or keeps starting outside Link

<!-- docs-tree:end -->

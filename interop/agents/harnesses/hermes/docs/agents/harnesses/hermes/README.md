# Hermes Agent

> Hermes, the Messenger, Speaks Any Model

[Hermes Agent](https://hermes-agent.nousresearch.com) is Nous Research's
agent. It takes a model from any local runtime as well as a hosted one.
Local Link installs it, keeps it in a sandbox and up to date, and runs it
as your agent's harness.

<!-- quickstart -->

## Start an Agent on Hermes Agent

Start the agent that will run Hermes Agent. The [guide](guide.md) explains the supported model providers and the credentials they need.

```sh
lnk agent start
```

## Switch to Hermes Agent

Select `hermes` as the harness. Local Link installs its pinned runtime on first use. Read the [switching and state details](guide.md) before you transfer conversations from another harness.

```sh
lnk agent harness set hermes
```

## Connect Hermes Agent to a Chat App

Connect Hermes Agent to a chat app through Local Link's bridge. The [guide](guide.md) explains how messages reach the harness and what state each chat keeps.

```sh
lnk agent connect telegram|slack|discord|signal
```

## Check Your Agent

Check that your agent is running, and see its current harness, model, and connections. The [switching guide](guide.md#move-from-openclaw-to-hermes) explains what carries across.

```sh
lnk agent show
```

## Move and Migrate Agents

Hermes Agent supports fresh creation, installation migration, native session import, and machine moves. The [guide](guide.md#bring-an-existing-hermes-agent) explains adoption and transfers, and [Harness Adapters](../../../../../docs/agents/harnesses/adapters/README.md) compares the shared management model.

## Understand the Adapter

The hermes adapter's runtime and configuration choices are in [Decisions](decisions.md). [Security](security.md) lists the programs it installs, the credentials it uses, and the limits on what it runs. When Hermes Agent won't start or remember, [Troubleshooting](troubleshooting.md) has the fix.

## Build the Adapter

[Runtime](more/runtime.md) explains how the hermes adapter installs the harness, writes its settings, and draws its process boundary. [Developing](more/developing.md) gives the local build and test commands.

<!-- docs-tree:start -->

## Docs

### Pages

- [Hermes Agent Guide](guide.md): Run Hermes Agent with a hosted or local model, and carry your conversations and memory between harnesses
- [Runtime](more/runtime.md): The hermes adapter installs, configures, and runs the native harness for Link

### More

- [Developing](more/developing.md): Build and inspect the hermes adapter from its workspace package
- [Decisions](decisions.md): Why `lnk-hermes` runs Hermes Agent the way it does
- [Security](security.md): What `lnk-hermes` installs and runs, beyond what Link's sandbox guarantees every harness
- [Troubleshooting](troubleshooting.md): What to do when Hermes Agent doesn't install, start or remember

<!-- docs-tree:end -->

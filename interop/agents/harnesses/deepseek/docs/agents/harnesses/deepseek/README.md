# DeepSeek Harness

> DeepSeek, a Whale That Fits in a Sandbox

[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) is
DeepSeek's open-source agent. Local Link installs it (about 800 MB),
keeps it in a sandbox and up to date, and turns off everything it would
send DeepSeek.

<!-- quickstart -->

## Start an Agent on DeepSeek Harness

Start the agent that will run DeepSeek Harness. The [guide](guide.md) explains the supported model providers and the credentials they need.

```sh
lnk agent start --model.default deepseek/deepseek-v4-pro
```

## Switch to DeepSeek Harness

Select `deepseek` as the harness. Local Link installs its pinned runtime on first use. Read the [switching and state details](guide.md) before you transfer conversations from another harness.

```sh
lnk agent harness set deepseek
```

## Connect DeepSeek Harness to a Chat App

Connect DeepSeek Harness to a chat app through Local Link's bridge. The [guide](guide.md) explains how messages reach the harness and what state each chat keeps.

```sh
lnk agent connect telegram|slack|discord|signal
```

## Move and Migrate Agents

DeepSeek Harness supports fresh creation, existing-installation migration, and native-data moves. The [guide](guide.md#keep-context-through-a-switch) explains session export and archive-backed continuation, and [Harness Adapters](../../../../../docs/agents/harnesses/adapters/README.md) separates those capabilities from native features.

## Understand the Adapter

The deepseek adapter's runtime and configuration choices are in [Decisions](decisions.md). [Security](security.md) lists the programs it installs, the credentials it uses, and the limits on what it runs. When DeepSeek Harness won't start or recover, [Troubleshooting](troubleshooting.md) has the fix.

## Build the Adapter

[Runtime](more/runtime.md) explains how the deepseek adapter installs the harness, writes its settings, and draws its process boundary. [Developing](more/developing.md) gives the local build and test commands.

<!-- docs-tree:start -->

## Docs

### Pages

- [DeepSeek Harness Guide](guide.md): Run DeepSeek Harness with your chosen provider or local runtime, then connect it to your chats
- [Runtime](more/runtime.md): The deepseek adapter installs, configures, and runs the native harness for Link

### More

- [Developing](more/developing.md): Build and inspect the deepseek adapter from its workspace package
- [Decisions](decisions.md): Why `lnk-deepseek` runs DeepSeek Harness the way it does
- [Security](security.md): What `lnk-deepseek` installs and runs, beyond what Link's sandbox guarantees every harness
- [Troubleshooting](troubleshooting.md): What to do when DeepSeek Harness doesn't install, start or answer

<!-- docs-tree:end -->

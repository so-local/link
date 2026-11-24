# Signal

> Signal Brings Your Agent to Your Phone

Text your agent on Signal. Your agent has a number of its own, linked
from a phone. Your agent answers you and no one else.

<!-- quickstart -->

## Start Your Agent

Start your agent before pairing Signal. Your agent needs a number of its own, and the [connection guide](guide.md) explains how to link one.

```sh
lnk agent start
```

## Connect Your Agent to Signal

Give your agent a number of its own, such as a second or prepaid SIM. The complete workflow is in [Connect it](guide.md#connect-it).

```sh
lnk agent connect signal
```

## Understand and Fix the Signal Connection

Signal connects through `signal-cli`. [Decisions](decisions.md) explains that choice, and [Native Program](more/native-program.md) describes the sandboxed install and linking boundary for `signal-cli`. When linking fails or a proxy gets in the way, [Troubleshooting](troubleshooting.md) helps.

<!-- docs-tree:start -->

## Docs

### Pages

- [Signal Guide](guide.md): Link a dedicated Signal number to your agent, then send messages from your own number
- [Native Program](more/native-program.md): The Signal adapter installs and links `signal-cli` for the harness and its bridge

### More

- [Decisions](decisions.md): How the Signal module connects an agent, through `signal-cli`
- [Troubleshooting](troubleshooting.md): What to do when your agent's Signal number doesn't link or doesn't answer

<!-- docs-tree:end -->

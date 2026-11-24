# Models Guide

Install or discover a local model, choose a hosted provider, and inspect the model your agent uses. A runtime’s configuration determines where inference runs; provider calls use your credential and the provider’s billing.

- **On your computer, without a Link inference fee.** `lnk` installs a model for
  you with Ollama, or finds the one you already run. [Local inference](local.md) uses
  your hardware.
- **A provider's model, with your own credential.** Anthropic, OpenAI or
  OpenRouter. [Keys on the wire](providers.md) keep the credential outside compatible
  harnesses.

- **For model builders.** A model you're building gets [its own address](runtime/README.md), for
  your agent and for a demo.

```sh
lnk up
lnk model install
lnk model list
```

## Pick where it runs

`lnk model usage`, `context`, and the request actions need the model
namespace of a running agent, as `lnk model show` does when it names no
model; there, `show` also says whether the model endpoint is ready, and
which models it has loaded. Outside that context, Link gives a usage message
with the next commands, without offering to report a bug. Use
`lnk agent model status` for your agent or `lnk model list` for runtimes
on this computer.

### Select your agent's model

Switch your agent between them in one command:
[Switch Models](../../../../agent/docs/agents/models/agent/README.md).

| | Costs | Who sees your words |
|---|---|---|
| [On your computer](local.md) | Your hardware and runtime costs | Determined by the runtime’s configuration |
| [A model provider](providers.md) | What the provider bills you | The provider |
| [For model builders](runtime/README.md) | Nothing | Your server, and whoever you give the password |

## What it needs

You need a Mac or a Linux computer. [`lnk up`](../../../../../../docs/more/modules/install.md#pick-your-modules) adds the module, and
`lnk model install` picks
a model on an Apple Silicon Mac, and elsewhere you name one. A model on
your own hardware is free.

## Where things live

Link keeps two small files, and your models stay in their runtime.

- `~/Link/Safe/Models/models.toml` holds the addresses you add, and nothing
  else.
- `~/.lnk/safe/models/facts.toml` holds owner-written model facts. Inspect the merged
  figures with `lnk model show <model>`.
- Models stay in their runtime, in Ollama's own folder for the ones
  `install` pulls.
- Ollama, when `install` adds it, is the official one. It's in
  `/Applications/Ollama.app`, or `~/Applications`, on macOS, and a
  system service on Linux.
- [`lnk mod remove model`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) removes the module, not Ollama or its
  models.

## More about it

Every flag is in `lnk model <command> --help`. [Security](security.md)
has what's protected and the gaps, and [Decisions](more/decisions.md) why it
works this way. When it finds no model: [Troubleshooting](troubleshooting.md).

## Program API

The [model actions API](more/api.md) gives agents and sandboxed programs the
same request and answer shapes.

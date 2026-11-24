# Models

> Models for Local and Hosted Inference

**Run a model on your computer for free, or use Anthropic, OpenAI or
OpenRouter with your own credential.** Use an existing local runtime or add one you are building.

<!-- quickstart -->

## Install

On an Apple Silicon Mac with at least 8 GB of memory, install Ollama, a
program that runs models on your computer, and the recommended Qwen3 size,
confirming the download first. The bare command recommends only on these
Macs. Supported runtimes and hosted providers are in the
[model guide](guide.md).

```sh
lnk model install
```

### Name a Model

On Linux or an Intel Mac, name a model explicitly. You can also choose
an explicit tag on Apple Silicon. For example, install the
`qwen3` tag if this machine has enough memory to run it. Read about
[hardware and runtime choices](local.md) before you pick.

```sh
lnk model install qwen3
```

## List

List models available from this computer’s local runtimes. Read how Local
Link [discovers those runtimes](guide.md).

```sh
lnk model list
```

## Choose Where a Model Runs

Install and serve a model [on your computer](local.md), use a hosted model
with [your own credential](providers.md), or connect
[a runtime you are building](runtime/README.md) at an address you choose.
The [model guide](guide.md) brings those routes together.

## Choose an Agent’s Model

Select the provider, runtime, and fallback for an existing agent with
[Switch Models](../../../../agent/docs/agents/models/agent/README.md). Model
settings stay separate from installing or serving a runtime on this computer.

## Inspect Usage and Integrate Callers

See retained usage by prompt part in [Context Reporting](context.md). Find
typed caller contracts, runtime metadata, configuration, and design
decisions in [Model Internals](more/README.md).

## Check Security and Fix Failures

Read about access limits and trust in [Security](security.md). Fix a failed
discovery or model request with [Troubleshooting](troubleshooting.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Models Guide](guide.md): Install or discover a local model, choose a hosted provider, and inspect the model your agent uses
- [On Your Computer](local.md): Run a model through a runtime on your computer, without a Link inference fee
- [Model Runtimes](runtime/README.md): `lnk model runtime` manages the local inference servers Local Link discovers
- [Context Reporting](context.md): See what your agent's context is spent on, part by part, for every harness: its instructions, each tool, and the conversation down to each tool's calls and results
- [Model Provider](providers.md): Use Anthropic, OpenAI, OpenRouter, or a provider an installed module adds, with your own credential

### More

- [Model Internals](more/README.md): Inspect model runtime integration, stored configuration, and the shared caller boundary
- [Security](security.md): The model module limits what callers can reach through a served runtime and what discovery exposes about this machine
- [Troubleshooting](troubleshooting.md): What to do when `lnk model` finds no model, or finds more than one

<!-- docs-tree:end -->

# Switch Models

> Models Switch While Your Agent Stays

**Change what your agent thinks with, in one command.** Use a model on
your computer or a provider's, and your agent keeps its files and
connections.

## Switch in One Command

Set the model you want. A running agent restarts with the model once you say
yes. A stopped agent saves the selection for its next start, which asks
for a hosted provider's key, or reads it from its variable, such as
`ANTHROPIC_API_KEY`.

```sh
lnk agent model set local/qwen3:8b
lnk agent model set anthropic/claude-sonnet-5-5
```

The new model replaces the saved one. The agent restarts when you
answer yes (`lnk agent restart` later, where nothing asks). The start ends with the table `lnk agent show` shows, the model
in bold. Later starts reuse the model, in whichever harness runs the agent,
until you name another. [Models](../../../../../model/docs/agents/models/README.md) helps
you choose.

## Say Who Serves It

A model's name starts with who serves it, so you always know where a
turn is sent. `set` and `start` print the full name, and Local Link sends a
model nowhere its name doesn't say.

```sh
lnk agent model set openrouter/deepseek/deepseek-v4
```

### Read the Prefixes

`local/` is a model running on this computer. `anthropic/` and `openai/`
are those providers' own APIs. `openrouter/` is OpenRouter, followed
by OpenRouter's own name for the model. Anthropic and OpenAI name their
models without a slash, so `openai/openai/gpt-6-luna` is refused with
`openrouter/openai/gpt-6-luna` suggested. Write a prefix in
lowercase. `Anthropic/claude-haiku-5-5` is refused with
`anthropic/claude-haiku-5-5` suggested, never sent to OpenRouter as
another vendor's model.

### Name Another Vendor's Model

A name whose first part is another vendor, such as `deepseek/deepseek-v4`
or `meta-llama/llama-4`, goes through OpenRouter, since Local Link reaches no
other provider directly. Local Link prints the name as
`openrouter/deepseek/deepseek-v4`.

### Use a Bare Name

A bare name goes to a model of that name running on this computer first.
Otherwise the name goes to whoever owns the model, never to OpenRouter by
default. `claude-...` goes to Anthropic, and `gpt-...`, `chatgpt-...`, or an
`o3`-like name goes to OpenAI. A bare name with no owner Local Link knows goes to
OpenRouter. So `gpt-6.1-sol` goes to OpenAI unless a model of exactly that
name runs here, and only `openrouter/openai/gpt-6.1-sol` goes to OpenRouter.
`set` and `start` say which.

### Keep Keys with Their Provider

A model takes only its own provider's key. A key for another provider is
refused, even when it's the only one set, so `openai/gpt-6.1-sol` with
only `OPENROUTER_API_KEY` set asks for an OpenAI key rather than going to
OpenRouter. For OpenRouter's copy, name `openrouter/openai/gpt-6.1-sol`.

## See Which Model Your Agent Uses

Use `--chat <name>` with `lnk agent model set` to change just that
conversation through the running harness. Without `--chat`, the selection
is the persistent default. `lnk agent start --model.default <id>` also selects a
model when starting the agent.

### Spell Model IDs Exactly

Model IDs are exact. A known misspelling such as `claude-haiku-5.5`
is refused with `claude-haiku-5-5` suggested. Local Link checks hosted IDs absent from
its catalogue against the provider's model list before
saving settings. If the check fails, the saved selection stays as
it was. Known IDs do not prove that your account has access to them.

### Inspect the Selection

Ask your agent's status for its model, and ask Local Link what a model
costs and which models run on this computer.

```sh
lnk agent show
lnk model show claude-haiku-5-5
lnk model list
```

`status` names the model and its provider, and `model show` says what the model
costs a million tokens, in and out. [A model Local Link doesn't know](../../../../../model/docs/agents/models/providers.md#what-link-knows-of-a-model) runs with
a window of 32,768 tokens until you describe it.

## Use a Model on Your Computer

Use a model running on your computer, which is free and private. On an
Apple Silicon Mac with at least 8 GB of memory, bare installation picks
a Qwen3 size for that machine. `local/qwen3` selects the installed sized
tag by its base name.

```sh
lnk model install
lnk agent start --model.default local/qwen3
```

### Choose a Model Explicitly

On Linux or an Intel Mac, bare installation offers no recommendation.
Name the tag to install, then select that installed model for the agent.
You can also name a tag on Apple Silicon. Choose a
model this computer can run; [local models](../../../../../model/docs/agents/models/local.md)
explains its hardware needs.

```sh
lnk model install qwen3
lnk agent start --model.default local/qwen3
```

### Choose a Runtime

`--model local/<name>` takes a name [`lnk model list`](../../../../../model/docs/agents/models/README.md#list) shows. Link Harness takes any
runtime `list` finds: Ollama, LM Studio, llama.cpp, vLLM, or Jan. A
harness from outside Local Link may take fewer, which
[Compare Harnesses](../../../../../../../interop/agents/harnesses/docs/agents/harnesses/compare.md) and its own page say.

### Watch for a Small Model

A small model may not take tools, and then the harness's tools, memory
among them, don't work with it. `start` and `status` mark a model whose name says it has fewer
than 30 billion parameters (`qwen3:8b`) with ⚠️, since it may not call its tools. To run, install and remove
models, see [On Your Computer](../../../../../model/docs/agents/models/local.md).

## A Hosted Model to Fall Back To

Give an agent a hosted model to fall back to, for a turn its own model can't
answer. The fallback runs when the model is off or sends nothing back for 60 seconds, when the
turn's prompt is past the model's window, or when the provider answers busy or
overloaded (429, or a 5xx).

### Choose a Configuration

Choose one primary and fallback configuration below. The local and hosted
primaries are alternatives. Turning the fallback off is a separate change
to the agent's current configuration.

### Local Primary

Keep Qwen on this computer as the primary, with a hosted OpenRouter
model available when that local model cannot answer.

```sh
lnk agent start --model.default local/qwen3 --model.fallback_model openrouter/qwen/qwen3-32b
```

### Hosted Primary

Use Anthropic for both the primary and the fallback, with the same
provider credential.

```sh
lnk agent start --model.default anthropic/claude-opus-5-5 --model.fallback_model anthropic/claude-sonnet-5-5
```

### Turn Fallback Off

Keep the current primary and turn off the saved fallback.

```sh
lnk agent start --model.fallback=false
```

### `--model.fallback_model`

`--model.fallback_model` names a hosted model as `--model.default` does, and Local Link finds its key the same way, or reads it from `LNK_FALLBACK_CREDENTIAL`. Never put the key on the
command line, which other users of this machine can see. Local Link keeps the fallback with the agent's settings
and its key with your secrets, and both move with the agent. `lnk agent
show` shows the fallback under the model.

### Give the Fallback a Key

The fallback's key stays outside the sandbox as the model's does. The proxy holds
one key, so beside a hosted model the fallback must be the same
provider's, with the same key, or `start` refuses it. A model on this
computer can't be a fallback.

### Check Which Harnesses Fall Back

Link Harness [falls back](../../../../../harnesses/link/docs/agents/link/guide.md#fall-back-to-a-hosted-model), one turn at a time.
A harness that can't fall back ignores the fallback, and Local Link keeps it for when a harness
that can runs the agent.

## A Provider's, with Your Key

Use a model from Anthropic, OpenAI, or OpenRouter with your own key.

### Know the Default Model

With a key and no `--model.default`, every harness gets
`anthropic/claude-haiku-5-5`, `openai/gpt-6-luna` or
`openrouter/anthropic/claude-haiku-5.5`, by the key's provider. All three cost the same, $0.10 a million tokens in and
$0.50 out, so switching provider doesn't change your bill. `--model.default`
names another model the provider has, such as
`anthropic/claude-sonnet-5-5` at twenty times the price; [`lnk model show <model>`](../../../../../model/docs/agents/models/guide.md#where-things-live) shows what one costs.

### Give the Key

Give the key in the provider's variable (`ANTHROPIC_API_KEY`,
`OPENAI_API_KEY`, or `OPENROUTER_API_KEY`), or paste it when `lnk` asks.
Never give the key on the command line, which other users of this machine can see
while the agent starts, so no flag takes it. Local Link keeps the key with the
agent's settings, and reads the variable again at each start to renew
the key. To change the key, start your agent again with a new one.

### Find the Key for a Named Model

`--model.default` takes the key from the variable of the provider the
[name says](#say-who-serves-it). Otherwise it takes the key the agent keeps for that
provider, and otherwise it asks for the key on the terminal and keeps it. So
`lnk agent start --model.default anthropic/claude-sonnet-5-5` is all you need to
move from a model on your computer to Anthropic's.

### Set a Key for a Local Agent

Local Link never ignores a key variable set while the agent runs a model on your computer. In a terminal, the start asks whether to switch to that
provider's default model, and otherwise says how. If more than one
variable is set and the agent has no model yet, Local Link doesn't choose
whom to bill, so unset all but the one to use.

### Keep the Key Out of Reach

Your agent never sees the key. Local Link's proxy, outside the sandbox, adds
the key on the way to the [provider](../../../../../model/docs/agents/models/providers.md#keep-the-key-outside-the-harness).

## Set its model and how hard it thinks

Your agent's [specification](../../../config/specs/agent.md) holds its
opinions of models, and Local Link reads it at every start. In this
`agent.toml`, `thinking` is `off`, `low`, `medium` or `high`, where the
model can think, `allow` lists the only models it may run, and
`[model.anthropic] default` is the model on Anthropic when you name none.

```toml
# agent.toml
schema = 1
[model]
thinking = "medium"
allow = ["claude-haiku-5-5", "claude-sonnet-5-5"]

[model.anthropic]
default = "claude-sonnet-5-5"
```

```sh
lnk agent start --spec agent.toml
lnk agent spec show
```

`lnk agent show` says the model your agent thinks with, beside the rest of what
the agent runs.

### Know Which Setting Wins

A provider's table beats `[model]` for that provider. `[model] default`
names who serves the model, as `--model.default` does, so it's the default of that
provider alone. A provider's table names its model as that provider
does. A start refuses a model `allow` doesn't list. Local Link's own specification
thinks `low` by default.

## Find More Help with Models

When a model doesn't start or answer, see
[Troubleshooting](troubleshooting.md). Every flag is in `lnk agent start --help`. [Decisions](decisions.md#which-model-does-an-agent-use-with-no---modeldefault)
says why the default is what it is.

## Understand Model Boundaries

Provider credentials stay where [Security](security.md) says, and model requests reach only what it lists. [Decisions](decisions.md) explains selection and fallback. [Troubleshooting](troubleshooting.md) helps you diagnose unavailable or incompatible models.

<!-- docs-tree:start -->

## Docs

- [Decisions](decisions.md): Why an agent selects a provider and default model this way
- [Security](security.md): How model credentials remain outside the agent and how hosting accounts supply managed models
- [Troubleshooting](troubleshooting.md): What to do when your agent's model doesn't start, answer or remember

<!-- docs-tree:end -->

# Model Provider

**Use Anthropic, OpenAI, OpenRouter, or a provider an installed module adds, with your own credential.** The provider
bills you and Link adds no inference fee. Keys on the wire keep the
credential outside a compatible harness.

## Add your key

Start your agent and give it the provider's credential when it asks.
With the credential already in its variable, such as `ANTHROPIC_API_KEY`
in the environment you start it from, it asks for nothing.

```sh
lnk agent start --model.default claude-sonnet-5-5
```

### Its default model

With a credential and no `--model.default`, every harness gets `claude-haiku-5-5` on
Anthropic, `gpt-6-luna` on OpenAI and `anthropic/claude-haiku-5.5` on
OpenRouter, each $0.10 a million tokens in and $0.50 out. Claude Haiku
5.5's prompts past 100,000 tokens cost five times as much, and GPT-6
Luna's past 272,000 twice as much in and half as much again out, so an
agent fills its window only up to there unless its specification says
[`past_price_jump`](../../../../agent/docs/config/specs/agent.md).
Every flag is in [Switch Models](../../../../agent/docs/agents/models/agent/README.md#a-providers-with-your-key).

## A Provider a Module Adds

An installed module can add a model provider of its own, reached at an
address the module declares and speaking another provider's API there.
The models module asks the installed modules once, through the question
`model.providers`, and keeps what they declare. You name its models
`<provider>/<id>` and give its key at the prompt.

### Its address

Its address is the one its module declares, never one a specification or
a team sets, so no setting can send its key, or another provider's,
anywhere else. The provider is always the one the model's name says,
never read from a key's prefix, and a key without the provider's declared
prefix is refused. With keys on the wire,
[the proxy adds its key](../../../../agent/docs/agents/models/agent/security.md#secrets)
for that one host alone.

### A name only a module provides

The models module's `providers.toml` lists the names only a module
provides, such as `link`. A model named by one goes to its module's
provider, and with no installed module providing it, Link refuses it,
saying no installed module provides link models. It never sends such a
model to OpenRouter.

## Keep the Key Outside the Harness

`lnk agent secret` says whether the harness holds its keys, or Link's
proxy adds them on the wire.

```sh
lnk agent secret wire
lnk agent secret agent
```

### Keys on the wire

With `wire`, the harness gets placeholders, and Link's proxy, which runs
outside the sandbox, adds the real model credential on the way to the provider.
It adds the real Telegram bot token on the way to Telegram the same way,
so the configured harness receives placeholders. A real credential left
in its home remains readable until removed.

### Which harnesses

Link Harness runs this way by
default, and so does every other harness whose adapter says it can,
which includes OpenClaw, Hermes Agent and DeepSeek Harness. Any other
holds its keys until you switch it. Link Harness never holds a bot
token, because Link carries its connections.

### Where it works

It works for Anthropic, OpenAI, OpenRouter, and a module's provider's credentials. With a local model
or `--network.mode host`, the setting is kept and does nothing. If a
request fails with the placeholder, `lnk agent secret agent` puts the key
back.

### A key left behind

[`lnk agent harness check`](../../../../agent/docs/agents/harnesses/use/harness/README.md#inspect-the-sandbox) reports `LEAK` if the real key or token is
still in the harness's home. What the proxy passes and refuses is in
[Security](../../../../agent/docs/agents/models/agent/security.md#secrets).

## What Link knows of a model

Link includes a model's window, the longest answer it writes, how it's
asked to think and what it costs, refreshed at every release for the
models OpenRouter lists. A model newer than your Link, or a figure
that's wrong, is yours to add or correct, and yours wins field by field.

```sh
lnk model show claude-sonnet-5-5
lnk model show claude-sonnet-5-5 --json
```

### Your own figures

Yours are in `~/.lnk/safe/models/facts.toml`, each model by the name it's
found by, its provider's or a router's, its figures over Link's.

```toml
# ~/.lnk/safe/models/facts.toml
schema = 1
[model."my-finetune"]
window = 65536
answer = 8192
thinking = "no"
price = { input = 0.2, output = 0.6 }
above = [{ tokens = 32000, price = { input = 0.4, output = 1.2 } }]
cache = [{ lasts = "5m", write = 1.25 }, { lasts = "1h", write = 2.0 }]

[model."claude-sonnet-5-5"]
window = 500000
```

### What Each Figure Says

`window` is the model's whole window in tokens, the answer included, and
`answer` the longest answer it writes. `thinking` is `adaptive`,
`budget`, `effort` or `no`. `price` is in dollars a million tokens,
cached too if it has one, and `above` is its price past a prompt of so
many tokens. `cache` lists its provider's cache, the unasked one first.
The `claude-sonnet-5-5` table corrects one of Link's figures and keeps
the rest.

### Its provider's cache

A provider keeps a prompt it was sent for a while, so the next request
that starts the same way reads that part cheaply. Writing a prompt to the
cache costs a little more than sending it, and a provider may offer
several lifetimes at different write prices. A model's `cache` lists
them, the lifetime the provider uses unasked first, each with its write
price as a multiple of input. Link's lists Anthropic's five
minutes and hour, and OpenAI's thirty minutes for its GPT-6 models. A
model with no `cache` is never asked for a lifetime. How Link chooses
one is in Link Harness's
[Decisions](../../../../harnesses/link/docs/agents/harnesses/link/decisions.md#the-prompt-cache).

### How a model is found

A model is found by its own name, with a router's prefix such as
`anthropic/claude-sonnet-5.5`, with a dated snapshot's suffix, or with
`.` for `-`. Every harness is handed the facts of its agent's model.
Why they're data and where they come from is in
[Decisions](more/decisions.md#where-do-a-models-figures-come-from).

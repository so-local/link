# Troubleshooting

What to do when your agent's [model](README.md) doesn't start, answer or remember. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## `start` offers no local models

Run [`lnk up`](../../../../../../../docs/more/modules/install.md#pick-your-modules), then [`lnk model install`](../../../../../model/docs/agents/models/README.md#install), or start your runtime,
so a model runs on this computer for `start` to offer.

## "several model runtimes are running"

Name the model with `--model.default`, so Link knows which runtime to start
your agent on.

### One Model on Several Runtimes

When several runtimes serve that same model, Link uses the one you added
if exactly one was added. Otherwise it refuses and names the runtimes.

## More than one key variable is set

Unset all but the one to use: Link doesn't choose whom to bill when
your agent has no model yet.

## "its fallback's key can't be kept on the wire beside its model's"

Fall back to a model of the same provider, on the same key, or keep
keys in the harness (`lnk agent secret agent`).

## "no model named ... runs on this machine"

A name after `local/` goes only to this computer. Name a model
[`lnk model list`](../../../../../model/docs/agents/models/README.md#list) shows, or install it with `lnk model install`. A refused model selection leaves no new agent,
including when `start` names a specification or `create` names a new agent.
Choose a served model and run the command again.

## "... is Anthropic's, and that key is OpenRouter's"

A model goes only where its name says, so a key for another provider is
refused. Set the named provider's key, or name the model with the key's
provider, such as `openrouter/anthropic/claude-haiku-5.5` for an
OpenRouter key.

## "the model answered 400 ... does not support tools"

[The model takes no tools](../../../../../harnesses/link/docs/agents/link/troubleshooting.md), and memory is a tool. Pick a model that takes tools, or stop
memory.

## A model on your computer is slow, or ignores its notes and tools

[A small model does this](../../../../../harnesses/link/docs/agents/link/troubleshooting.md) when memory is on. Use a larger model, or a hosted one, instead.

## It forgets what was said earlier

Its window is full: set a larger `[window] max` in its
[specification](../../../config/specs/agent.md), at a higher price per message.

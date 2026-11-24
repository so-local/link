# Troubleshooting

What to do when `lnk model` finds no [model](README.md), or finds more than one. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## `lnk model list` finds nothing

Run `lnk model install`, or start your runtime on its default port: the
models module finds runtimes at their default ports and at the
addresses you add.

## A runtime on another port

Run `lnk model runtime add <url>` to remember it, or pass `--upstream <url>` to
point one command at it.

## "several model runtimes are running"

Name a model, or pass `--upstream` to say which runtime one command
uses: with more than one, `lnk model` doesn't pick for you.

## A model's name on two runtimes

[`lnk agent start --model.default <name>`](../../../../agent/docs/agents/README.md#create-an-agent) takes the one on the runtime you added
(an address at a runtime's default port too, once you add it), and says
so. With neither or both added, it names each and stops: `lnk model runtime add
<url>` the one you want.

## "`lnk model` is part of the model module (lnk-model), which isn't installed"

Run [`lnk up`](../../../../../../docs/more/modules/install.md#pick-your-modules), which installs it with what goes with it, then the
`lnk model` command again. In a terminal, `lnk` offers to install it
for you.

## The agent answers "nothing answers on port …"

Its model runs on this machine, and the runtime serving it isn't
running. Start the runtime, such as Ollama, then check what it serves
with `lnk model runtime list`.

## The agent answers "… route's bridge couldn't be reached"

The sandbox couldn't start `lnk-model`'s bridge, which carries every
model call, and what follows says why. The module is usually missing or
from another install. Run [`lnk mod list`](../../../../../../docs/more/modules/guide.md#get-help) to see which `lnk-model`
is found, then [`lnk mod add model`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module), which a `lnk` built from source
builds beside itself.

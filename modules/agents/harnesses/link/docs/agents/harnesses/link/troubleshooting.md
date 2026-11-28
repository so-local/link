# Troubleshooting

What to do when Link Harness doesn't answer, or answers wrong. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## macOS: "Operation not permitted" before an answer

A native sandbox denial of `file-read-data` on the agent's `agent`
folder indicates a harness that opens the parent for reading before
accessing its conversations. Update the Link Harness and restart the
agent. The harness traverses this parent without listing it; access to
each child stays subject to the sandbox's grants.

## It doesn't answer on a connection

Run [`lnk agent log show --follow`](../../../../../../agent/docs/agents/start/log/README.md#read-the-log). The connection modules' log is
`~/.lnk/safe/agents/<agent>/bridge.log`.

## Model setup

Model errors can come from a missing selection or the provider's credential.
Inspect the returned error before changing the model's configuration.

### "Your agent couldn't answer", and its log says "the model answered 401"

Start it again with the right key: `ANTHROPIC_API_KEY=... lnk agent
start` (or your provider's variable).

### "no model named for ..."

Name the model it runs on: [`lnk agent start --model.default <name>`](../../../../../../agent/docs/agents/README.md#start-your-agent), a hosted
model's name or one [`lnk model list`](../../../../../../model/docs/agents/models/README.md#list) shows running on this computer.

## "Signal isn't linked"

Run [`lnk agent connect signal`](../../../../../../agent/docs/agents/connections/connect/README.md#choose-an-app) in a terminal, and scan the code from
the agent's phone (Settings > Linked Devices > Link New Device).

## It forgot something from long ago, or makes things up about it

Past the window it's sent, it has only its summary. A larger `[window]
max` in its specification (`lnk agent spec show`) helps, at a higher price per
message, up to the model's own. [`lnk agent memory start`](../../../../../../tools/memory-tool/docs/agents/tools/memory/README.md#memory-start) lets it search
the conversation's files, which keep everything.

## "the model refused the prompt as longer than its window"

The window you set is more than the model takes. Set a smaller
`[window] max`, or correct the model's window in
`~/.lnk/safe/models/facts.toml`, which [`lnk model show <model>`](../../../../../../model/docs/agents/models/guide.md#where-things-live) prints.

## "the model answered 400 ... does not support tools"

Pick a model that takes tools (`lnk agent start --model.default local/qwen3`), or
[`lnk agent memory stop`](../../../../../../tools/memory-tool/docs/agents/tools/memory/guide.md#stop-memory).

## A model on your computer is slow, ignores its notes, or cuts a long answer

On a model on your computer it's slow, ignores a note it was given (it says it can't see what you told
it), or can't write out a
long answer. A small model (`qwen3:8b`) takes tens of seconds a turn
with memory on, and tries a tool's call more than once. It may not use
a note in its prompt, and its runtime may run it with a window too
small for a long answer (`ollama ps` shows its context).

### Use a larger model

Use a larger model, or a hosted one: `lnk agent start --model.default
claude-sonnet-5-5`, which asks for the key unless `ANTHROPIC_API_KEY`
is set or one is kept. `start` and `status` mark such a model ⚠️.

## A skill isn't in `lnk agent show --instructions`

Its `SKILL.md` must start with front matter between two lines of `---`,
with a `name` the same as its folder's and a `description`. `lnk agent
skill add` says what's wrong with one.

## Tool failures

Tools can fail while starting or during a call. Inspect their logs to
distinguish a missing tool from one that was offered and returned an error.

### A tool is missing from its answers

`lnk agent log show` says why ("tool ... left out of this turn"). [`lnk agent show --permissions`](../../../../../../agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks) lists the tools it may call.

### A tool runs but fails

`lnk agent log show --follow` includes failed tool calls as well as startup
messages. Each notice names the tool and the returned error, without
printing the call's arguments, and is followed by the IDs of its chat and
call. An idle agent may show only its startup
lines. Reproduce the failure while following the log to capture its cause.

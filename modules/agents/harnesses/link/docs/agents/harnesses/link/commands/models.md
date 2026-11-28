# Model Commands

Model commands show and switch the model a conversation runs on, how
hard it thinks and whether it's asked fast, without a restart.

## The model namespace

The model's commands are the model namespace's actions, as [`lnk model`](../../../../../../../model/docs/agents/models/README.md#models)
has them: [`/model:show`](../../../../../../../model/docs/agents/models/guide.md#where-things-live) says what Link knows of the conversation's
model, its provider, window, longest answer, how it thinks and its
price, and [`/model:list`](../../../../../../../model/docs/agents/models/README.md#list) names the models its provider offers that its
specification allows (`[model] allow`), as the provider's own list says.
`[commands] models` controls this list separately from `[commands] model`,
which controls showing and switching the model.

## Switch this conversation

`/model:set <name>` switches the conversation it's sent in to one of
those, with no restart: its next turn uses it, and the chat's other
conversations, and other chats, keep theirs. A name of `[model.aliases]`
names the model it stands for, so `/model:set high` runs whatever the
specification calls `high`. A name the provider doesn't offer is refused, and the
model stays. Given no name, it names the model and the others.

## Switch every chat

`/model:set <name> --every-chat` switches every chat of the agent too, as
the agent's model, until the agent starts again, which runs it on the
model it was started with, by [`lnk agent start --model.default`](../../../../../../../agent/docs/agents/README.md#start-your-agent). A conversation
that chose its own keeps it. A model of another provider needs that
start. When the provider's list can't be read, `/model:list` and
`/model:set` say why, and the model stays.

## Ask for speed

`/model:set --fast` asks the provider's faster service for the
conversation's turns, and `/model:set --no-fast` stops. It's OpenAI's
priority processing, or Anthropic's fast mode on the models that have
it, at the provider's higher price. Where the provider offers no faster
service for the model, it's refused.

## Set how hard it thinks

`/model:set --effort high` sets how hard the conversation's model
thinks: `off`, `low`, `medium` or `high`, as `[model] thinking` takes,
asked where the model can. With `--every-chat`, every chat's too, until the
agent starts again. `/model:set` alone says the level beside the model.

## Kept as the conversation's

Each choice is a command event of its conversation, so a resumed
conversation runs on what it chose, and a new one on the agent's. A
subagent runs on the model it was started with, else on its parent
conversation's, and [`/task:agent`](../../../../../../../tools/scheduler-tool/docs/agents/tools/scheduler/task/README.md#delegate-work) without a `model` gives it this
conversation's.

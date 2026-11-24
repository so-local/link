# Model Contract

> Model Types Shared Across Callers

`link-model-contract` shares model facts, request types, and context reports
between model callers and the bridge. Its routing helpers describe how a
program behind a proxy reaches a provider or local runtime.

## Caller Guides

The [caller types](chat.md) explain prompt validation, tool continuation, and
provider responses. [Context caller types](context.md) explain prompt-part
measurement and report boundaries.

## Model Answers

What [`lnk model`](../../../../../../docs/agents/models/README.md#models) answers (`model.names`, `model.shown`, `model.status`
and every other output its module's `types.toml` declares) is generated
into this crate as `link_model_contract::declared`. The `lnk model`
program writes each answer through its type, and a program reading one,
such as the agent forwarding a model command, names the same type
through its build's owner for the `model` module.

## A model's facts

`lnk model
show` prints the facts, and the harness module hands a harness [its
agent's model's facts](../../../../../../docs/agents/models/providers.md#what-link-knows-of-a-model) with its settings (`Settings::model_facts`), so
any harness reads them.

### Facts Fields

Each model's facts (`link_model_contract::models::Facts`) are its
window and longest answer in tokens, how it's asked to think and what
it costs, each optional, so a file may correct one field of another's.

### `thinking`

How it's asked to think (`Thinks`): `no`, not at all or not in a way
Link knows; `adaptive`, Anthropic's adaptive thinking at an effort;
`budget`, Anthropic's thinking within a budget of tokens; or `effort`,
reasoning at an effort, as OpenAI and OpenRouter ask it.

### `price`

What it costs in US dollars a million tokens (`Price`): `input`,
`output` and, when it has one, `cached`. `Price::of` prices a turn's
tokens.

### `above`

Where its price jumps (`Jump`), lowest first, as its provider charges
a long prompt: past a jump's `tokens`, the whole prompt with the
cache's tokens, a call costs that jump's `price`, input, output and
cache alike. Claude Haiku 5.5's jumps past 100,000 tokens, GPT-6
Luna's past 272,000.
`Facts::price_at` is the price of a prompt's size, and `first_jump`
where a harness that keeps within the first price stops (an agent
specification's `[window] past_price_jump`).

## Tool Call IDs

`chat::calls` normalizes call IDs once for every harness. IDs are at most
64 ASCII letters, digits, underscores, or hyphens. Missing IDs are named
from the response sequence and position, and colliding IDs within a
response are refused. Shared result matching refuses missing, duplicate,
or unasked-for answers. A provider can repeat an ID in another response,
so durable execution identity also needs its originating response.

## Link's file and yours

Link includes its model facts as a file (`LINK_MODELS`), refreshed with the
models' figures at every release. Yours, `~/.lnk/safe/models/facts.toml`
(`path`, `yours`), adds a model or corrects one, field by field, yours
winning (`facts`, `known`), so a model released after your Link is
described without a new one.

### Read strictly

`parse` reads and validates a models file strictly. The `yours` file
reader checks its metadata against the 1 MiB `MAX_BYTES` limit before
reading it. `check` refuses an empty name, one past 128 bytes or containing
a control character, a window under 1024 tokens, an answer under 256, or
a negative or non-finite price. Jumps need a base price, strictly rising
thresholds, and a threshold below the window when the window is known.

### How a name is matched

`plain` is a name as Link's files match it: its own, after a router's
prefix (`anthropic/claude-sonnet-5.5`), without a dated snapshot's
suffix (Anthropic's `-20251001`, OpenAI's `-2024-08-06`), lowercase,
`.` read as `-`.

## Refreshed at a release

`refresh` builds the next file from OpenRouter's models list, and is an
error for an answer without a model array or with fewer than ten
entries. It begins with the current file, preserving its figures when an
update is refused. A figure that moves past `plausible_change` keeps
the current file's, and the change is said, one line each. OpenRouter
lists one price a model, so a model's jumps stay as the current file
has them.

### Models added and retired

A model `retired` lists, by its `plain` name, is never added back,
under any router's name for it. A first party's model the file doesn't
have is added only when `current_generation` holds: its line, by
`generation`, is one the file describes, and its generation is newer
than any the file has of it.

### `plausible_change`

A change its provider could make: never a window or longest answer more
than halved, or a price moved more than tenfold, a price of nothing or
of anything from nothing included.

## The model routes

`link_model_contract::models::route` holds the routes `lnk-model`'s
bridge serves: a hosted model's at `/_link/model` (`HOSTED`), its key
added outside the sandbox, and this machine's model runtimes' at
`/_link/here/<port>`, for inference only. How each provider is spoken
to is the bridge's.

### Hosted providers

The route adds keys for `anthropic`, `openai` and `openrouter`
(`PROVIDERS`). `url` is where a program behind its proxy reaches one:
the base URL its client takes in place of the provider's, `/v1` added
by Anthropic's clients, not by OpenAI's and OpenRouter's.

### A model at an address of its own

A model at an address of its own, the one a module declares for its
provider or a `base_url`, is reached as the provider it speaks there, at that one host
(`At`): its API's root before the `/v1` its provider's clients take. The
bridge passes it inference alone, its server tools removed, as it does a
provider's own. An address speaking OpenAI's API whose path doesn't end
with `/v1`, or Anthropic's that does, is refused, since inference there
can't be told.

#### Its route

Its route is apart from the provider's: `/_link/model-at` (`HOSTED_AT`)
for an agent's model and `/_link/model-at-fallback` for its fallback.
`Hosted::url` is where its program reaches it.

### The key

An agent's route names its key, and the bridge reads it from what the
store grants the route's caller at each call (`Hosted::with_key`), so a
key removed stops at the next. A program is given a placeholder in its
place (`placeholder`): its kind's prefix (`sk-ant-api03-`, `sk-or-v1-`,
`sk-proj-`), which clients look at, and a tag, none of the secret.

### Models that search on their own

`searches_by_nature` says whether a model searches the web whatever a
request asks: Perplexity's Sonar models, and OpenAI's search and deep
research models, directly or on OpenRouter. Its provider fetches pages
for the program, past any list of hosts its sandbox has.

<!-- docs-tree:start -->

## Docs

- [Model Caller Types](chat.md): Share typed prompts, tool results, and completed responses between a harness and the model bridge
- [Context Caller Types](context.md): Attribute a model request to stable prompt parts without saving its text

<!-- docs-tree:end -->

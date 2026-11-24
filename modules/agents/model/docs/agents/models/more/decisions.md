# Decisions

Why the models module works the way it does. What it does is the
[feature page](../README.md); what it protects is [Security](../security.md).

## What does Link manage for models?

Ollama. Link wraps it rather than running models itself: it installs the
official Ollama if it's missing, starts it, and pulls and deletes models
through Ollama's API. `install` and `uninstall` work with Ollama; their installer and model catalogue stay with that runtime. You install other runtimes yourself, and Link discovers and serves them.

## What is the contract with a runtime?

The OpenAI-compatible `/v1` API. `list` asks each runtime's default port
for `GET /v1/models` and takes whatever answers with an OpenAI-style
model list. `serve` forwards only `/v1/...` and streams the response
back, so a runtime's management API, such as Ollama's `/api/pull` and
`/api/delete`, stays private.

## Where does Ollama come from?

Ollama's own downloads, not a copy Link pins and runs as its own service.
The app on macOS starts itself at login and updates itself, and the Linux
install script's system service survives reboots. The cost: Link trusts
ollama.com's HTTPS and, on macOS, Apple's notarization, rather than
checksums of its own.

## Which model does Link recommend?

Qwen3, sized by memory, on Apple Silicon only. It calls tools, which
agents need, and runs well in 8 to 32 GB of unified memory. On Intel Macs
and servers without a GPU a local model is too slow for an agent, so an
API key is the better default there. `lnk model install <model>` still
works.

## Where do a model's figures come from?

From a file of model facts Link includes, not code: a model's window, longest
answer, how it thinks and its price change with every model released,
and a table in a harness would need a release to change.

### Refreshed at every release

The file is refreshed at every release from OpenRouter's public models
API, which needs no key and lists the models of every provider Link
reaches, with their windows, longest answers and prices per token; how
a model thinks is kept as Link wrote it, since OpenRouter says only
whether it reasons. The refreshed file is built in, not committed back,
so the release needs no write access to the repository.

### A release that stops

A release whose refreshed file doesn't read back whole, or no longer
describes Link's own default models with figures a model could have,
stops.

### Bound by the committed file

A change to a model the committed file has is bound by it: a window or
longest answer more than halved, or a price moved more than tenfold,
keeps the committed figure and is said in the release's log, so a wrong
answer from OpenRouter can't shrink every agent's window or zero its
costs.

### No walking past the bound

Each change is weighed against the committed figure, never one the same
answer changed, and a model the answer lists under several ids, none
its own, keeps the committed figures: so a list of aliases can't walk a
figure past the bound a step at a time.

### Current models only

The file describes only the models their providers currently offer.
When a newer model replaces one in its line, as Claude Opus 5.5 replaced
Claude Opus 5 and the 4.x Opus models, the older one leaves the file and
is listed in `retired.toml` beside it, with the model that replaces it.
Older OpenAI models went the same way: GPT-5, GPT-4.1, and GPT-4o give
way to GPT-6.1 Sol, and their mini and nano sizes to GPT-6 Luna.

### Only the current generation enters

A refresh adds no retired model back, under any router's name for it,
and adds a model of Anthropic or OpenAI only as a newer generation of a
line the file describes. A model's line is its name without its numbers,
and its generation is the numbers in order, so `claude-sonnet-4-5` and
`claude-3-5-sonnet` are both older Claude Sonnets than
`claude-sonnet-5-5`, and `claude-opus-6` would enter as a newer Opus.
A first party's new line, which needs how it thinks and its cache
written by hand, is added by hand. Another vendor's models enter as
OpenRouter lists them.

### Retired models still run

A retired model can still be named: it reaches its provider while the
provider serves it, and Link describes it no more, so its window is
`[window] unknown`'s unless your facts file describes it.

### Yours over it

Your [model facts file](files.md#lnksafemodelsfactstoml) overrides the model facts Link includes field by field. Describing a new model there updates its metadata without changing the provider protocol or requiring a Link release.

### Every harness reads them

Each harness gets its agent's model's facts with its settings
(`model_facts`), so any harness, not only Link's, reads the same
figures. Link Harness takes a model's window from yours first, then
from what a runtime on the machine reports, then from Link's, and
prices a turn its provider didn't price.

## Why does a sandbox see a model's details in one shape?

A program in a sandbox that read each runtime's own details route
would learn every runtime's shape, and each new runtime would be a new
shape to learn. Those routes also answer more than a model's details:
where its files are on the machine, often with the user's name, its
modelfile, template, license and settings. So the local model route
answers `/_link/model/show` itself, from typed fields, and refuses the
runtimes' own routes: a runtime is one mapping outside the sandbox.

### Why reuse runtime clients?

Metadata and local typed actions reuse loopback HTTP transports without
proxies, redirects or TLS trust roots. Every lookup still reads fresh runtime
metadata. A hosted action uses its own client bound to its proved Unix relay;
that relay owns TLS to the provider. Grants, credentials, model choices and
responses remain current.

### Why a `GET`

It is a `GET` with the name in its query, since it reads: the route reads
no body, and the name is checked as plain before anything is asked.

## Why is a model `uninstall`, and a runtime `remove`?

Every `lnk` command [pairs its verbs](../../../../../../../docs/more/developing/modules/specs.md#name-a-command) by the kind of thing: `install`
and `uninstall` for software Link downloads, a model, and `add` and
`remove` for what runs elsewhere and Link takes on, a runtime.

## How does a model at its own address reach an agent?

`lnk model runtime add <url>` remembers it, and discovery asks it as it asks the
default ports. So `lnk model runtime list --json`, and with it [`lnk agent start`](../../../../../agent/docs/agents/README.md#create-an-agent),
the tunnel and every harness, see it without a flag of their own, and a
model someone is building needs no port trick. Only an address on this
machine is added: discovery never goes through a proxy, and the sandbox
lets a harness reach a local port, not another host. `lnk model runtime remove`
drops it, and `list` shows an added address while nothing answers there,
so a server being restarted isn't forgotten.

## How is context reported?

Each model call's prompt is taken apart once, where every harness's
calls pass, and [kept as names and numbers](../context.md).

### Where is a call's context taken apart?

In the model bridge, from the request each call sends its provider.
Every harness's model calls pass the bridge, an outside harness's own
request on the model route and Link Harness's through the model
actions, so one census reports them all and no harness reports its own.
The census reads the provider's JSON by the API it speaks and keeps
names and numbers only: a report holding a word of a prompt would be a
second copy of the conversation.

### How are a part's tokens known?

By splitting the provider's measured prompt among the parts, each by
about as many tokens as its kind takes. Counting each part with the
provider would cost a call a part, and a tokenizer of each model would
be code to write and release for each. The split adds up to what the provider bills,
and it says so: `apportioned`, or `estimated` where nothing was measured.

### How is a skill named?

By where its caller wrote it. Link Harness composes its system prompt,
so it sends each section's and skill's byte range with the call, and
the bridge counts each beneath `instructions` by its name. Ranges cost a
few lines and are exact. An outside harness writes its own prompt, so
its instructions stay one part: finding Link's texts in it would take
fingerprints and a matcher to maintain for one more level of detail.

### How is a chat's own work counted?

In the chat that asked for it. A call names its parent, and the report
counts each child's parts beneath its parent's, so a chat's cost holds
the work it asked for, while its window holds only its own. Link
Harness names its thread as the parent of its compaction and goal
calls. A subagent's messages don't carry its parent, so its calls name
none, and a subagent is a chat of its own.

### What names a chat?

Its caller's name for it: Link Harness's thread, sent with the typed
request. An outside harness's requests carry no chat, and every call of
a conversation repeats its opening message, so that message's
fingerprint names it without keeping its words.

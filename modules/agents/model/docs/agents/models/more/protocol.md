# Interface

What other programs can rely on from `lnk model`.

## `lnk model`

The commands of `lnk model` other programs read, each with its output
and who reads it.

### `lnk model runtime list --json`

`[{"name", "base_url", "models": ["..."], "added"?, "state"}]`, one
entry per runtime found at a default port, and one per address `lnk
model runtime add` remembers, answering or not: `name` then the one it
was given, and `added` true. An address added at a runtime's default
port keeps that runtime's name while it answers, `added` true too.
`state` is `running` for one answering with models, `no_models` for one
answering with none, and `not_answering` for an added address nothing
answers at, its `models` empty: it's listed until `lnk model runtime
remove` forgets it.

#### `--start`

With `--start`, it lists after starting an installed Ollama that isn't
running. Read by [`lnk agent start`](../../../../../agent/docs/agents/README.md#create-an-agent) (`--start`: offers the models here,
Ollama's too when it was stopped).

### `lnk model serve --json`

One line, `{"url", "description"}`, once it listens on a `127.0.0.1`
port; it stops when its stdin closes. Exit status 2: no runtime here
serves the model named.

#### Following the owner

Its stdin is how it follows its owner: the program that started it holds
the other end, so when that program ends, however it ends, stdin closes.
The port then closes at once, so a new connection is refused. A request
it had already taken is still answered, for up to 10 seconds, and then
the server ends.

#### A memory engine's `model`

`lnk agent memory serve` reads it for a memory engine's `model`, and
passes `<url>` to the engine as `$LNK_MEMORY_MODEL`, empty when serving fails
or when no line comes within 10 s.

#### Link Memory's `--embed`

Link Memory with `--embed <model>`, run outside a sandbox, reads it and
calls `<url>/v1/embeddings` while it serves; when serving fails it searches by
words and says `lnk model install <model>`.

### `lnk model list --fit --json`

A list of model names, as `lnk model list --json` writes them, holding
the one model that suits this machine, or none where no local model
does. Read by `lnk agent start`, which offers to install it when no
model runs here.

### `lnk model install <model> --yes`

Exit status 0 once the model is pulled and Ollama lists it. Read by
`lnk agent start`, which installs the local model picked.

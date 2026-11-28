# Model Details

**A program in a sandbox asks a model's details in one shape, whatever
runs the model.** The local model route answers `GET
/_link/model/show?name=<name>` itself: outside the sandbox it asks the
runtime in that runtime's own way, and answers with Link's fields alone.

## Ask

From a sandbox, at the local model's address its program was given:

```sh
curl "$LNK_LOCAL_MODEL_11434/_link/model/show?name=qwen3:8b"
```

The name is the model's, as its runtime names it: letters, digits and
`-_.:/`, at most 256 characters, each part between two `/` more than
dots.

## What it answers

The answer is JSON, `link_model_contract::show::Shown`, each field left
out where the runtime doesn't say it.

### Its fields

`name` is the model, as it was asked for; `family` its family or
architecture (`llama`, `qwen3`); `parameters` its size in parameters,
as its runtime writes it (`8.0B`); and `quantization` how its weights
are quantized (`Q4_K_M`). `context` and `trained` are the window its
runtime runs it with and the one it was trained for, each in tokens,
and `capabilities` what it can do: `completion`, `tools`, `vision`,
`embedding` and `thinking`.

### What never reaches the sandbox

A word the runtime says of the model is kept only when it's 1 to 64
letters, digits and `-_.:`, so no path or sentence of the runtime's
reaches the sandbox: its modelfile, template, license, settings and
where its files are stay outside.

## What it asks

Each runtime is asked in turn, and the first answer that maps is
Link's.

### Ollama

`POST /api/show`, then `GET /api/ps`: its family, size, quantization
and capabilities, the window it runs the model with (loaded, else its
`num_ctx`) and the one it was trained for.

### LM Studio

`GET /api/v0/models/<id>`: its family, quantization and capabilities,
the loaded window and the longest.

### llama.cpp and OpenAI-compatible runtimes

llama.cpp is asked `GET /props`, and an OpenAI-compatible runtime
(vLLM) `GET /v1/models`, the model's entry: each gives the window it
runs the model with, where it says.

### An answer none of them maps

An answer none of them maps is refused, `403`, and so is one past 1 MB
or a show that takes more than two and a half seconds. A runtime Link
learns to ask is one mapping more; the shape a sandbox sees stays the
same.

## The model list

`GET /v1/models` is the route's too: the runtime's list, rebuilt with
each model's id alone, `{"id", "object": "model"}`, and only an id that
is a plain name, so a file's path as an id is left out.

## The runtimes' own routes

A program in a sandbox can't ask any of the runtimes' detail routes
itself: [the local model route](../../../../../../security/sandbox/contract/route/docs/tooling/sandbox/more/contract/models-and-secrets.md#a-local-models-port) refuses each, and passes only inference,
minimal native lists, health and version compatibility answers.

## Metadata words

Family and quantization pass only when they name a recognized model
architecture or weight format. Parameter sizes are numeric counts,
optionally with a K/M/B/T scale. Unknown values are omitted, including
plain metadata words that could name a host user. New architectures
need a mapping before their metadata is exposed to a sandbox.

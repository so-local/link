# On Your Computer

Run a model through a runtime on your computer, without a Link inference fee. `lnk` installs models with Ollama or discovers a runtime you already run; that runtime’s configuration determines whether inference stays local.

## Install a model

`install` asks before it installs Ollama, and `--yes` skips the
question.

```sh
lnk model list --fit
lnk model install
lnk model install qwen3:8b
```

### What it picks

On an Apple Silicon Mac it picks the largest Qwen3 that leaves room for
everything else. Anywhere else, or with less than 8 GB, it recommends
none, but `install <model>` still works.

| Memory | Model |
|---|---|
| 8 GB | `qwen3:4b`, about 2.5 GB |
| 16 GB | `qwen3:8b`, 5.2 GB |
| 32 GB or more | `qwen3:14b`, 9.3 GB |

## See what's running

`list` finds Ollama, LM Studio, llama.cpp, vLLM and Jan on their default
ports, which are 11434, 1234, 8080, 8000 and 1337. It also finds [the
addresses you add](runtime/README.md#use-a-model-at-its-own-address).

```sh
lnk model list
lnk model list --start
lnk model runtime list
```

`runtimes` gives each runtime one state: `running` when it serves
models, `no models` when it answers and serves none, and `not
answering` for an address you added that doesn't answer now.

## Serve one on this machine

Anything that speaks the OpenAI API can then use the model.

```sh
lnk model serve --model qwen3 --port 9100
lnk model serve
```

### How it serves

It serves until you stop it, and starts Ollama if it's installed but
stopped. A named model answers every request, whatever model the request
asks for. To serve a runtime on a port that isn't its default, or any
other OpenAI-compatible server, add `--upstream <url>`.

## Uninstall a model

It deletes the model from Ollama and frees its disk space.

```sh
lnk model delete qwen3
```

[`lnk uninstall`](../../../../../../docs/more/modules/guide.md#uninstall) offers the models module's two parts: `models`, every
model Ollama runs here and the runtimes you added, and on a Mac
`ollama`, the Ollama app. Ollama on Linux is a system service its own
installer set up as root, which stays: remove it as Ollama's
documentation says.

## Use it from your agent

[`lnk agent start`](../../../../agent/docs/agents/README.md#start-your-agent) [offers the models running here](../../../../agent/docs/agents/models/agent/README.md), or the recommended one
to install, so your agent needs no credential. An embedding model lets your
agent's [memory](../../../../tools/memory-tool/docs/agents/tools/memory/guide.md#search-by-meaning)
find a note by what it means, not only its words.

```sh
lnk model install qwen3-embedding
```

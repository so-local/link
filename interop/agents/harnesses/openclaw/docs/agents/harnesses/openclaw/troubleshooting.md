# Troubleshooting

What to do when [OpenClaw](README.md) doesn't find its model, loses a turn or keeps
starting outside Link.

## "OpenClaw runs local models through Ollama"

The model is in LM Studio, llama.cpp, vLLM or Jan. Pull it in Ollama,
or use [Hermes](../../../../../hermes/docs/agents/harnesses/hermes/README.md).

## No local model offered

Ollama isn't installed or isn't running. [`lnk up`](../../../../../../../../docs/more/modules/install.md#pick-your-modules) installs it, and the
Ollama app or `ollama serve` in a terminal of its own keeps it running.

```sh
lnk up
```

## It answers "Auto-compaction could not recover this turn"

The model's context window was too small. [`lnk agent start`](../../../../../../../../modules/agents/agent/docs/agents/README.md#create-an-agent) again sets
32,768 tokens. A model that can't hold that much needs more memory, or
a hosted model.

## Something else on your Mac uses port 18789

Or [the ports after it](security.md#its-ports). Stop it, because
`main`'s ports are fixed. Another agent's are in its own block, which [`lnk agent list --json`](../../../../../../../../modules/agents/agent/docs/agents/start/create.md#run-more-than-one-agent) shows.

## An old OpenClaw install keeps starting outside Link

Its own service, from before Link ran it, is still installed outside
its home. See [Clean up an old OpenClaw
install](guide.md#clean-up-an-old-openclaw-install).

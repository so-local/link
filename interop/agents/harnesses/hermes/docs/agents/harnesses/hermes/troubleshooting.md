# Troubleshooting

What to do when [Hermes Agent](README.md) doesn't install, start or remember.

## It fails or forgets with a local Ollama model

Inspect the runtime’s context limit when its error says the prompt is too
large. Increase that limit within the selected model’s capacity and your
computer’s memory. For example, a model that supports 64,000 tokens can
be served with `OLLAMA_CONTEXT_LENGTH=64000 ollama serve`.

## The install fails at `git clone` on a Mac

Its installer needs git, from the developer tools, which are on unless
you denied them. Allow them again:

```sh
lnk agent permission add --commands.developer_tools
```

## Install fails at Node on Linux with `libatomic.so.1`

Its installer runs Node, which needs the `libatomic1` library, missing
from minimal Ubuntu and Debian images. The installer tries to add it with
`sudo`, which Link's sandbox refuses, so install it yourself, then set
Hermes again, with `sudo apt-get install -y libatomic1`.

## Something else on your Mac uses port 8642

Or [the ports after it](security.md#its-ports). Stop it, because
`main`'s port is fixed. Another agent's is in its own block, which [`lnk agent list --json`](../../../../../../../../modules/agents/agent/docs/agents/start/show.md#list-your-agents) shows.

# Inspect Your Agent

See everything about your agent in one readout, or print the prompt its model gets.

```sh
lnk agent show
lnk agent show work
lnk agent show --instructions
```

## Everything About It

`lnk agent show` reads the agent whole. It opens with its name, state
and machine, then gives each part of it a section.

### Its Sections

Model gives its model, fallback and the context window its specification
prefers. Harness lists its harness, tools and skills. Connections and
Memory say what it's connected to and what it remembers. Sandbox gives
each kind of grant on its own line, then its network. Who gives its ID,
owner and teams, and Today its memory and spend once [`lnk measure`](../../../../../tooling/measure/docs/tooling/measure/README.md#measure) has
them. Where names each of its folders and files on this machine.

### As JSON

`lnk agent show --json` prints its name, harness, model and specification
opinions as one JSON document, for a program to read.
`lnk agent show --json` prints what `lnk agent list --json` gives for
it.

## The Harness's Prompt

Link Harness shows the prompt it assembles from the agent's instruction
files, tool descriptions, skills and notes. [Link Harness](../../../../harnesses/link/docs/agents/link/guide.md#tell-it-who-to-be)
explains its section order, caching and editable sources. A harness
from outside Link builds its own prompt, which this command cannot show.

## Read Its Conversations

[Read Its Conversations](../chat/README.md#read-its-conversations) covers saved chat history and converting Link Harness conversations between storage formats.

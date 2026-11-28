# List and Show Agents

List your agents, ask whether one runs, and see everything about it in one readout.

```sh
lnk agent list
lnk agent show
lnk agent show --instructions
```

## List Your Agents

`lnk agent list` asks your boxes for their agents too. `--refresh` also
asks your clouds for boxes made on your other computers, and `--here`
lists only this machine's agents, asking nothing else.

```sh
lnk agent list --refresh
lnk agent list --here
```

## Check Whether an Agent Runs

`status` says one of five states. **running** means its harness answers,
and **running in a terminal** that it runs there, from
`lnk agent start --foreground`. **asleep** means it stopped for being
idle and its next message starts it. **stopped** means it isn't running.
**not answering** means it runs but its harness doesn't answer, and
[`lnk agent log show`](log/README.md) shows why. Asleep and the two running states exit with success, the others with a failure.

```sh
lnk agent show work
lnk agent log show --follow
lnk agent log show --live
```

## Paused Wires and JSON

`status` succeeds for running and asleep agents. When its proxy has
paused its wire, or every wire is stopped, it says so and why. On a box,
where no notification shows, that's where you see it. For other
programs, `status --json` and `list --json` are in
[contracts](../../../../../../docs/more/modules/contracts.md).

## Everything About an Agent

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

## Prompt the Harness Builds

Link Harness shows the prompt it assembles from the agent's instruction
files, tool descriptions, skills and notes. [Link Harness](../../../../harnesses/link/docs/agents/harnesses/link/guide.md#tell-it-who-to-be)
explains its section order, caching and editable sources. A harness
from outside Link builds its own prompt, which this command cannot show.

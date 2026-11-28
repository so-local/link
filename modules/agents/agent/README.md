# Agent

The `lnk agent` module starts, uses, connects, and moves an agent. It
runs the agent's harness sandboxed in the background, and it starts the
connections, tools, and stores the agent uses.

<!-- docs-tree:start -->

## Docs

### Pages

- [Agents](docs/agents/README.md): An agent is an AI that gets things done for you
  - [What Are Agents](docs/agents/agent.md): An agent is an AI that gets things done for you
  - [Start Your Agent](docs/agents/start/README.md): Your first `lnk agent start` creates Onboarding, an agent that answers what Local Link can do
  - [Talk to Your Agent](docs/agents/chat/README.md): Talk to your running agent in a terminal, send a message from a script, or text your agent from your phone
  - [Create a Shareable Agent](docs/agents/share/README.md): People share agents the way they share open-source code: start from one someone made, make it your own, and pass it on to your friends or publish it for everyone
  - [Create an Agent Team](docs/agents/team/README.md): A team splits a job by purpose
  - [Move Your Agent](docs/agents/move/README.md): Move your agent to a box in your own cloud, and back
  - [Migrate to Local Link](docs/agents/migrate/README.md): Bring an agent you already run with OpenClaw, Hermes, or DeepSeek under Local Link
  - [State](docs/agents/state/README.md): An agent's state is the files and folders the agent has access to, and the ones that make its tools work
  - [Switch Harnesses](docs/agents/harnesses/use/README.md): Switch the harness that runs your agent with `lnk agent harness set <harness>`
  - [Switch Models](docs/agents/models/agent/README.md): Change what your agent thinks with, in one command
  - [Connect Messaging Apps](docs/agents/connections/connect/README.md): Connect your agent to a messaging app so you can text the agent from your phone or workspace
  - [Tools and Skills](docs/agents/tools/use/README.md): Choose the tools your agent can call, and add reusable instructions for recurring work
- [Permissions](docs/tooling/sandbox/permissions/README.md): What your agent may do comes in two layers: what you grant it, and what its harness asks for beyond Local Link's defaults

### References

- [Agent Specification](docs/config/specs/agent.md): An agent, written down: the file `lnk agent start --spec` reads and `lnk agent spec show` prints
- [Team Specification](docs/config/specs/team.md): A team's file: its head (`[access]` among it), its roster and its shared settings, the opinions every member shares in `[settings.agent]`
- [Agent Commands](docs/more/reference/modules/agents/agent.md): Every command the `agent` module offers, from its declarations, in order of the resource each acts on

### Packages

- [Agent Contract](contract/README.md): `link-agent-contract` defines what an agent is, for every program that reads an agent without running it

### More

- [More](docs/agents/more/README.md): Inspect your agent's configuration and stored files, understand its security boundaries, and integrate with its runtime interfaces

<!-- docs-tree:end -->

- [Agent Configuration](docs/agents/more/settings/README.md): An agent's specification, this machine's settings, and the files `lnk agent` keeps for each agent
- [Decisions](docs/agents/more/decisions.md): Why agents work the way they do
- [Interface](docs/agents/more/interface/README.md): What other programs can rely on from `lnk agent`: its commands' output, the commands a move runs, and what other modules ask of it
- [Agent Bridge](docs/agents/more/interface/bridge.md): What `lnk agent` runs beside an agent's harness to carry its connections and serve Link's tools
- [Troubleshooting](docs/agents/models/agent/troubleshooting.md): What to do when your agent's model doesn't start, answer or remember
- [Security](docs/agents/more/security.md): What each part of an agent is kept from reaching, starting with the harness that runs it

## Code

- [`adapter.rs`](src/adapter.rs): the harness program's commands
- [`dispatch.rs`](src/dispatch.rs): each `lnk agent` command, sent to what runs it
- [`launching.rs`](src/launching.rs): a harness set up and started
- [`settings.rs`](src/settings.rs): an agent's settings file
- [`transfer.rs`](src/transfer.rs): moving an agent between machines

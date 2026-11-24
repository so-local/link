# Scheduler Contract

`link-scheduler-tool-contract` holds the flags the agent module starts Link Scheduler with, taken from the agent's specification and teams, and the caps that every machine running the tool obeys.

<!-- docs-tree:start -->

## Docs

- [Scheduler Contract](docs/agents/tools/scheduler/more/contract.md): `link-scheduler-tool-contract` turns an agent's specification (`[tool.scheduler]`) and its teams into the flags Link Scheduler takes, so its numbers are always the specification's, never the program's nor the machine's

<!-- docs-tree:end -->

## Code

- [`cli.rs`](src/cli.rs): lnk-scheduler-tool's command line, one type per command
- [`scheduler.rs`](src/scheduler.rs): launch settings and caps

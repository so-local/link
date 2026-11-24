# Link Scheduler

Link Scheduler (`lnk-scheduler-tool`) sends an agent messages from someone other than you, now or later. Subagents and schedules both work this way.

<!-- docs-tree:start -->

## Docs

### Pages

- [Scheduler](docs/agents/tools/scheduler/README.md): Your agent can start subagents, schedule messages to itself, and message the rest of its team
  - [Scheduler Guide](docs/agents/tools/scheduler/guide.md): Enable Scheduler to delegate tasks, send team messages, and arrange future turns for your agent
  - [Task Actions](docs/agents/tools/scheduler/task/README.md): Use `lnk agent task` to delegate work or run a command through an agent’s enabled providers
  - [Cron Actions](docs/agents/tools/scheduler/cron/README.md): Use `lnk agent cron` to schedule a prompt for an agent from your terminal, and to list or delete its schedules

### Packages

- [Scheduler Contract](contract/README.md): `link-scheduler-tool-contract` holds the flags the agent module starts Link Scheduler with, taken from the agent's specification and teams, and the caps that every machine running the tool obeys

### More

- [Scheduler Internals](docs/agents/tools/scheduler/more/README.md): Inspect the scheduler’s launch contract, durable runner, and delegation boundaries

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): subagents and schedules
- [`main.rs`](src/main.rs): the program
- [`mcp.rs`](src/mcp.rs): the scheduler as a tool
- [`store.rs`](src/store.rs): where jobs are kept

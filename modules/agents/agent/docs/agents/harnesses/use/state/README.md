# Export Agent State

Capture your agent's portable state for inspection without starting a model request. With an existing `main` agent on this machine, the command prints the machine-readable result, which you can save under `~/Link`, such as in `~/Link/agent-state.json`.

```sh
lnk agent state export --agent main --json
```

## Capture Committed State

The command uses the same Agent State Protocol (ASP) capture as a harness switch. A running harness answers through its existing endpoint. Local Link reads a stopped harness through its adapter when the adapter supports offline capture. Link Harness supports both paths through the same conversation store and projection. A harness that cannot export while stopped asks you to start it first.

### Read the Retained Contents

The result contains the state kinds the harness supports. The `sessions` hold portable conversation updates and stored media. Capture reads committed records without stopping work. A request still running can therefore appear unfinished, and later activity falls outside the snapshot.

## Inspect Execution Evidence

Link Harness adds a versioned `evidence` manifest beside each session's updates. It preserves native tool arguments, captured replies, supplied instructions, timing, and declared omissions through the shared [execution evidence contract](../../../../../../harnesses/portability/contract/wire/docs/agents/harnesses/more/vocabulary/wire.md#execution-evidence). Repeated provider call IDs do not identify separate executions by themselves.

### Treat Imported Records as Archival

Imported updates remain archival, including any metadata they carry. Imported updates do not establish execution by the current harness. A missing native evidence record remains unverified. Skill loading records establish what was supplied, while the resulting work still needs inspection.

### Check Snapshot Completeness

Snapshot completeness concerns retained records. Partial log tails, incomplete compact frames, and a missing prefix remain explicit gaps. A store without a snapshot guarantee reports unknown. A complete snapshot can still contain unfinished calls or omitted reply contents.

### Protect Archive Access

The owner archive retains conversation content, tool inputs and outputs, supported media, and archival thinking. Store the archive with the same access as the agent's files. The native evidence manifest excludes thinking.

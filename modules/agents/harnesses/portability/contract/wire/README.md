# Harness Wire

`link-harness-wire` holds the protocol-free vocabulary that a harness and its agent's host share. It covers message threads, prompts and their answers, and skills as the Agent Skills standard writes them.

<!-- docs-tree:start -->

## Docs

- [Harness Wire](docs/agents/harnesses/more/vocabulary/wire.md): `link-harness-wire` holds the words a harness's messages and skills are written in: Link's own connection's threads, what came of a message, and a skill as the Agent Skills standard writes one
- [Harness Prompts](docs/agents/harnesses/more/vocabulary/prompts.md): `link-harness-wire`'s `runtime` is how an agent's host calls its harness when both run in one process

<!-- docs-tree:end -->

## Code

- [`evidence.rs`](src/evidence.rs): durable runtime observations and their declared omissions
- [`message.rs`](src/message.rs): a message thread
- [`runtime.rs`](src/runtime.rs): a prompt, who sent it, what's streamed back and what came of it
- [`skill.rs`](src/skill.rs): a skill and its `SKILL.md`

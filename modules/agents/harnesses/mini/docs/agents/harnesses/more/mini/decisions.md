# Decisions

Mini is the smallest persistent, tool-using harness built on Link's model and tool interfaces. Its implementation is a readable reference for learning, contract tests and simple applications.

## Core, Runtime and Link

The core owns the model-and-tools loop. The runtime owns complete conversation records and one optional instruction file. The Link adapter owns the harness contract's commands and protocol connections. This separation makes the loop understandable without learning Agent Client Protocol (ACP), provider APIs or filesystem policy first.

### Shared Model Interface

Mini delegates provider protocols, streaming and cache controls to Link's model interface. It keeps the complete typed history and stable tool definitions that the model layer needs. The shared model layer understands tool calls, while Mini retains the loop that executes them.

## Reference Functions

The principal functions name stable responsibilities rather than Rust-specific mechanisms. An implementation can be compared through `run_turn`, `ask_model`, `call_tool`, `load_conversation`, `append_message` and `read_instructions`. The worked JSON tool turn defines their observable ordering. Internal helpers and language syntax are free to differ.

### Documentation Structure

The overview follows a turn from the instruction's source and incoming message through model requests, tool results and completion. It introduces the repeated exchange as the agentic loop after explaining the sequence. The deeper pages follow `src/core`, `src/runtime` and `src/link`. Their source links and executable example use the same calls and data, so the explanation can be checked against the code.

### Language Scope

This package implements Mini in Rust. The principal operation names describe responsibilities without requiring the same helper functions or language constructs in every language.

## Add Behavior at the Boundary

The core's supplied operations separate its ordering from model access, tool execution and recording. An application can build on those operations without copying the loop. [Extend Mini](extensions.md) locates context preparation, skills and incoming scheduled messages around a turn. The current `link` folder is host integration, not an extension registry, and Mini and Link Harness still have separate implementations.

### Tools Are Optional

The ability to handle a tool request does not require any tools to be installed or offered. With an empty definition list, a model can return an ordinary answer and the same loop completes without executing tools. This keeps a text-only application within the minimum harness's behavior.

## Minimum Product Boundary

Complete conversations, valid tool continuation and shared provider caching are baseline correctness. Convenience alone does not justify another feature. Mini has one optional instruction file and no skill loader, persona composition, instruction discovery, planner, automatic fallback or compaction. A fixed system instruction and arbitrary round budget do not belong in its core.

### Reference Fixture

The separate harness fixture reuses Mini for contract impersonation and descriptive fake media. Its controls are not read by the installed executable. Tests live under `tests/core`, `tests/runtime` and `tests/link`, while the portable transcript stays in `tests/fixtures/tool-turn.json`. Tests and applications exercise the same underlying loop and runtime.

<!-- docs-tree:start -->

## Docs

<!-- docs-tree:end -->

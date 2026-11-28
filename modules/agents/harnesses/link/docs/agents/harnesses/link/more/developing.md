# Developing

`lnk-harness` is Link Harness, the harness a Link agent runs by
default: a small router in Rust that answers you, keeps the thread, and
calls the tools Link runs for it. How to use it, and why it works the
way it does: [Link Harness](../README.md).

## Its code

This package, `modules/agents/harnesses/link/`, is its code. It speaks Link's adapter contract
(`link_harness_contract::harness`), and Link's release builds it and releases it as
one of Link's modules, at the core's version.

## Execution Evidence

Native conversation events use the shared
[`link-harness-wire` evidence vocabulary](../../../../../../portability/contract/wire/docs/agents/harnesses/more/vocabulary/wire.md#execution-evidence).
An assistant's proposed calls retain original provider IDs separately
from execution references, which use the successfully appended proposal
sequence and call position. A dispatch marker is committed after local
checks and session startup, immediately before a tool request may leave.
Failure to commit it prevents sending. Each result is committed when it
finishes, while model history restores answers to their call order.

### Captured Replies

Matching MCP results preserve structured fields alongside the model's
text. Captured reply JSON is bounded to 1 MiB, with whole-field omission
digests and reasons. Model-text clipping has separate byte counts and a
digest. Binary contents use existing stored part references, and failed
decoding or storage remains explicit. Unrestricted reply metadata is
omitted, and request authorization tokens never enter these records.

### Unknown Outcomes

A matching JSON-RPC error is distinct from a local refusal. Transport
loss after possible dispatch and incomplete work after a crash remain
unknown. Recovery records no invented monotonic duration and never
repeats an uncertain effect. Summary-time proposals and outcomes are
appended even when no final summary survives. A non-error MCP result is
still not proof that a task's postconditions hold.

### Model Attempts

Each selected model attempt records its final canonical request after
fallback selection and system substitution. Text and argument digests,
source event references, and transformed source spans identify included
content without copying prompts or opaque thinking. Source attribution
follows tool answers through pairing, and synthesized unanswered replies
remain synthetic. Media selection is canonical only, since a provider's
conversion can omit a selected image or document.

### Saved Configuration

The private harness configuration uses the shared saved-file channel,
declared beside `Config`, and names schema 1. The shared no-schema policy
reads existing unversioned configuration as the current shape. Missing
read registrations stay unverified. Reads retain the file-identity cache,
and writes retain the atomic private writer with mode 0600. Private
endpoint bindings stay in configuration and never enter execution evidence.

### Skill Delivery

Skill receipts must match the actual returned text at a host-registered
read boundary. That text is the call's `content`, read through its
untrusted-content fence (`link_untrusted::unfenced`): File's is the text
itself, and Memory's is the document's JSON, its `text` field checked and
its other fields matched against the structured content. Neither tool
puts the text in its structured content, which holds the receipt alone.

### Skill Credit

A whole source receipt cannot grant full credit after
model-text clipping. Explicit invocation preserves original input apart
from the expanded body. Source loading, inclusion in a recorded model
attempt, and following the instructions are separate assertions.

## Build and test

From the repository root, with the programs its tests run already built, run `cargo build --locked -p link-harness`, `cargo test --locked -p link-harness`, `cargo test --locked -p link-harness --test harness turns::`, `cargo fmt --check`, then `cargo clippy --all-targets --locked -- -D warnings`.

### Its fake model

Every test's fake model is `tests/common`'s `fake_model`: it keeps each
request and streams the events the test makes of it. A test that orders
a step against an answer holds each answer after its first piece with
`Hold::Until` and lets it go with `LetGo`, so the order is what the test
let go and never how long anything took.

### Conformance and the sandbox

`tests/conformance.rs` runs the conformance kit, the checks every
harness passes. `tests/sandbox.rs` calls Link Memory as a tool. The tests also run the core's `lnk` and Link Memory's `lnk-memory-tool` from the workspace's `target/debug`. [The sandboxed tests skip without bubblewrap](../../../../../../../../../packages/test-kit/docs/more/developing/testing/test-environments.md) unless `LNK_REQUIRE_SANDBOX=1` makes that a failure, as CI sets it.

# Harness Fixture

`lnk-harness-fixture` reuses Mini Harness for adapter and connection tests. It adds descriptive fake media and owns model, tool-discovery, command-advertisement and state-capability controls that the installed Mini executable never reads.

## Code

- [`main.rs`](src/main.rs): fixture controls and the shared ACP entry point

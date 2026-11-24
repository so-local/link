# Measures

`link-measure-contract` holds Local Link's measures. It defines the closed
schema of what a unit uses or counts, and how a measure crosses programs as
OTLP (OpenTelemetry Protocol) JSON. The measure module and the relay both
speak this schema.

<!-- docs-tree:start -->

## Docs

- [Interface](docs/tooling/measure/more/protocol.md): What other programs can rely on from `lnk measure`

<!-- docs-tree:end -->

## Code

- [`client.rs`](src/client.rs): this machine's side
- [`lib.rs`](src/lib.rs): the schema and its OTLP

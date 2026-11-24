# Measure Module

The measure module, `lnk-measure`, shows what each agent and this machine
use, watched from outside them. A watcher keeps the numbers. It can also
send them to an OTLP (OpenTelemetry Protocol) endpoint you name.

<!-- docs-tree:start -->

## Docs

### Pages

- [Measure](docs/tooling/measure/README.md): Measure shows what each of your agents costs you as it happens, including memory, CPU, tokens and spend, and the same for this machine
  - [Measure Guide](docs/tooling/measure/guide.md): Inspect current resource use, retain a history, and send numeric measurements to your observability tools
  - [Files](docs/tooling/measure/files.md): The files `lnk measure` keeps, and what each one holds
  - [Measure Export](docs/tooling/measure/export/README.md): The watcher keeps what it sees on this machine

### References

- [Measure Commands](docs/more/reference/modules/tooling/measure.md): Every command the `measure` module offers, from its declarations, in order of the resource each acts on

### Packages

- [Measures](contract/README.md): `link-measure-contract` holds Local Link's measures

### More

- [Decisions](docs/tooling/measure/decisions.md): Why `lnk measure` works the way it does
- [Security](docs/tooling/measure/security.md): What `lnk measure` reads, keeps and sends, and what it never does
- [Troubleshooting](docs/tooling/measure/troubleshooting.md): What to do when [`lnk measure`](README.md) shows no history, can't watch, or sends nothing

<!-- docs-tree:end -->

## Code

- [`cli.rs`](src/cli.rs): its command line and outputs, generated from `actions.toml` and `types.toml`, with its check hooks in `checks.rs`
- [`export.rs`](src/export.rs): sending measures as OTLP
- [`main.rs`](src/main.rs): `lnk measure`'s commands
- [`sample.rs`](src/sample.rs): one look at this machine
- [`watch.rs`](src/watch.rs): the watcher

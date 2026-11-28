# Cloud Module

The cloud module (`lnk-cloud`) connects your Amazon Web Services and
Google Cloud accounts once, for every part of Local Link that uses them.
`lnk storage` keeps buckets in those accounts, and `lnk box` makes
machines in them.

<!-- docs-tree:start -->

## Docs

### Pages

- [Platforms](docs/services/cloud/README.md): Connect your AWS or Google Cloud account once, and your boxes and buckets there use it
  - [Platforms Guide](docs/services/cloud/guide.md): Connect named cloud accounts, inspect their identities, and disconnect them without deleting their resources

### References

- [Cloud Commands](docs/more/reference/modules/cloud/cloud.md): Every command the `cloud` module offers, from its declarations, in order of the resource each acts on

### Packages

- [Cloud Contract](contract/README.md): The package `link-cloud-contract` defines the requests between the modules that use clouds and a cloud adapter (`lnk-aws`, `lnk-gcp`), and the subprocess client that sends them

### More

- [Interface](docs/services/cloud/more/protocol.md): What other programs can rely on from `lnk cloud`, and where the commands a cloud adapter, `lnk-<kind>`, answers are

<!-- docs-tree:end -->

## Code

- [`cli.rs`](src/cli.rs): the command line
- [`lib.rs`](src/lib.rs): the connected accounts
- [`main.rs`](src/main.rs): the `lnk cloud` commands
- [`module.toml`](module.toml): the module's manifest

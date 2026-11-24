# Cloud Contract

The package `link-cloud-contract` defines the requests between the
modules that use clouds and a cloud adapter (`lnk-aws`, `lnk-gcp`), and
the subprocess client that sends them. The modules `lnk-cloud`, `lnk-box`,
and `lnk-storage` send the requests, and an adapter answers them.

<!-- docs-tree:start -->

## Docs

- [Cloud Adapters](docs/services/cloud/more/adapters.md): The commands a cloud adapter, `lnk-<kind>`, answers: what `lnk-aws` and `lnk-gcp` answer, typed in `link_cloud_contract`, which `lnk-cloud`, `lnk-box`, `lnk-storage` and `lnk-agent` call

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the requests and the client
- [`messages.rs`](src/messages.rs): each command's request and reply
- [`spec.rs`](src/spec.rs): a box's specification, as an adapter takes it
- [`want.rs`](src/want.rs): what a box asks of a cloud

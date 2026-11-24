# Box Contract

The box contract describes the machine a workload addresses, whichever
cloud provider runs it. It holds the captured inputs, the guest access, and
the creation records.

<!-- docs-tree:start -->

## Docs

- [Machine Inputs](docs/services/boxes/more/contract.md): The box contract describes the machine a workload addresses, whichever provider runs it, with its captured inputs, creation records, guest bootstrap and authenticated access

<!-- docs-tree:end -->

## Code

- [`common.rs`](src/common.rs): Lease, intent, guest target and SSH key every provider shares.
- [`forward.rs`](src/forward.rs): A Unix socket on another machine, given by `ssh -R`.
- [`lib.rs`](src/lib.rs): Data-only composition and local adapter messages.
- [`provider.rs`](src/provider.rs): The provider seam: opaque payloads, handles and summaries.

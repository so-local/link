# Untrusted

Untrusted puts one marker around every piece of third-party text that a tool hands a model. The marker is a fence whose closing line carries a random ID.

<!-- docs-tree:start -->

## Docs

- [Untrusted Content](docs/more/libraries/runtime/untrusted.md): `link-untrusted` puts one marker around every piece of third-party text an agent's tool hands its model, so the model can always tell what a page, a file or a program said from what its tool and its owner say

<!-- docs-tree:end -->

## Code

- [`src/lib.rs`](src/lib.rs): The fence.
- [`src/marker.toml`](src/marker.toml): Its words.

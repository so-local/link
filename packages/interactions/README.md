# Interactions

Interactions coordinates questions, approvals, and work controls, and keeps them in one protected store.

<!-- docs-tree:start -->

## Docs

### Pages

- [Interactions](docs/more/libraries/interactions/interactions.md): Link Interactions coordinates questions, bound approvals and owner control intents through one protected store

### Packages

- [Interaction Contract](contract/README.md): Interaction Contract holds the stable references that scheduling and interactions share, with no runtime or storage code

### More

- [Interaction Decisions](docs/more/libraries/interactions/interactions/decisions.md): Interaction coordination shares one typed lifecycle while hosts and resource sources retain execution authority
- [Interaction Security](docs/more/libraries/interactions/interactions/security.md): Interaction authority depends on trusted source and owner ingress adapters around a private machine store

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): public engine interface and contract types
- [`reply.rs`](src/reply.rs): shared targeting and answer grammar
- [`store.rs`](src/store.rs): protected state, atomic transitions and outbox

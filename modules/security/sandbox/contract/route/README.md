# Route Contract

The package `link-sandbox-route` defines a route, a way out of a sandbox
beside the network that the sandbox's proxy mounts under `/_link/` and a
bridge outside the sandbox serves. The package holds the wire protocol,
the bridge's lifecycle, dialing, TLS, and the placeholders a program holds
in place of a credential.

<!-- docs-tree:start -->

## Docs

- [Routes](docs/tooling/sandbox/more/contract/routes.md): `link-sandbox-route`: a route, a way out of a sandbox beside the network, which its proxy mounts under `/_link/` and a bridge outside serves
- [Models and Secrets on the Wire](docs/tooling/sandbox/more/contract/models-and-secrets.md): How a hosted model's key and a service's secret are added to a sandbox's calls outside it, by a route of its proxy's

<!-- docs-tree:end -->

## Code

- [`route.rs`](src/route.rs): a route and its call
- [`route/bridge.rs`](src/route/bridge.rs): a route's bridge
- [`route/placeholders.rs`](src/route/placeholders.rs): credential placeholders

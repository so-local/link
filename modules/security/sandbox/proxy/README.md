# Proxy

The package `link-sandbox-proxy` is the sandbox's one way out, a
filtering proxy that Local Link runs outside the sandbox, with a table of
routes that names none of them. Every call is a line in the proxy's log,
and a flag stops or pauses the proxy's wire. Only `lnk-sandbox` links the
package.

<!-- docs-tree:start -->

## Docs

### Pages

- [Filtering Proxy](docs/tooling/sandbox/more/proxy/README.md): The package `link-sandbox-proxy` is the sandbox's one way out, a filtering proxy that Local Link runs outside the sandbox, with a table of routes that names none of them
  - [Where a Sandbox May Connect](docs/tooling/sandbox/more/proxy/connect.md): What the filtering proxy refuses, and how it reaches the hosts, endpoints and ports a sandbox may
  - [Proxy Protocol](docs/tooling/sandbox/more/proxy/wire.md): What the filtering proxy logs of every call, and how its wire is stopped or paused

### More

- [Decisions](docs/tooling/sandbox/more/proxy/decisions.md): Why the sandbox's filtering proxy, `link-sandbox-proxy`, works the way it does

<!-- docs-tree:end -->

## Code

- [`filter.rs`](src/filter.rs): its rules
- [`lib.rs`](src/lib.rs): the proxy
- [`routes.rs`](src/routes.rs): its routes
- [`wire/mod.rs`](src/wire/mod.rs): the wire, its log and flags

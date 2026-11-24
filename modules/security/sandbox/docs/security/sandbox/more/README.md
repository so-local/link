# More

> Understand Sandbox Boundaries and Workers

Every side that builds or calls a sandbox shares three boundaries. The
[Sandbox Contract](../../../../contract/docs/security/sandbox/more/contract.md)
defines the shared inputs. [Bridges](../../../../bridge/docs/security/sandbox/more/bridges.md)
explain how callers supply identity and grants. The
[Filtering Proxy](../../../../proxy/docs/security/sandbox/more/proxy/README.md)
covers the HTTP boundary that approved requests cross.

## Runtime Maintenance

Read [Sandboxed SSH Sessions](sessions.md) for session state and
[Worker Controller Maintenance](workers.md) for worker supervision. Both
help when you diagnose or change those implementation paths.

<!-- docs-tree:start -->

## Docs

- [Sandboxed SSH Sessions](sessions.md): On a box, a person gets a Linux user of their own, whose every SSH session is in their sandbox: A box's users
- [Worker Controller Maintenance](workers.md): The worker installer maintains an ordinary Linux box's privileged launcher and controller identity

<!-- docs-tree:end -->

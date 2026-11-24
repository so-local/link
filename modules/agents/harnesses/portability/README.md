# Portability

Portability is what lets an agent move between harnesses. It has four parts: the rules every adapter answers, the Agent State Protocol (ASP) that carries an agent's state, the hub that serves a harness to Local Link's bridges, and the kit that checks an adapter's conformance.

<!-- docs-tree:start -->

## Docs

- [Agent State Protocol](agent-state-protocol/README.md): `link-agent-state-protocol` implements the Agent State Protocol (ASP)
- [Harness Conformance](conformance/README.md): `link-harness-conformance` is the kit that checks a harness program answers the harness commands, by running the program as `lnk-agent` would
- [Harness Contract](contract/README.md): `link-harness-contract` sets the rules between the agent module (`lnk-agent`) and a harness program
- [Harness Hub](hub/README.md): `link-harness-hub` serves a harness's Agent Client Protocol (ACP) over stdio to Local Link's bridges as ACP over a WebSocket, with each thread a session kept across restarts

<!-- docs-tree:end -->

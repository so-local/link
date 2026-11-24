# Test Fixtures

Test Fixtures holds executable examples and shared setups for Local Link's tests. Start with
[Mini Harness](../../modules/agents/harnesses/mini/README.md) to follow a
model turn, the connection fixture for message delivery, or the host
fixture for agent moves. Each fixture's Code links give a reading path,
with test controls explained beside their handlers.

<!-- docs-tree:start -->

## Docs

- [Connection Fixture](connection/README.md): `lnk-connection-fixture` demonstrates how a connection carries messages, as the connection contract describes, against a test's fake bot service
- [Declarations Fixture](declarations/README.md): `link-declarations-fixture` compiles a small module's declarations through Actions Build, so tests run the generated types, command line and dispatch end to end
- [Environment Fixture](env/README.md): `link-env-fixture` gives environment and job tests a folder-backed cloud, fake programs and inspectable results
- [Harness Fixture](harness/README.md): `lnk-harness-fixture` reuses Mini Harness for adapter and connection tests
- [Host Fixture](agent-place/README.md): `lnk-agent-place-fixture` demonstrates how a host serves agents, as the hosted-agent contract describes, using inspectable files instead of running agents
- [Webhook Fixture](webhook/README.md): `lnk-webhook-fixture` demonstrates a pushed connection's signature check and accept-or-drop verdict

<!-- docs-tree:end -->

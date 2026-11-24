# Webhook Fixture

`lnk-webhook-fixture` demonstrates a [pushed connection](../../../modules/agents/connections/contract/src/connection/pushed.rs)'s signature check and
accept-or-drop verdict. Tests run it as the webhook module to check
posts signed by Local Link and by GitHub. It is never shipped.

## Code

- [`main.rs`](src/main.rs): the line protocol, sender settings, inbound verification and verdict

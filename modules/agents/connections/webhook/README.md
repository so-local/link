# Webhooks for Link

The webhook module, `lnk-webhook-connection`, checks each post to an agent's own webhooks (`lnk agent webhook`). It verifies the signature of the sender, Link or GitHub, before the post reaches the agent.

<!-- docs-tree:start -->

## Docs

### Pages

- [Webhook Module](docs/agents/connections/webhook/README.md): The webhook module checks each post to your agent's webhooks
  - [Webhook Module Guide](docs/agents/connections/webhook/guide.md): Configure the webhook validator before another application posts events to your agent

### More

- [Validator Protocol](docs/agents/connections/webhook/more/protocol.md): The webhook validator receives signed posts from the agent bridge and returns an acceptance verdict
- [Security](docs/agents/connections/webhook/security.md): How a post to one of your agent's webhooks is checked, beyond what every webhook's secret gets, which Link's Credentials page covers

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the module and its checks
- [`main.rs`](src/main.rs): `lnk-webhook-connection sift`
- [`module.toml`](module.toml): the module's manifest

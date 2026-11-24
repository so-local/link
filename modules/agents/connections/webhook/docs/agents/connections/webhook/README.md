# Webhook Module

> Webhooks Let Your Apps Nudge Your Agent

The webhook module checks each post to your agent's webhooks. `lnk-webhook-connection` is the
module `lnk agent webhook` needs. Your agent's bridge hands the module [each
post](../../../../../../agent/docs/agents/connections/connect/webhook/README.md), and only a post the module accepts reaches your agent.

<!-- quickstart -->

## Add the Webhook Module

Install the module that validates signed posts before they reach your agent. [Webhooks](../../../../../../agent/docs/agents/connections/connect/webhook/README.md) explains the sender and signature requirements.

```sh
lnk mod add webhook-connection
```

## Read How Posts Are Validated

The [guide](guide.md) explains which posts the module accepts. The [validator protocol](more/protocol.md) defines the process boundary. [Security](security.md) describes signature checks, replay protection, and secret rotation.

<!-- docs-tree:start -->

## Docs

### Pages

- [Webhook Module Guide](guide.md): Configure the webhook validator before another application posts events to your agent

### More

- [Validator Protocol](more/protocol.md): The webhook validator receives signed posts from the agent bridge and returns an acceptance verdict
- [Security](security.md): How a post to one of your agent's webhooks is checked, beyond what every webhook's secret gets, which Link's Credentials page covers

<!-- docs-tree:end -->

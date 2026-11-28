# Troubleshooting

What to do when [DeepSeek Harness](README.md) doesn't install, start or answer. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## "the Node Link downloaded isn't the one it pins"

A bad download: the Node it fetched doesn't match the SHA-256 in its
source. Nothing ran, so try again later.

## `status` says it isn't answering

[`lnk agent log show --follow`](../../../../../../../../modules/agents/agent/docs/agents/start/log/README.md#read-the-log) has `dsh`'s own errors, which say why it
doesn't answer its endpoint.

## "the harness did not initialize within 120 s"

`dsh` hangs at its start, and `lnk agent log show --follow` has its errors.
The next check or message starts it again.

## Something else uses port 18820

Stop it, because `main`'s port is fixed. Another agent's is in its own
block, which [`lnk agent list --json`](../../../../../../../../modules/agents/agent/docs/agents/start/show.md#list-your-agents) shows.

## It answers without web search

Expected, because its search goes through DeepSeek's API, which Link
turns off. Its `web_fetch` works.

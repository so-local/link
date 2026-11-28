# Security

How model credentials remain outside the agent and how hosting accounts supply managed models.

## Models supplied by a host

A hosting account answering `managed_models` receives no model or
fallback setting in a move's switch. The source keeps a private
checkpoint of model metadata under `~/.lnk/safe/agents/transfers/moved/<id>.models`. The keys
stay in the secret keeper without a running-agent grant. Removing a key
with [`lnk secret delete`](../../../../../../security/secret/docs/config/secrets/README.md#an-agent-that-leaves) also removes that retained value. Link restores
the remaining settings only for the same agent ID returning to this
machine, then removes the checkpoint after the agent saves its credentials.

## Keys and models

Where an agent's keys are kept, and what a local model reaches.

### Secrets

Where the model provider credential and each connection's token are kept.

- The model provider credential and the bot token are kept with your [secrets](../../../../../../security/secret/docs/config/secrets/README.md):
- `~/.lnk/safe/secrets/secrets.toml` (mode 600, or the
  [Keychain](../../../../../../../docs/more/architecture/security.md#secrets-in-the-keychain)
  on a Mac), each under an ID of its own, with no name, kept and
  granted by the agent's ID and its use (its model's key, its Telegram
  bot's token), so an agent made later with its name has none of them.
- The harness's own `.env` in its home holds them too where the harness
  needs it (mode 600). A key still in the agent's settings, or in the
  Keychain as `agent/<agent>/…`, where Link kept it before, is never
  read and never removed: the agent's start names it and how to give it
  again.
- **A secret you grant the agent** (`lnk secret create <NAME>
  --internet-host <host>`, then `lnk secret add <NAME> --to agent:<agent>`) is sealed: its proxy adds it to requests
  for that host alone, and the harness is told only a placeholder,
  `$<NAME>`, and its service's root on the proxy,
  `$LNK_SEALED_<NAME>_URL`.
- One kept plain is never given to an agent, and an agent whose harness
  runs without a proxy of its own (no sandbox, or a harness shared with
  your other agents) doesn't start with one.
- They reach the adapter on stdin, never on a command line or in the
  background service's definition: the LaunchAgent plist is readable by
  other local users. The harness's `.env` stays a file.
- For a harness Link carries connections for (Link Harness), the bot token
  never reaches the adapter, nor the harness's proxy: `lnk-agent`
  hands it to `lnk-<connection> bridge` on stdin, and the bridge service
  (`lnk agent bridge`) reads it from the agent's settings itself, never
  from its definition.
- With keys on the wire (the default for Link Harness and for a harness
  whose adapter says it takes the proxy's address, or `lnk agent secret
  wire`; behind the proxy), the harness's model settings get a
  placeholder instead of a hosted model's key, and the proxy adds the
  real one outside the sandbox.
- A model at an HTTPS address of its own, the one a module declares for
  its provider or a `base_url`, never takes the provider route, which would send its key
  to OpenAI or another provider. It has a route of its own on the proxy,
  its fallback another, and the harness gets the proxy's address for it
  and a placeholder.
- The proxy adds that key, the agent's own model or fallback key, for
  that one host alone, as the API its provider speaks there, with the
  same filters as a provider's own below.
- An address the proxy can't hold a key to (plain HTTP, a port of its
  own, or an IP address) stops the agent's start rather than send its
  key elsewhere. So does an address speaking OpenAI's API whose path
  doesn't end with its `/v1`, where the proxy couldn't tell inference.
- Link Harness keeps that placeholder in its home's `config.json`, and
  another harness in its `.env`. A command run in the harness's sandbox
  (`lnk agent harness run`) gets the proxy's address, not a key.
- The same goes for a Telegram bot's token: the harness gets the bot's
  id and `:link-wire-` with a tag that's new at each start and its
  agent's alone, never made from the token, and reaches Telegram through
  its proxy, [which adds the token](../../../../../../security/sandbox/contract/route/docs/tooling/sandbox/more/contract/models-and-secrets.md#a-services-secret-added-by-the-proxy) and refuses `setWebhook`, `logOut` and
  `close`, by the rule the Telegram module gives (`lnk-telegram rule
  --json`).
- An agent doesn't start while a connected connection's module can't say
  whether it has such a rule, so its secret never reaches the harness by
  mistake. `lnk agent harness check` reports `LEAK` if the key or the
  token is anywhere in the harness's home. Slack's, Discord's and
  Signal's stay with a harness that speaks them.
- The key proxy passes inference only, at a provider's own address or
  one of its own: messages, completions, embeddings, a voice note's
  transcription and the models' list; never files, batches or the
  account.
- The provider's own tools that run on its servers (web search, web
  fetch, code execution, MCP servers) are removed from each request, and
  a request asking the provider to fetch a URL, such as an image or a
  document, is refused: they would reach the internet for the agent.

### A local model

`lnk agent start` offers the models [`lnk model list`](../../../../../model/docs/agents/models/README.md#list) finds, or on an
Apple Silicon Mac with none, to install the one [`lnk model list --fit`](../../../../../model/docs/agents/models/local.md#install-a-model)
picks. A local model needs no key: the harness talks to the runtime on
localhost, and nothing leaves the machine for the model. Behind its
proxy, a local model is reached for inference only: its admin API, which
pulls and deletes models, gets a 403.

# Decisions

Why an agent selects a provider and default model this way.

## Which model does an agent use with no `--model.default`?

Link's default for the provider, named, in every harness:
`claude-haiku-5-5` on Anthropic, `gpt-6-luna` on OpenAI and
`anthropic/claude-haiku-5.5` on OpenRouter. Each harness's own default
differs (OpenClaw's for OpenAI costs many times Link Harness's), and
would leave `status` saying only "the harness's default". A named model
says what it costs, and stays the same when you switch harness.

### One price

The three are one price, $0.10 a million tokens in and $0.50 out, so
switching provider never multiplies the bill without your saying so. A
mid-size model such as Claude Sonnet costs twenty times that, and you're
paying, so the default is the cheap one and `--model.default` names a larger
one. OpenRouter's own `openrouter/auto` isn't the default: it sends each
prompt to a model of its choosing and bills that model's price, which is
often a large one.

### A hosted key

A start doesn't check a hosted key: that takes a call to the provider,
and `start` and `status` look only at this machine. They say the agent
is `started` and `running`, which is its harness answering; a key the
provider refuses shows at the first message, as "Your agent couldn't
answer" on your phone and the provider's error in `lnk agent log show`.

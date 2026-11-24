# Models

Mini asks [Models](../../../../../../../../model/docs/agents/models/README.md) for an answer in one typed shape. That layer translates Mini's instruction, messages and tool definitions into provider requests, then decodes streamed responses into text, calls and continuation data. Provider protocols and cache controls stay there, so Mini's loop works with the shared result rather than a provider's wire format.

## Ask the Model

[`Request::ask_model`](../../../../../../src/link/model.rs) builds a typed `Prompt` from the instruction, complete conversation and stable tool definitions. An installed agent sends it to the granted `model/chat` endpoint. Standalone fixtures can use the shared model client's native provider mapping instead. Either route returns assistant text, tool calls and any continuation blocks to the same core.

## Model Limits

The adapter fills the window the agent's specification sets, `[window] max`, which is also what Link's sandbox lets a request ask for: Link's own 128,000 held to the model's window, or the value you set. A specification without one uses Link's known context window, falling back to 128,000 tokens from [`defaults.toml`](../../../../../../src/link/defaults.toml) when that fact is absent. Its answer budget is at most 4,096 tokens, lowered by the model's known answer limit and context window. This limit applies to each model answer and does not cap the number of tool rounds.


## Context Exhaustion

Mini resends the complete conversation rather than trimming or summarizing it. A conversation that no longer fits the model's window ends in an error. Mini has no automatic model fallback, retry, context compaction or spending policy.

## Preserve Prompt Caching

The [shared model client](../../../../../../../../model/contract/src/chat/client/bodies.rs) owns provider request bodies and cache controls. Mini preserves earlier messages and tool results exactly and orders tool definitions deterministically. Changing an instruction or offered tool can change that reusable prefix. Provider cache expiry, minimum prompt size and supported models still determine whether a request gets a cache hit.

## Anthropic

The shared client marks the system instruction when present and the last message block with Anthropic's `cache_control`. Marking the last block includes earlier messages in the reusable prefix. Mini adds no second provider policy.


## Cache Lifetime

Anthropic's default cache lifetime is five minutes and reuse refreshes it. A one-hour cache is an explicit provider option with a higher write price, which Mini does not request. See [Anthropic prompt caching](https://platform.claude.com/docs/build-with-claude/prompt-caching) for eligibility and pricing.

## OpenAI

OpenAI prompt caching is automatic for supported models and matching prefixes. Mini supplies the saved conversation through the shared interface, whose native OpenAI route uses Chat Completions. `previous_response_id` belongs to Responses API conversation continuation and is not needed for this caching. See [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching) and [conversation state](https://developers.openai.com/api/docs/guides/conversation-state).

## Read Usage

The model bridge reports provider usage through Link's normal model interface. Mini does not maintain a competing estimate or promise a particular cache-hit rate. Cached input still has a price, and a growing conversation can cost more or exceed its model's context window.

<!-- docs-tree:start -->

## Docs

<!-- docs-tree:end -->

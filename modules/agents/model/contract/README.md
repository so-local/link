# Model Contract

`link-model-contract` holds what Local Link knows of models (each one's window, its longest answer, how it thinks, and what it costs), included as data, and the model routes a program behind its proxy calls.

<!-- docs-tree:start -->

## Docs

- [Model Contract](docs/agents/models/more/contract/README.md): `link-model-contract` shares model facts, request types, and context reports between model callers and the bridge
  - [Model Caller Types](docs/agents/models/more/contract/chat.md): Share typed prompts, tool results, and completed responses between a harness and the model bridge
  - [Context Caller Types](docs/agents/models/more/contract/context.md): Attribute a model request to stable prompt parts without saving its text

<!-- docs-tree:end -->

## Code

- [`api.rs`](src/api.rs): model action request and response types
- [`chat.rs`](src/chat.rs): prompts, messages, and provider responses
- [`context.rs`](src/context.rs): prompt parts and context reports
- [`models.rs`](src/models.rs): model facts
- [`models/route.rs`](src/models/route.rs): the model routes

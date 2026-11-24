# Models

The models module, `lnk model`, finds the model runtimes running on this machine, installs Ollama and its models, serves a runtime's OpenAI-compatible API, and bridges the model routes out of a sandbox.

<!-- docs-tree:start -->

## Docs

### Pages

- [Models](docs/agents/models/README.md): Run a model on your computer for free, or use Anthropic, OpenAI or OpenRouter with your own credential
  - [Models Guide](docs/agents/models/guide.md): Install or discover a local model, choose a hosted provider, and inspect the model your agent uses
  - [On Your Computer](docs/agents/models/local.md): Run a model through a runtime on your computer, without a Link inference fee
  - [Model Runtimes](docs/agents/models/runtime/README.md): `lnk model runtime` manages the local inference servers Local Link discovers
  - [Context Reporting](docs/agents/models/context.md): See what your agent's context is spent on, part by part, for every harness: its instructions, each tool, and the conversation down to each tool's calls and results
  - [Model Provider](docs/agents/models/providers.md): Use Anthropic, OpenAI, OpenRouter, or a provider an installed module adds, with your own credential

### References

- [Model Commands](docs/more/reference/modules/agents/model.md): Every command the `model` module offers, from its declarations, in order of the resource each acts on

### Packages

- [Model Contract](contract/README.md): `link-model-contract` holds what Local Link knows of models (each one's window, its longest answer, how it thinks, and what it costs), included as data, and the model routes a program behind its proxy calls

### More

- [Model Internals](docs/agents/models/more/README.md): Inspect model runtime integration, stored configuration, and the shared caller boundary
- [Security](docs/agents/models/security.md): The model module limits what callers can reach through a served runtime and what discovery exposes about this machine
- [Troubleshooting](docs/agents/models/troubleshooting.md): What to do when `lnk model` finds no model, or finds more than one

<!-- docs-tree:end -->

## Code

- [`bridge/mod.rs`](src/bridge/mod.rs): the model routes' bridge
- [`discover.rs`](src/discover.rs): finding running runtimes
- [`externals.json`](externals.json): the programs it installs
- [`main.rs`](src/main.rs): `lnk model`'s commands
- [`serve.rs`](src/serve.rs): `lnk model serve`

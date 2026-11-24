# Agents

Agents holds everything that runs an agent in Local Link. It has the agent module, plus the harnesses, models, connections, tools, and stores the agent uses.

<!-- docs-tree:start -->

## Docs

### Modules

- [Agent](agent/README.md): The `lnk agent` module starts, uses, connects, and moves an agent
- [Models](model/README.md): The models module, `lnk model`, finds the model runtimes running on this machine, installs Ollama and its models, serves a runtime's OpenAI-compatible API, and bridges the model routes out of a sandbox

### Folders

- [Connections](connections/README.md): Connections are how you reach your agent
- [Harnesses](harnesses/README.md): Harnesses are the programs that run your agents
- [Stores](stores/README.md): Stores hold the collections an agent keeps, through one store contract and a driver for each place
- [Tools](tools/README.md): Tools let your agent do more than talk

<!-- docs-tree:end -->

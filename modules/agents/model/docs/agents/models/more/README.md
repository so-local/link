# Model Internals

> Model Runtime Contracts and Settings

Inspect model runtime integration, stored configuration, and the shared
caller boundary. User installation and model selection stay in the
[model guide](../guide.md).

## Implement a Caller

Typed inference requests are defined in [Model Actions](api.md),
sandbox-visible metadata in [Model Details](details.md), and structured
command output in the [program interface](protocol.md). Request types, model
facts, and context measurements are explained in the
[shared contract](../../../../contract/docs/agents/models/more/contract/README.md).

## Configure the Runtime Boundary

Persisted files and endpoint inputs are named in
[Files](files.md). The discovery and runtime choices behind
the interfaces are explained in [Decisions](decisions.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Files](files.md): The files `lnk model` keeps, and what each one holds
- [Model Actions](api.md): An agent's model has one API, whatever serves it
- [Model Details](details.md): A program in a sandbox asks a model's details in one shape, whatever runs the model

### More

- [Interface](protocol.md): What other programs can rely on from `lnk model`
- [Decisions](decisions.md): Why the models module works the way it does

<!-- docs-tree:end -->

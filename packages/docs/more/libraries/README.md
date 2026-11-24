# Shared Libraries

> Import Each Capability from Its Owner

Choose the library that supplies the behavior your caller needs. These
references explain public entry points, their guarantees, and the code
to read next. Each package keeps its own documentation and implementation.

## Types and Boundaries

Component declarations, typed configuration, execution destinations, and
stable identities connect in [Types and Boundaries](types/README.md). Start
there when defining an interface shared by independently built programs.

## Files and Secrets

Protected filesystem access, settings, immutable artifacts, content identity,
and secret handling are in [Files and Secrets](files/README.md). The references
distinguish safe access from the policy deciding who gets it.

## Runtime and Communication

Process lifetimes, service protocols, HTTP clients, and output are in
[Runtime and Communication](runtime/README.md). Follow those
entry points when connecting a command to a long-lived or remote operation.

## Questions and Effects

Libraries that coordinate questions, saved decisions, and authorized
changes are in [Questions and Effects](interactions/README.md). The contracts
keep interaction separate from the operation under control.

<!-- docs-tree:start -->

## Docs

- [Files and Secrets](files/README.md): Files supplies filesystem and location primitives
- [Questions and Effects](interactions/README.md): Interactions coordinates questions, answers, and their effects through its shared contract
- [Runtime and Communication](runtime/README.md): Runtime provides an asynchronous runtime to synchronous entry points
- [Types and Boundaries](types/README.md): Build an interface from typed declarations before adding callers

<!-- docs-tree:end -->

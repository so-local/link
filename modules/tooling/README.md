# Tooling

Tooling holds the modules that run around your agents. They measure what
each one costs, read the manual offline, move their state, and build Local
Link's programs.

<!-- docs-tree:start -->

## Docs

### Pages

- [Tooling](docs/tooling/README.md): Tooling gives you tools that run around your agents
  - [Tooling Overview](docs/tooling/guide.md): What Link runs around your agents: what each agent costs, and these docs offline

### Modules

- [Archive Module](archive/README.md): The archive module, `lnk archive`, exports your things' state to one `.tar.gz` file and imports one
- [Bundle Module](bundle/README.md): The bundle module, `lnk bundle`, builds any set of Local Link's programs from source for their targets
- [Manual Module](manual/README.md): The manual module, `lnk man`, reads the documentation and the command reference of installed components offline
- [Measure Module](measure/README.md): The measure module, `lnk-measure`, shows what each agent and this machine use, watched from outside them
- [Specification Module](spec/README.md): The specification module, `lnk spec`, lists, shows and converts the specification kinds that every installed module exports

<!-- docs-tree:end -->

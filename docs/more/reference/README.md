# Reference

> Shared Files, Commands, and Runtime Contracts

Find the inputs and interfaces that apply across Local Link's programs.
Each feature's guide explains the feature's own tasks and links here for shared rules.

## Find a Command

The [command index](commands.md) lists the commands declared by this build.
A command's help and owning guide explain arguments and behavior.
Learn how installed programs are found and how their declarations reach the
command line in [Link Modules](../modules/README.md). See each command a module
declares, with usage, arguments, and answer, in [Module Commands](modules.md).
Let bash and zsh finish those commands and their flags on Tab with
[Shell Completion](../../../core/cli/docs/more/reference/completion.md).

## Configure a Program

Reusable TOML settings and field flags are explained in
[Specifications](../../../packages/spec/docs/more/reference/specs.md), and the
[specification catalogue](../../config/specs/README.md) links to the complete
reference for each kind. Find declared inputs, without their current values, in
[Environment Variables](environment.md). [Variables](variables.md) lists the
ones a module's declarations define.

## Inspect Files and Declarations

[Locations](locations.md) distinguishes protected configuration, installed
programs, user data, and runtime sockets. The declarations and generated files
that maintain those interfaces are in [Repository Files](files.md).
Contributor setup and checks are in [Developing](../developing/README.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Environment Variables](environment.md): Find the environment variables declared by Link's commands and libraries, including internal and test overrides
- [Where Link Keeps Things](locations.md): Local Link keeps what's yours in `~/Link` and what it needs in `~/.lnk`
- [Repository Files](files.md): What each recurring file in the repository is, who reads it and who edits it, so a contributor can tell a declaration from generated data
- [Command Index](commands.md): Find the commands declared by the core and registered modules

### References

- [Variables](variables.md): Every environment variable the workspace's crates define, modules, packages and contracts alike, from their `variables.toml`, by the crate defining it, and each namespace a crate computes names in
- [Module Commands](modules.md): Each module's commands, from its declarations, with their usage, runtime arguments, specifications, and answers, on its own page or on its domain's where a domain has several: Agents, Box, Cloud, Core, Env, Sandbox, Secret, Storage, and…
  - [Tooling Modules](modules/tooling.md): The commands of each tooling module, from its declarations, with their usage, runtime arguments, specifications, and answers: Archive, Bundle, Manual, Measure, and Specification
  - [Agents Modules](modules/agents.md): The commands of each agents module, from its declarations, with their usage, runtime arguments, specifications, and answers: Agent, MCP, and Model

<!-- docs-tree:end -->

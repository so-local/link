# Agent Specifications

An agent's opinions are a specification. The specification meets Local Link's, the agent's teams', and the agent's own opinions as
sets. `lnk agent spec show` prints the specification an agent runs, with each key marked
by whose it is. The [Agent Specification](../../../config/specs/agent.md) lists every field.

## Show an Agent's Specification

Name the agent, or leave the name out where there's only one. Each key ends
with a comment saying whose it is: Local Link's, a profile's, a team's, or
yours.

```sh
lnk agent spec show main
```

With `--json`, the command prints the specification alone, as a file holds it, without
saying whose each key is. An agent and a file are two specifications, so the command refuses a request that names both.

## Show a File's Specification

Give a specification file, field flags, or both, and the command prints the specification they make
with every default written in, as `lnk agent start --spec` would read it.
Nothing is started or changed.

```sh
lnk agent spec show --spec agent.toml --model.thinking high
```

## Start With a Specification

`lnk agent start` and `lnk agent create` take the same file and flags.
Without a file, the flags change only their keys of the agent's own specification.
The [Agent Specification Guide](../../../../contract/spec/docs/config/specs/guides/agent.md)
explains how the specifications meet.

```sh
lnk agent start --spec agent.toml --turns.rounds 12
```

## Show a Team's Specification

A team's file is a specification of its own, applied with `lnk agent team create
--spec`. The [Team Specification](../../../config/specs/team.md) lists every field.
Name a team to print its file, or give a file. A team specification's field flags
go over its file. Without `--spec`, `lnk agent team create` refuses
the flags.

```sh
lnk agent team spec show research
lnk agent team spec show --spec team.toml
```

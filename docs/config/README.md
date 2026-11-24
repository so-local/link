# Configuration

> Specifications and Settings You Can Inspect

**Configuration is the TOML you write to tell Local Link what you want,
and it comes in two kinds: specifications and settings.** You can read
it, inspect it and keep it in git, so you get the same agent, machine,
or sandbox every time.

## Specifications and Settings

A specification describes one thing, such as an agent, a box or a
sandbox. You hand it to the command that makes or runs that thing, and
its shape is declared in a checked reference. Settings are the standing
choices that aren't a specification: yours, a machine's, or a module's,
which no one thing owns. Each setting is one value in a `settings.toml`
or a module's settings file, read with `get` and written with `set`.
Configuration means both together, and nothing more.

## Start an Agent from a File

Write `agent.toml` using the [Agent Specification](../../modules/agents/agent/docs/config/specs/agent.md#use-it),
then start the agent from that specification.

```sh
lnk agent start --spec agent.toml
```

## Create a Box from a File

Write `web.toml` using the [Box Specification Guide](../../modules/compute/box/docs/config/specs/guides/box.md),
then create that machine in your connected cloud account.

```sh
lnk box create aws|gcp --spec web.toml
```

## Run a Program from a File

Choose the project permissions in `app.toml` using the
[Sandbox Specification Guide](../../modules/security/sandbox/docs/config/specs/guides/sandbox.md),
then run the project's npm tests with those permissions.

```sh
lnk sandbox run --spec app.toml -- npm test
```

## Find the Specification for a Resource

Find the [reference](specs/README.md) for an agent, team, box, environment,
sandbox, or policy. The references also explain the TOML format and the
defaults those files share. Every specification is held to [its
cases](../../tests/specs/README.md), each a specification and what must come of it.

## Where Control Files Live

Every setting, specification, and grant that steers Link is a control file, kept
in a [Safe](../../modules/storage/safe/docs/config/safe/README.md) that
no run reaches, so nothing a run does can change what steers it. That
page explains what a Safe holds and how its files are listed, read, and
written.

## Where an Agent's Things Are Kept

An agent's specification says where it would keep its things, and the
machine's policy says where things may be kept. The two meet through the
[agent settings](../../modules/agents/agent/docs/agents/more/settings/README.md).
That guide explains supported destinations and machine overrides, while
[specification composition](../../modules/agents/agent/contract/spec/docs/config/specs/guides/agent.md#specifications-meet-as-sets)
explains how an agent's and its team's specifications combine.

## Files You Can Edit

Your configuration is files you can edit in place. This installation
also has a stable ID and name. [Installation Identity](../../packages/machine/docs/config/installation.md)
describes both and shows how to inspect the identity file.

### Agent, Team, and Model Files

A few more files are yours to edit. Each agent keeps setup and behavior
in `~/Link/Safe/Agents/<name>/agent.toml`, and [permissions](../../modules/agents/agent/docs/security/sandbox/permissions/README.md) in
`~/Link/Safe/Agents/<name>/sandbox.toml`, whichever harness runs the agent.
[A team keeps the opinions its agents share](../../modules/agents/agent/contract/spec/docs/config/specs/guides/agent.md#where-its-kept) in `~/Link/Safe/Teams/<team>.toml`.
[You describe models](../../modules/agents/model/docs/agents/models/providers.md#what-link-knows-of-a-model), or correct Local Link's figures, in
`~/.lnk/safe/models/facts.toml`.

### `~/.lnk/safe/agents/policy.toml`

[The machine's own rules](../../modules/agents/agent/docs/agents/more/settings/README.md) say, for example, never to run an agent unsandboxed,
and where agents' things may be kept.

### Every Other File

Local Link keeps every other file under `~/.lnk`. [Where Link Keeps
Things](../more/reference/locations.md) explains the layout and lists
every location, and each file is on its module's Files page. [`lnk variable list <command>`](../../core/cli/docs/more/architecture/files.md#list-a-commands-variables) lists the
environment variables a command reads.

## Secrets and Credentials

A specification holds no secret, and the command names the accounts and
secrets it uses. A secret is any value kept private, and Local Link keeps
your [secrets](../../modules/security/secret/docs/config/secrets/README.md)
on this computer and gives them only where you grant them. A credential
is a secret that authorizes, and the
[Credentials](../../modules/security/docs/config/credentials/README.md)
page lists every credential Local Link mints, holds, or checks, and whom
each authorizes.

## Shared Specification Rules

Specification files and flags share rules for inputs, paths, inspection, and explicit
conversion. [Specifications](../../packages/spec/docs/more/reference/specs.md) explains them.

<!-- docs-tree:start -->

## Docs

- [Specifications](specs/README.md): Choose a complete TOML format for an agent, team or cloud resource

<!-- docs-tree:end -->

# Overview

Install Local Link, start an agent, and choose the services and tools the agent needs. Run the agent on your computer or in your own clouds.

```sh
curl -fsSL https://local.link/install.sh | sh
lnk up
lnk agent start
lnk agent chat
```

## Agents

Make any agent you can imagine, share it with your community, and start the agents others share. Start your first agent, then choose a harness and model, connect a chat app, and add tools and stores. [Get Started with Agents](../modules/agents/agent/docs/agents/README.md) walks through each step. If you already have an OpenClaw, Hermes or DeepSeek agent, [bring it under Local Link](../modules/agents/agent/docs/agents/harnesses/use/migrate/README.md).

- [Start Your Agent](../modules/agents/agent/docs/agents/start/README.md): make your first agent and chat with it
- [Share Agents](../modules/agents/agent/docs/agents/share/README.md): share an agent you made, and start the ones your friends make
- [Ready-Made Agents](../modules/agents/agent/docs/agents/ready-made/README.md): start from one of four agents Local Link made
- [Switch Harnesses](../modules/agents/agent/docs/agents/harnesses/use/README.md): run your agent on Link Harness, OpenClaw, Hermes, or DeepSeek
- [Switch Models](../modules/agents/agent/docs/agents/models/agent/README.md): pick the model your agent thinks with
- [Connect Messaging Apps](../modules/agents/agent/docs/agents/connections/connect/README.md): reach your agent from Slack, Discord, Telegram, or Signal
- [Move Your Agent](../modules/agents/agent/docs/agents/move/README.md): take your agent between your computer and your cloud

## Cloud

Run machines and keep files in your own cloud accounts. [Connect an account](../modules/cloud/cloud/docs/services/cloud/README.md) first. Then create boxes to run agents and buckets to store files. The [Cloud](services/README.md) section covers each step.

- [Cloud Accounts](../modules/cloud/cloud/docs/services/cloud/README.md): add your Amazon Web Services or Google Cloud account
- [Boxes](../modules/compute/box/docs/services/boxes/README.md): run programs on a machine in your cloud
- [Environments](../modules/compute/env/docs/services/environments/README.md): describe the setup a box runs with
- [Storage](../modules/storage/storage/docs/services/storage/README.md): back up your folders and get them back

## Security

Local Link keeps every program it runs to what you allow. The [Sandbox](../modules/security/sandbox/docs/security/sandbox/README.md) runs a program so the program changes only the folders you give it and reaches only what you allow. Every agent runs in a sandbox, and so can any other program. The [Security](../modules/security/docs/security/README.md) section covers the checks.

- [Sandbox](../modules/security/sandbox/docs/security/sandbox/README.md): run any program with only the access you give it
- [Permissions](../modules/agents/agent/docs/security/sandbox/permissions/README.md): choose the folders and hosts an agent can reach
- [Where Link Keeps Things](more/reference/locations.md): see what Local Link keeps where, and which folders no agent can read
- [Security](../modules/security/docs/security/README.md): check that the limits hold

## Tooling

Measure [what each agent costs](../modules/tooling/measure/docs/tooling/measure/README.md), read the offline manual, make archives, and build Local Link's programs. The [Tooling](../modules/tooling/docs/tooling/README.md) section covers them all.

- [Measure](../modules/tooling/measure/docs/tooling/measure/README.md): see what each agent costs
- [Archives](../modules/tooling/archive/docs/tooling/archive/README.md): keep or share a copy of an agent
- [Manual](../modules/tooling/manual/docs/tooling/manual/README.md): read these docs offline with [`lnk man`](../modules/tooling/manual/docs/tooling/manual/README.md#manual)
- [Bundle](../modules/tooling/bundle/docs/tooling/bundle/README.md): build Local Link's programs yourself

## Configuration

Configuration is specifications and settings together. Use [specifications](config/specs/README.md) to describe an agent, a box, or a sandbox you can make again, and [settings](config/README.md#specifications-and-settings) for the standing choices that are yours, this machine's, or a module's. The [Configuration](config/README.md) section also covers secrets and the credentials among them.

- [Specifications](config/specs/README.md): describe an agent, a box, or a sandbox in a TOML file
- [Installation Identity](../packages/machine/docs/config/installation.md): who Local Link is on this machine, under your user
- [Configuration](config/README.md): specifications, settings, secrets, and credentials

## More

See how the parts of Local Link fit together, add an adapter, or contribute. The [More](more/README.md) section covers architecture, modules, and development.

- [Architecture](more/architecture/README.md): how the parts of Local Link fit together
- [Repository Organization](more/architecture/organization.md): where each piece of code belongs
- [Developing](more/developing/README.md): build, test, and contribute

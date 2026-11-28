<div align="center">

<img src="web/apps/docs/src/app/icon.png" alt="Local Link sunflower logo" width="128" height="128">

# Local Link

**Own Your Intelligence**

[Website](https://local.link/) · [Documentation](https://docs.local.link/) · [Presale](https://cloud.local.link)

**[MIT](LICENSE-MIT) · [Apache](LICENSE-APACHE)**

</div>

---

**Local Link starts an AI agent on your computer in minutes.** Your agent
searches the web, writes, runs programs, and works on a schedule, inside a
sandbox you control. Text your agent from your phone, and keep your agent
always on in your own cloud.

<!-- quickstart -->

## Install

**Install only the parts you use.** Local Link is modular, so each
capability is a program of its own. Start on macOS or Linux, and
[setup](docs/more/modules/install.md) helps you choose. The last command
starts your agent.

```sh
curl -fsSL https://local.link/install.sh | sh
lnk up
lnk agent start
```

## Skip the Setup

**Get an always-on agent with no cloud account, model keys, or command line
to manage.** These are the three hardest parts of running your own AI agent.
[Join the presale for early pricing](https://cloud.local.link).

## Agents

**Imagine any agent, and share it.** Create an
[agent](modules/agents/agent/docs/agents/README.md) you can talk to in your
apps, move your agent between machines, and share your agent with your
friends and your community.

### Start Your Agent

**Start an agent blank, or from one someone shared.**
[Start an agent](modules/agents/agent/docs/agents/start/README.md) on this
computer and choose its model when asked. The agent keeps running in the
background. [What Are Agents](modules/agents/agent/docs/agents/agent.md)
explains the five parts every agent is made of.

```sh
lnk agent start
```

### Talk to Your Agent

**Chat in your terminal or from your phone.**
[Talk to your agent](modules/agents/agent/docs/agents/chat/README.md) here,
then connect
[Slack](interop/agents/connections/slack/docs/agents/connections/slack/README.md),
[Discord](interop/agents/connections/discord/docs/agents/connections/discord/README.md),
[Telegram](interop/agents/connections/telegram/docs/agents/connections/telegram/README.md),
or [Signal](interop/agents/connections/signal/docs/agents/connections/signal/README.md)
to reach your agent where you already talk.

```sh
lnk agent chat
lnk agent connect slack|discord|telegram|signal
```

### Create a Shareable Agent

**Share the agents you make, and start the ones your friends make.**
Anyone who runs Local Link can make an agent, and anyone who makes one
can [share it](modules/agents/agent/docs/agents/share/README.md) as one
[archive](modules/tooling/archive/docs/tooling/archive/README.md) file.
Whoever imports it checks every file before it lands and gets an agent
of their own, while your credentials stay with you. Start from one of
the six [ready-made agents](agents/README.md), or from one your
community shared.

To share an agent, stop it and export it as one file.

```sh
lnk agent stop
lnk archive export --select agent.main
```

To start an agent someone shared with you, import its file and start it.
A ready-made agent's file is on the
[latest release](https://github.com/so-local/link-releases/releases/latest)
as `link-<name>.tar.gz`: run this where you saved it.

```sh
lnk archive import link-coach.tar.gz
lnk agent start --agent coach
```

### Leverage Link Harness

**Local Link Harness is fast, and your agent's instructions and
conversations are plain files.** Our
[first-party harness](modules/agents/harnesses/link/docs/agents/link.md) runs
your agent by default. Local Link Harness answers 13 times faster than Hermes
and uses 81% less memory. Local Link Harness answers 50 times faster than
OpenClaw and uses 94% less memory. You open and edit every instruction,
skill, and conversation like any other file.
[View benchmarks](modules/agents/harnesses/link/docs/agents/harnesses/link/benchmarks.md)

```sh
lnk agent harness set link
```

### Move Your Agent

**Move your agent to your cloud and back again.**
[Move your agent](modules/agents/agent/docs/agents/move/README.md) to a
[cloud account you've connected](#connect-your-cloud-accounts), keeping its
files, chats, memory, history, and connections, and bring it back here
whenever you like.

```sh
lnk agent move aws|gcp
lnk agent move here
```

Or create an agent on a cloud box from the start, so it never runs on
your computer.

```sh
lnk agent create work --on aws-1|gcp-1
```

### Migrate to Local Link

**Bring an agent you already use under Local Link.**
[Migrate an existing agent](modules/agents/agent/docs/agents/migrate/README.md)
from OpenClaw, Hermes, or DeepSeek, then manage it through the same
[agent commands](modules/agents/agent/docs/agents/README.md). The
original keeps working until you choose to remove it.

```sh
lnk agent migrate --harness openclaw
lnk agent migrate --harness hermes
lnk agent migrate --harness deepseek
```

### Change Your Harness

**Run your agent on the harness you like.** You can
[switch harnesses](modules/agents/agent/docs/agents/harnesses/use/README.md)
to use
[OpenClaw](interop/agents/harnesses/openclaw/docs/agents/harnesses/openclaw/README.md),
[Hermes](interop/agents/harnesses/hermes/docs/agents/harnesses/hermes/README.md),
or [DeepSeek](interop/agents/harnesses/deepseek/docs/agents/harnesses/deepseek/README.md),
and your agent keeps its state, model, and connections.

```sh
lnk agent harness set openclaw|hermes|deepseek
```

### Change Your Model

**Choose the model your agent thinks with.** Pick a
[hosted model](modules/agents/agent/docs/agents/models/agent/README.md) from Anthropic,
OpenAI, or any maker through OpenRouter, or use a
[model on your computer](modules/agents/model/docs/agents/models/README.md).

```sh
lnk agent model set anthropic/claude-sonnet-5-5
lnk agent model set openai/gpt-6.1-sol
lnk agent model set openrouter/anthropic/claude-sonnet-5-5
```

## Cloud

**Your cloud account, your choice of provider.** Use
[cloud services](docs/services/README.md) through your own
Amazon Web Services or Google Cloud account. Connect the account once,
then use it for compute and storage.

### Connect Your Cloud Accounts

**One sign-in, from the login you already have.**
[Connecting an account](modules/cloud/cloud/docs/services/cloud/README.md)
reuses your [AWS CLI](interop/cloud/aws/docs/services/cloud/aws.md) profile
or your [gcloud](interop/cloud/gcp/docs/services/cloud/gcp.md) login. You
can give it an AWS access key or a Google Cloud service account's key file
instead. Every part of Local Link then uses that one credential, and
connecting makes nothing in the account. When a box needs either command
line and it's missing, Local Link installs it for you, for your user only.

```sh
lnk cloud connect aws|gcp
```

### Run Programs in Cloud Boxes

**Cloud machines with Local Link already installed.** Create a
[box](modules/compute/box/docs/services/boxes/README.md) in a cloud account
you connected to run programs there. Then open a shell on the box.

```sh
lnk box create aws|gcp
lnk box ssh
```

### Keep Files in Cloud Storage

**Verified backups and restores.**
[Back up your folders](modules/storage/storage/docs/services/storage/README.md)
to cloud storage, verify each copy, and restore files when you need them.
Create a bucket, then push a folder to copy it up to your cloud. Pull it
later to get the files back onto your computer, each one checked.

```sh
lnk storage create aws|gcp
lnk storage push photos/2019
lnk storage pull photos/2019
```

## Sandbox

**Every program Local Link runs gets only the access you give it.** The
[sandbox](modules/security/sandbox/docs/tooling/sandbox/README.md) lets a
program change only the folders you hand it and reach only the hosts you
allow. Your settings and credentials live in
[Safe folders](docs/more/reference/locations.md#safe-exec-and-data) that
nothing in a sandbox can read or change.

### Agents Run Sandboxed

**Every agent starts in the sandbox, whichever harness runs it.**
[Permissions](modules/agents/agent/docs/tooling/sandbox/permissions/README.md)
say what your agent may reach beyond its own folder. Grant a host or a
folder when the agent needs one, and take it back the same way.

```sh
lnk agent permission add --internet.hosts api.github.com
```

### Sandbox Any Program

**Run a shell, a script, or a tool you don't trust with the same limits.**
Give it the folders it may write and the tools it may run, and nothing
else.

```sh
lnk sandbox run --commands.toolchains shell --files.write . -- bash
```

### Check the Limits Hold

**See for yourself that nothing gets through.** The check sets out bait,
such as a secret in your home folder and one in Local Link's settings,
then tries to reach each from inside a sandbox. It fails if anything gets
through.

```sh
lnk sandbox check
```

## Uninstall

**Leave cleanly, with a clear choice about your data.**
[Uninstall Local Link](docs/more/modules/guide.md#uninstall) and the harness
software it installed, keeping saved data unless you choose to delete it.
The [location guide](docs/more/reference/locations.md) explains
what lives where, so you can inspect the footprint yourself.

```sh
lnk uninstall
```

## More

For repeatable specifications and settings, start with [Configuration](docs/config/README.md).
[Tooling](modules/tooling/docs/tooling/README.md) groups the sandbox,
measurement, the offline manual, build tools, and archives.

**[Guides for using and building Local Link](docs/more/README.md).** Read the
[user documentation](docs/guide.md) or browse it offline with
[`lnk man`](modules/tooling/manual/docs/tooling/manual/README.md).

- [More](docs/more/README.md)
- [Repository Organization](docs/more/architecture/organization.md)
- [Architecture](docs/more/architecture/README.md)
- [Developing](docs/more/developing/README.md)
- [Website](https://local.link/)
- [Documentation](https://docs.local.link/)
- [Presale](https://cloud.local.link)
- [Our Values](docs/more/values.md)
- [Code of Conduct](docs/more/code-of-conduct.md)
- [Report a Vulnerability](docs/more/vulnerabilities.md)

## License

**Local Link is free and open source.** You can read every line, run Local
Link on your own machines, and keep what your agents learn about you private.
Use Local Link under your choice of the [MIT](LICENSE-MIT) or
[Apache 2.0](LICENSE-APACHE) license.

<details>
<summary>Repository Contents</summary>

<!-- docs-tree:start -->

## Docs

### Pages

- [Overview](docs/guide.md): Install Local Link, start an agent, and choose the services and tools the agent needs
- [Cloud](docs/services/README.md): Connect your own clouds, then create boxes to run programs and buckets to keep files
- [Configuration](docs/config/README.md): Configuration is the TOML you write to tell Local Link what you want, and it comes in two kinds: specifications and settings
- [Core](core/README.md): Local Link core supplies the machinery that every Local Link program shares
- [Interoperability](interop/README.md): Local Link works with other projects' products through adapters
- [Local Link's Agents](agents/README.md): Local Link's ready-made agents in `ready-made/` are where you start: import one, make it yours, and share what you make
- [Modules](modules/README.md): Modules are the programs Local Link installs and runs
- [Packages](packages/README.md): Packages holds the shared libraries that Local Link's programs link
- [Scripts](scripts/README.md): Scripts holds what a release reads from the workspace, the shell completion for `lnk`, and the index of the website's pages
- [Tests](tests/README.md): Tests holds the checks that read across Local Link's packages: the repository checks, the docs checks, the check runner and its timing report, each specification's cases, and the fixtures system tests run
- [Web](web/README.md): Web holds the npm workspace of Local Link's documentation website, docs.local.link, and the package every Local Link website shares

### More

- [More](docs/more/README.md): Local Link is open source

<!-- docs-tree:end -->

</details>

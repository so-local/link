# Product Names

Every name in title case that Local Link's docs, websites and messages
use for the company and what it makes, with what each one means. A name
means one thing everywhere, and a program's exact name, such as
`lnk-file-tool`, goes in backticks beside it.

## Company and Command Line

Local Link is the company and everything it publishes: the product, the
websites at local.link and the repository. Link is the command line and
the modules it runs, whose command is `lnk`. A sentence about what a
program does says Link, and one about who makes, sells or pays for
something says Local Link.

| Name | What it means |
|---|---|
| Local Link | The company, its product, websites and repository |
| Local Link Company | The company's legal name, in licenses and notices |
| Link | The `lnk` command line and the modules it runs |

## Link Harness

Link Harness is Link's own harness, the program that runs an agent's
turns, and its program is `lnk-harness`. Its instructions and skills
are files you edit, it shows the prompt it sends, and it keeps your
conversations in [an event log you control](../../../modules/agents/harnesses/link/docs/agents/harnesses/link/README.md).
Other products' harnesses, such as Hermes, run under Link too, and
[`lnk agent harness set link`](../../../modules/agents/agent/docs/agents/harnesses/use/README.md#switch-in-one-command) switches an agent to Link Harness.

## Clouds

A cloud is anywhere Link makes boxes, buckets, jobs and agents, reached
through an adapter. An account uses the name you choose when connecting
its provider, such as AWS or Google Cloud. A name a cloud goes by, its
kind or its kind without `-cloud`, belongs to that cloud's own account:
an account of another kind never takes it.

## Agents

An agent is a model working for you with its own instructions, files and
conversations, and a harness is the program that runs its turns. Link
runs its own harnesses and those of other products, such as Hermes.

| Name | What it means |
|---|---|
| Mini Harness | Smallest harness, with the whole agent loop readable |
| Models | Model module: local runtimes or your provider's key |

## Tools

A tool is an MCP (Model Context Protocol) server, a program an agent's
harness calls for one kind of work. Each of Link's tools is named Link
and its subject, and its program is `lnk-<subject>-tool`, except Link
Document Converter's, which is `lnk-document-converter`, and Link MCP's,
which is `lnk-mcp`.

| Name | What it means |
|---|---|
| Link File | Reads, writes, replaces and finds an agent's files |
| Link Run | Runs an agent's programs, each call in a sandbox |
| Link Code | Asks language servers for diagnostics and references |
| Link Browser | Opens pages in the agent's own sandboxed Chromium |
| Link Desktop | Gives an agent a virtual desktop to see and work |
| Link Web | Fetches pages as Markdown and searches the web |
| Link Memory | Searches and reads an agent's memory and conversations |
| Link Scheduler | Subagents, schedules and messages for later |
| Link Document Converter | Text of PDF and Word files, for Link Memory |
| Link MCP | Local Link's commands as tools, for any MCP client |

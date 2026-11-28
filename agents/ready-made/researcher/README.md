# Researcher

Researcher is a ready-made research agent that searches the web, reads
its sources, and writes a cited report into its files folder. The full
guide is [Researcher](../../docs/agents/ready-made/researcher.md) in Local Link's docs.

## Who Researcher Is For

Researcher is for anyone with a question that takes an afternoon of
reading to answer well: a purchase to compare, a technology to
evaluate, or a claim to check. Every factual claim in a report cites its
source by title and URL, conflicting sources are told apart, and what
Researcher couldn't confirm is said rather than filled in.

## What Researcher Comes With

Researcher runs on Link Harness, stays awake, and reads no repository's
instructions. Its model is Link's default, `claude-haiku-5-5` on
Anthropic, the current Haiku.

- **Tools:** file for your documents and its reports, scheduler, and web, which fetches pages and searches with a Brave Search or Tavily key.
- **Skills:** `plan-investigation`, which plans the question and when the report is done, and `check-claims`, which checks each claim against its source.
- **Reports:** Markdown files in `reports/` in its files folder, such as `reports/2026-11-10-heat-pumps.md`.

## Bring Researcher In

Download `link-researcher.tar.gz` from Local Link's
[latest release](https://github.com/so-local/link-releases/releases/latest), then import it from the folder you saved it in.
Import checks the whole
archive, asks whether to keep the name `researcher`, and places the agent in
`~/Link/Agents/researcher/`, with its settings in `~/Link/Safe/Agents/researcher/`.
Nothing is installed, connected, or started until you say so.

```sh
lnk archive import link-researcher.tar.gz
```

From a checkout of Local Link's repository, pack this folder as the
release does, then import `~/Link/Archive/researcher.tar.gz`.

```sh
lnk archive export --from agents/ready-made/researcher
```

## Start Researcher and Chat

Add Link's web tool, which installs the `web-tool` module and asks for a
search key. Without the key, Researcher reads only the URLs and
documents you give it. Start Researcher and ask a question, such as
`Compare heat pumps and gas boilers for a cold climate, with sources`.

```sh
lnk agent tool add web --agent researcher
lnk agent start --agent researcher
lnk agent chat --agent researcher
```

## What Researcher Can Reach

Researcher's sandbox writes only its files folder and its own
instructions and skills, reaches the internet over HTTPS through its
proxy, and calls Link Web. Researcher runs no program.

```toml
# ~/Link/Safe/Agents/researcher/sandbox.toml
schema = 1

[files]
write = [".", "instructions", "skills"]

[internet]
hosts = ["*"]

[tools]
names = ["web"]
```

```sh
lnk agent show --agent researcher --permissions
```


## Make Researcher Your Own

**One skill makes Researcher an expert in your field.** An imported agent is yours. Edit its standing instructions in
`~/Link/Agents/researcher/agent/instructions/instruction.md`, or ask the agent
to, since its sandbox writes them. Add or remove skills and tools, and
change one key of its specification with a field flag on
`lnk agent start`, which the agent keeps.

```sh
lnk agent skill list --agent researcher
lnk agent tool add memory --agent researcher
lnk agent spec show researcher
```

A house style for reports, such as a summary first or preferred sources, belongs in its instructions.
[Make an Agent Your Own](../../docs/agents/ready-made/README.md#make-an-agent-your-own)
covers each change, and
[Share an Agent](../../../modules/agents/agent/docs/agents/share/export.md#share-your-agent)
shows how to export your version for others.

## This Folder

This folder is an archive kept as a folder. `archive.toml` names the
agent and lays out its folders: `Agents/researcher/` holds its instructions,
skills, and Home, and `Safe/Agents/researcher/` holds its `agent.toml` and
`sandbox.toml`. This README sits beside `archive.toml`, travels in the
released archive, and is left out on import, so the agent's files stay
exactly what the archive names. The Home starts blank, with headings and
empty fields only, and fills as you talk to the agent.

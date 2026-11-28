# Researcher

Researcher is a ready-made research agent that searches the web, reads its sources, and writes a cited report into its files folder. Every factual claim in a report names its source, and what Researcher couldn't confirm is said rather than filled in.

## Who Researcher Is For

Researcher is for anyone with a question that takes an afternoon of
reading to answer well: a purchase to compare, a technology to evaluate,
or a claim to check. Each answer is a Markdown report you keep, with a
title and a URL for every source, so you can check any sentence
yourself.

## Start Researcher

Import Researcher as [Ready-Made Agents](README.md#bring-one-in)
describes, then add Link's web tool, which installs the `web-tool`
module and asks for a Brave Search or Tavily key for its search. Start
Researcher and ask your question in this terminal.

```sh
lnk agent tool add web --agent researcher
lnk agent start --agent researcher
lnk agent chat --agent researcher
```

### Ask a Question

Ask in plain words, such as
`Compare heat pumps and gas boilers for a cold climate, with sources`.
Researcher plans the investigation, reads each source itself rather
than its summary, and checks every claim against its passage. The chat
answers with the main finding and the report's file, such as
`~/Link/Agents/researcher/home/reports/2026-11-10-heat-pumps.md`.

### Work From Your Own Documents

Put documents in Researcher's files folder and ask about them by name.
Researcher reads them with its file tool and cites each by its path, so
a report grounded in your documents needs no search. Without a search
key, Researcher reads only the URLs and documents you give it, and says
what a search would have added.

## What Researcher Comes With

Researcher runs on Link Harness with Link's default model,
`claude-haiku-5-5` on Anthropic, the current Haiku. Its tools are file,
scheduler, and web, and it runs no program. Two skills hold its method:
`plan-investigation` writes down the question, where to look first, and
what the report must answer, and `check-claims` checks each claim's
numbers, dates, and names against its source.

## What Researcher Can Reach

Researcher's sandbox writes only its files folder, where reports go,
and its own instructions and skills. It reaches any public host over
HTTPS through its proxy, since a question can lead anywhere, and names
Link Web as the one tool it may call. Where the `web-tool` module isn't
on this machine, Researcher starts without web and says so.

## Make Researcher Your Own

**One skill makes Researcher an expert in your field.** A skill holds the
sources, methods, and checks your field's experts use, and Researcher
reads it before every question in that field.
A house style for reports, such as a summary first or your preferred
sources, goes in Researcher's instructions, and memory lets Researcher
search its earlier reports by meaning.
[Make an Agent Worth Sharing](../../../../modules/agents/agent/docs/agents/share/worth-sharing.md) shows how to
change each.

```sh
lnk agent tool add memory --agent researcher
```

## Contribute Here

**A skill for a field you know makes Researcher better for everyone.**
Each skill is a folder of instructions in the agent's `agent/skills/`,
and [How Each Agent Is Kept](README.md#how-each-agent-is-kept) shows
where Researcher's files live and how a release packs them. Send us the
skill for your field.

# Onboarding

Onboarding is your first agent, named `onboarding`: ask what Local Link
can do, and Onboarding answers from Local Link's manual with the command
to type. The full guide is
[Onboarding](../../docs/agents/ready-made/onboarding.md) in Local Link's
docs.

## Who Onboarding Is For

Onboarding is for anyone who just installed Local Link. Ask how to start
another agent, give an agent a permission, or bring in an agent someone
shared, and Onboarding gives you the command and what it does.

## What Onboarding Comes With

Onboarding runs on Link Harness and stays awake. Its model is Link's
default, `claude-haiku-5-5` on Anthropic, the current Haiku.

- **Tools:** Link MCP, which reads Local Link's offline manual, and file and scheduler, so you can try both.
- **Skills:** `what-can-you-do`, which answers from its tools and the manual, and `next-step`, which sets up one thing a command at a time.
- **Home:** a files folder that starts empty.

## Start Onboarding

The agent module carries this folder, and the first `lnk agent start` on
a computer with no agent creates Onboarding from it.

```sh
lnk agent start
lnk agent chat
```

## What Onboarding Can Reach

Onboarding's sandbox writes only its files folder and its own instructions
and skills. Onboarding reaches its model through its proxy, runs no
program, and calls no other tool.

## This Folder

This folder is an archive kept as a folder. `archive.toml` names the
agent and lays out its folders: `Agents/onboarding/` holds its instructions
and Home, and `Safe/Agents/onboarding/` holds its `agent.toml` and
`sandbox.toml`. This README sits beside `archive.toml` and is left out
on import, so the agent's files stay exactly what the archive names.

# Onboarding

Onboarding is your first agent: ask what Local Link can do, and Onboarding answers from Local Link's manual with the command to type. Your first `lnk agent start` creates Onboarding, so it needs no import.

## Who Onboarding Is For

Onboarding is for anyone who just installed Local Link. Ask how to start
another agent, give an agent a permission, or bring in an agent someone
shared, and Onboarding gives you the command and what it does.

## Ask Onboarding Anything About Local Link

On a computer with no agent, [`lnk agent start`](../../../../modules/agents/agent/docs/agents/README.md#start-your-agent) creates Onboarding from
the archive the agent module carries and starts it. Then chat with it in
this terminal.

```sh
lnk agent start
lnk agent chat
```

Ask `What can you do?`, `How do I start another agent?` or
`How do I text you from my phone?`. Onboarding reads the manual's page
for each question and answers from it, never from memory. Onboarding
runs no command for you: you type each one.

## What Onboarding Comes With

Onboarding runs on Link Harness with Link's default model,
`claude-haiku-5-5` on Anthropic, the current Haiku. Its tools are
[Link MCP](../../../../modules/agents/tools/mcp/docs/agents/tools/mcp/README.md),
which reads Local Link's offline manual, and file and scheduler.
File and scheduler let you try both without making a second agent: ask
Onboarding to keep a note, or to remind you in two minutes.
Two skills shape its answers: `what-can-you-do` answers from its tools
and the manual, and `next-step` sets up one thing a command at a time.

## What Onboarding Can Reach

Onboarding's sandbox writes only its files folder and its own instructions
and skills. Onboarding reaches its model through its proxy, runs no
program, and calls no other tool.

```sh
lnk agent show --agent onboarding --permissions
```

## Make Onboarding Your Own

**Onboarding is plain files you can change, like every agent.** Its
instructions are in `~/Link/Agents/onboarding/agent/instructions/instruction.md`,
and a skill you add there teaches Onboarding a task of yours. Start your
next agent once Onboarding has shown you how.

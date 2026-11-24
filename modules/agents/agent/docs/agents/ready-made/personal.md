# Personal Assistant

Personal Assistant is a ready-made agent, named `personal`, that learns about you, keeps your lists and reminders, and plans your day. Personal Assistant keeps what it learns in plain files, one for each part of your life, which you read and edit like any other.

## How Far Personal Assistant Goes

We've worked with Personal Assistant, and we think it works. It's still
an example, made to show what one agent that learns about you in plain
files can do. The parts of your life it should keep, and how it should
remind you, belong in its skills and instructions.

## Who Personal Assistant Is For

Personal Assistant is for anyone who wants one place to tell what
matters, from a sister's birthday to a bill's due date, and be reminded
of it at the right time. It answers in a few short lines, asks before
assuming, and says in one line what it saved. What the agent knows is
only for you, never repeated to others, in a group chat included.

## Start Personal Assistant

Import the agent as [Ready-Made Agents](README.md#bring-one-in)
describes, start it, and connect the messaging app you already use. On
its first chat it asks your name, your city, and how you like answers,
then learns the rest as it comes up.

```sh
lnk agent start --agent personal
lnk agent chat --agent personal
lnk agent connect --agent personal telegram|slack|discord|signal
```

### Reminders and a Morning Brief

Ask for a reminder in your own words, such as
`Remind me to call Ana on Friday at six`. The agent confirms the time
and your zone, then creates a schedule. Once it knows your zone, it
offers a daily morning brief that plans your day from your routines,
dates, reminders, and open to-dos.

## What Personal Assistant Keeps

The agent's Home, its files folder, holds one Markdown file for each
part of your life, each with fixed headings it fills as you tell it
things. When something changes, it changes the line rather than adding a
second one. Your lists hold only what you said, and anything more is
offered as a suggestion.

- `about-me.md` holds the basics, what you like, how you like answers, and your time zone.
- `people.md`, `work.md`, and `health.md` hold family and friends, projects and deadlines, and appointments.
- `home.md` and `money.md` hold household practicalities, with no address, and budgets, bills, and due dates.
- `interests.md`, `routines.md`, and `dates.md` hold tastes, daily and weekly rhythms, and birthdays and renewals.
- `lists/` holds `todo.md` and `shopping.md`, and `notes/` holds anything else, one file per topic.

### Never Secrets

The agent's instructions tell it never to record a password, a PIN, a
card or account number, a one-time code, or a recovery phrase, in any
file or schedule. Shared one, it saves nothing and says why, since its
files are readable text. Budgets, bills, and due dates are fine.

## What Personal Assistant Comes With

Personal Assistant runs on Link Harness with Link's default model,
`claude-haiku-5-5` on Anthropic, the current Haiku. Six skills cover
the everyday work: `learn-about-you`, `recall`, `remind`, `plan-day`,
`lists`, and an optional `weekly-review`.

### Tools

The agent comes with file, scheduler, memory, web, Link MCP, run, and
browser on. Scheduler creates reminders without asking first, since you and the
agent are its only parties. Memory searches its files and conversations
by meaning, and web fetches pages and searches once a key is set. Run
runs `sh` alone, for a builtin's arithmetic or a date, and code stays
off. The browser, installed with the `browser-tool` module on first
start, needs Chromium, Google Chrome, Microsoft Edge, or Brave, and the
agent asks before it books, buys, signs up, or sends anything.

### Link MCP

[Link MCP](../../../../tools/mcp/docs/agents/tools/mcp/README.md) lets
you ask the agent about Local Link. The agent reads Link's manual and
runs Link's declared commands, and a change asks you first.

## What Personal Assistant Can Reach

The agent's sandbox writes only its files folder and its own
instructions and skills, and reaches its model and the web through its
proxy, to any public host. The sandbox file grants one program, `sh`,
so a script reaches no other, and names Link Web as a tool it may call.

## Make Personal Assistant Your Own

A part of your life the Home doesn't have yet, such as `car.md`, is a
new file, with a line in the Home's `README.md` saying what it holds. A
habit you want every day, such as an evening wrap-up, is a schedule the
agent creates when you ask, and
[Make an Agent Worth Sharing](../share/worth-sharing.md) covers the
rest.

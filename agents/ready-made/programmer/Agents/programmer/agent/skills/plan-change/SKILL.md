---
name: plan-change
description: Plan a code change before making it: what changes, where, how it is tested, in what steps, and what could break.
---

# Plan a Change

Write a short plan before you edit anything, and say it in your reply.
For a planning-only task, read the relevant code first and deliver the
requested plan without implementing it or committing. Save a plan file
only when the owner requests one. Use the same planning method in every
language, with commands and idioms from the matching language skill.

- **Goal:** the behavior that changes, in one sentence, and what is
  explicitly not part of it.
- **Where:** the files and functions you will change, found by reading
  the code, never guessed. Include callers and implementers of anything
  whose signature or behavior changes (`grep` for them).
- **Steps:** an ordered list of small steps, each leaving the build and the
  tests green. Put the test first where you can.
- **Tests:** the test that fails before the change and passes after it,
  plus edge cases (empty, one, many, invalid, boundary, error path).
- **Risks:** public interfaces, data formats, migrations, concurrency,
  performance and other users of the code, and how you will check each.
- **Choices:** explain material tradeoffs and compatibility constraints.
  Separate facts found in the repository from assumptions to confirm.
- **Open questions:** anything the task leaves unclear. If the answer
  changes the design, ask before building.

Keep the plan to what was asked. If the task is trivial, a two-line plan
is right. If it is large, split it into separate commits, and offer the
split before starting. Something larger you notice goes in your report as
a suggestion, not in the change.

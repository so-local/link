---
name: check-in
description: Send a short, kind evening check-in on today's planned session.
---

# Check In

Reads: `plan/week.md`, `profile.toml`, `endurance/sessions.csv`,
`strength/sets.csv`, `team/log.csv`, `nutrition/log.csv`,
`body/measurements.csv`. Writes nothing; logging comes from their reply.

1. If `onboarded` is `false` in `profile.toml`, send one friendly line
   offering to get started (onboard), and stop.
2. Today's date: find today's row in `plan/week.md`. If it is rest, or
   the week's plan is out of date, say nothing, or send one short line
   if something useful follows from the logs (no food logged in 2 days,
   no weight in a week).
3. If today's session has a log row for today, or status `done`: send a
   short congratulation and the next planned session. No questions.
4. If it is still `planned`: ask one question, "How did today's
   (session) go?" with a kind way out ("or tell me if you skipped it").
5. If yesterday's session was missed and they did not reply, do not ask
   again. Offer once to move it.
6. One message, at most three short lines. Never guilt, never repeat.

If a reply tells you they did the session, run the matching log skill.
If they skipped, mark it `skipped` in `plan/week.md`, ask why in one
line, and offer to adjust the week. If they mention pain or illness,
follow the safety rules.

---
name: log-team-session
description: Log a team practice or game and keep the team schedule current.
---

# Log a Team Practice or Game

Reads: `team/schedule.csv`, `team/notes.md`. Writes: `team/log.csv`,
`team/schedule.csv`, `team/notes.md`, `plan/week.md`.

1. Get the date, kind (`practice` or `game`), the opponent or practice
   focus, RPE, and for a game the minutes played and the result.
2. Append one row to `team/log.csv`:
   `date,kind,opponent_or_focus,minutes_played,result,rpe,highlights,notes`
   Shape: `<YYYY-MM-DD>,<practice|game>,<their words>,<minutes>,<result>,<rpe>,"<their words>","<their words>"`
   Leave `minutes_played` and `result` blank for practice.
3. Remove the matching row from `team/schedule.csv` (same date and kind)
   by replacing the file without that row.
4. Add a dated one-line note to `team/notes.md` under Notes if there is
   something about the position or a goal: `YYYY-MM-DD: text`.
5. Mark the matching day in `plan/week.md` as `done`.
6. If they report a knock or tightness, say what to do for recovery and
   apply the safety rules if it is pain. Consider the load: a game or
   hard practice counts as a hard day for the next day's plan.

Reply: one line of what you logged and how recovery looks.

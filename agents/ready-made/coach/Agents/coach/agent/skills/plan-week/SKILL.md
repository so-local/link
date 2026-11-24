---
name: plan-week
description: Write next week's plan into plan/week.md from the block, program, team schedule and last week.
---

# Plan the Week

Reads: `profile.toml`, `endurance/block.md`, `strength/program.md`,
`team/schedule.csv`, `plan/reviews.csv`, `plan/week.md`,
`body/measurements.csv`. Writes: `plan/week.md`.

1. The week starts on a Monday. Before overwriting `plan/week.md`, make
   sure last week has a row in `plan/reviews.csv` (run weekly-review if
   not).
2. Fixed items first: team practices and games in `team/schedule.csv`
   for those dates, and any race in `endurance/races.csv`.
3. Then the `availability` days and `minutes_per_day` in the profile.
   Never plan outside them.
4. Add the block's week row (`weekly_km`, `long_run_km`, `key_sessions`)
   and the program's lifting days. Put the long session on
   `long_session_day`.
5. Spacing: no two hard days in a row (hard is intervals, tempo, a long
   run, a heavy lower-body day, a game). Lifting legs is not the day
   before a key run or a game. At least one full rest day. If they are
   short of sleep or the last review shows overload, make the week
   lighter, not heavier.
6. Write `plan/week.md` with its exact header: `week_start`, `focus`
   and a table row per day:
   `| 2027-01-18 | Mon | Easy run | 8 km at Z2, 5:50-6:20/km | planned |`
   Include rest days as `Rest` rows so nothing is ambiguous.
7. Say the plan in a short list by day (day, session, one detail).

Reply with that list and one sentence on the week's focus.

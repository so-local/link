---
name: log-endurance
description: Log a run, ride or swim and say how it fits the plan.
---

# Log an Endurance Session

Reads: `endurance/sessions.csv` (to see the week), `plan/week.md`,
`endurance/block.md`, `endurance/zones.md`. Writes: `endurance/sessions.csv`,
`plan/week.md`, and `endurance/races.csv` for a race.

1. Get the date (today if unsaid), sport, distance, duration and RPE.
   Ask for only what is missing. Heart rate and notes are optional.
2. Type: `easy`, `long`, `tempo`, `intervals`, `recovery`, `race` or
   `cross`. Infer it from what they said, or from the plan for that day.
3. Pace for runs: `duration_min / distance_km`, written `M:SS`
   (for example 58 min over 10.2 km is 5.69 min, so `5:41`). Leave it
   blank for rides. For swims, put the pace per 100 m in notes.
4. Append one row to `endurance/sessions.csv`:
   `date,sport,type,distance_km,duration_min,avg_pace_min_per_km,avg_hr_bpm,rpe,notes`
   Shape: `<YYYY-MM-DD>,<sport>,<type>,<km>,<min>,<m:ss>,<bpm>,<rpe>,"<their words>"`
5. In `plan/week.md`, set the matching row's `status` to `done` (or add
   a row if unplanned, status `done`).
6. If type is `race`, put the finishing time in `result_time` of the
   matching row of `endurance/races.csv`.
7. Add this week's distance in `sessions.csv` (Monday to Sunday) and
   compare it with the block's `weekly_km` for the week, if a block is
   active.
8. Check for trouble: an easy run with an RPE of 7 or more, a heart
   rate above Z2 on an easy day, or pain in the notes. Say it kindly
   and apply the safety rules if there is pain.

Reply: one line of what you logged, the week's distance versus plan, and
what is next.

---
name: weekly-review
description: Review the past week against the plan and record it in plan/reviews.csv.
---

# Weekly Review

Reads: `plan/week.md`, `endurance/sessions.csv`, `strength/sets.csv`,
`team/log.csv`, `nutrition/log.csv`, `nutrition/targets.md`,
`body/measurements.csv`, `endurance/block.md`. Writes: `plan/reviews.csv`,
`endurance/block.md` (`done_km`).

1. The week is the Monday to Sunday in `week_start` of `plan/week.md`.
2. Count: `sessions_planned` is the non-rest rows; `sessions_done` is
   the rows whose status is `done` or that have a log row that week.
   Reconcile `week.md` with the logs, marking missing ones `skipped`.
3. Totals: kilometres by sport from `sessions.csv`, lifting sessions and
   total sets from `sets.csv`, team sessions and minutes from `log.csv`.
   Write the week's total km as `done_km` in the block's week row.
4. Nutrition: for each day with food logged, the kcal and protein versus
   targets. Average the days logged and name how many were logged.
5. Body: weight average this week versus last, sleep average, resting
   heart rate trend, from `measurements.csv`.
6. Judge: what went well (wins), what was a problem (missed sessions,
   high RPE, pain, poor sleep, low protein), and one focus for next week.
   Load check: if weekly distance rose more than 10% over last week or
   more than one session had RPE 9 or higher, make next week lighter.
7. Append one row to `plan/reviews.csv`:
   `week_start,sessions_planned,sessions_done,training_load_note,wins,problems,next_focus`
   Shape: `<YYYY-MM-DD>,<count>,<count>,"<note>","<wins>","<problems>","<focus>"`
8. Do not rewrite `plan/week.md` here; plan-week replaces it.

Reply: a short message on the week with the 3 numbers that matter, one
win, one thing to change, and the focus for next week.

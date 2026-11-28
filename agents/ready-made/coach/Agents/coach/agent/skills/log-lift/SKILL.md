---
name: log-lift
description: Log a lifting session set by set and update personal records.
---

# Log a Lifting Session

Reads: `strength/exercises.csv`, `strength/records.csv`,
`strength/program.md`, `plan/week.md`. Writes: `strength/sets.csv`,
`strength/exercises.csv`, `strength/records.csv`, `plan/week.md`.

1. Get the date and each exercise with its sets as reps and weight, for
   example "squat 5x5 at 80" or "bench 8 at 60, 7 at 60, 6 at 60". Ask for
   RPE of the hardest set per exercise if they have not said.
2. Match each exercise to a name in `exercises.csv`. If it is missing,
   add a row first: `exercise,pattern,primary_muscles,equipment,notes`
   (shape `<their name>,<pattern>,"<muscles>",<equipment>,`).
3. Append one row per set to `strength/sets.csv`:
   `date,exercise,set,reps,weight_kg,rpe,notes`
   Shape: `<YYYY-MM-DD>,<catalogue name>,<set>,<reps>,<kg>,<rpe>,`.
   `set` counts from 1 per exercise for that day. Bodyweight sets use
   `0`; added load is the number.
4. Estimated 1RM (Epley) for each exercise, from its best set of 10
   reps or fewer: `weight_kg x (1 + reps / 30)`, rounded to 0.5 kg. For
   1 rep it is the weight itself. Example: 80 x (1 + 5/30) = 93.3, so 93.5.
5. In `records.csv`, if it beats the row's `est_1rm_kg` (or the exercise
   has no row), replace or add the row:
   `exercise,est_1rm_kg,best_reps,best_weight_kg,date,notes`
   and tell them it is a new record.
6. Compare with the program's progression rules: if every set reached
   its top reps at RPE 8 or less, say the weight for next time.
7. Mark the matching day in `plan/week.md` as `done`.

Reply: one line of what you logged, any record, and next time's weight.

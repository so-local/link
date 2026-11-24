---
name: plan-strength
description: Write a progressive-overload lifting program with deloads and estimated 1RMs.
---

# Plan Strength

Reads: `profile.toml`, `strength/sets.csv`, `strength/records.csv`,
`strength/exercises.csv`, `strength/program.md`. Writes:
`strength/program.md`, `strength/exercises.csv`, `strength/records.csv`.

1. Ask the goal (strength, size, support for a sport), days per week
   (from the profile), equipment (gym or home) and `limits`. Respect
   every limit; swap exercises rather than argue.
2. Pick a scheme by experience:
   - Beginner: full body, 2 to 3 days, 3 sets of 5 on squat or leg press,
     a press, a row or pull-down, and a hinge, linear progression.
   - Intermediate: upper and lower or push, pull and legs, 3 to 4 days,
     3 to 4 sets of 5 to 8 on main lifts, 8 to 12 on accessories.
   - With endurance or a team sport in season: 2 days, full body, 2 to 3
     sets, kept off the day before a key run or game.
3. Starting weights: from `records.csv` use 70 to 75% of `est_1rm_kg` for
   the work sets. With no history, start light: the weight they could
   lift for 8 clean reps with 3 left in the tank, then adjust at once.
   Estimated 1RM (Epley), from a set of 10 reps or fewer:
   `weight_kg x (1 + reps / 30)`.
4. Progressive overload: when every set reaches its top reps at RPE 8 or
   lower, add the increment (2.5 kg lower body and pulls, 1.25 to 2.5 kg
   presses) next session. Miss reps two sessions in a row: drop 10% and
   rebuild. Deload every 4 to 6 weeks, or sooner if sleep, joints or
   motivation say so: 60% of the sets at 90% of the weight for one week.
5. Add any new exercise to `strength/exercises.csv` first
   (`exercise,pattern,primary_muscles,equipment,notes`).
6. Write `strength/program.md` with its exact header lines, the days
   table (`day,focus,exercise,sets,reps,start_weight_kg,increment_kg,rest_s`)
   and set `next_deload_date`. Keep the progression rules section.

Reply: the scheme in a short list, first session's weights, the deload
date, and the next step (plan-week).

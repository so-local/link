---
name: log-body
description: Log weight, waist, resting heart rate or sleep and spot trends.
---

# Log Body and Recovery

Reads: `body/measurements.csv`, `nutrition/targets.md`. Writes:
`body/measurements.csv`, `profile.toml` (`weight_kg`).

1. Get the date (today if unsaid) and whichever of weight (kg), waist
   (cm), resting heart rate (bpm) and sleep (hours, last night) they give.
2. If a row for that date exists, replace it with the merged values.
   Otherwise append: `date,weight_kg,waist_cm,resting_hr_bpm,sleep_h,notes`
   Example: `2027-01-12,72.4,,52,7.5,"slept well"`. Leave unknown cells
   empty.
3. Set `weight_kg` in `profile.toml` to the newest weight.
4. Trends: weight is noisy, so compare the average of the last 7 rows
   with the average of the 7 before. A resting heart rate 5 bpm or more
   above their usual for three days, or sleep under 6 hours three days
   in a row, means rest: say so and ease the next hard session.
5. If weight moved 2 kg or more since `weight_kg_used` in `targets.md`,
   suggest the nutrition-targets skill.

Reply: one line of what you logged and one line of trend, if any.

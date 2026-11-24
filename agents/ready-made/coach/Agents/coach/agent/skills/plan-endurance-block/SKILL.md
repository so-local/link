---
name: plan-endurance-block
description: Build a periodized running, cycling or swimming block toward a race, with zones.
---

# Plan an Endurance Block

Reads: `profile.toml`, `endurance/sessions.csv`, `endurance/races.csv`,
`endurance/zones.md`, `endurance/block.md`. Writes: `endurance/block.md`,
`endurance/zones.md`, `endurance/races.csv`.

Before planning: they must have a goal race or target date, and a
current weekly distance (last 4 weeks of `sessions.csv`, or ask). If
`health` has an injury, plan with it or send them to a professional.
Starting a marathon block with under 20 km a week or 3 months of
running: suggest a base block first.

1. Add the goal race to `races.csv` if absent:
   `date,name,sport,distance_km,priority,goal_time,result_time,notes`
2. Weeks available = from the next Monday to the race. A marathon needs
   12 to 20 weeks; with fewer, plan the best shorter block and say so.
   If they have more, start with extra base weeks.
3. Starting weekly distance = the average of the last 4 weeks, rounded.
4. Phases for a marathon (scale for other distances): base 35%, build
   35%, peak 15%, taper 3 weeks (shorter races: 1 to 2 weeks).
5. Weekly distance: increase by at most 10% a week. Every fourth week is
   a step-back week at 70 to 80% of the week before. Peak weekly
   distance for a marathon is usually 55 to 90 km depending on
   experience (beginner 45 to 60, intermediate 60 to 80, advanced 80 or
   more). Never plan past what their days and minutes allow.
6. Long run: about 30% of the week, grows by at most 10% a week, never
   more than 2 km over the previous long run when it is under 20 km,
   with a step-back every fourth week. Marathon peak long run 30 to
   32 km, three weeks before the race. Never run the full distance.
7. Taper (marathon): weekly distance 80%, 60%, then 40% of peak in the
   last 3 weeks, keeping some intensity, with the long run 16 to 20 km,
   then 10 to 12 km. Race week is easy, short runs plus rest.
8. Key sessions per week: one long run, one quality session (tempo in
   build, intervals only in short-race blocks, marathon-pace in peak),
   the rest easy. Four to five runs a week for a marathon; fewer if
   their days say so. Keep intervals under 5% of weekly distance.
9. Zones: with `max_hr_bpm` and a recent race time or a hard effort,
   set `reference_effort` and `threshold_pace_min_per_km` in
   `endurance/zones.md` and write its five rows, replacing any there:

   | zone | name | heart rate, % of `max_hr_bpm` | pace, from threshold | use |
   |---|---|---|---|---|
   | Z1 | Recovery | 50-60 | | warm-up, cool-down, recovery days |
   | Z2 | Easy aerobic | 60-70 | plus 60 to 90 s/km | most running; conversation pace |
   | Z3 | Steady | 70-80 | plus 20 to 40 s/km | marathon-pace work |
   | Z4 | Threshold | 80-90 | threshold | tempo runs, cruise intervals |
   | Z5 | Intervals | 90-100 | minus 15 to 30 s/km | short hard repeats, under 5% of weekly distance |

   Round heart rates to whole beats. Without a recent effort, leave the
   pace column and threshold empty and say to use heart rate and the
   talk test.
10. Before replacing an ended block, append its date range, goal, outcome,
    and main lesson under Completed Blocks in `endurance/block.md`.
    Preserve that section and all earlier summaries. Do not add a notes
    column to the weekly review CSV.
11. Write `endurance/block.md` with its exact header lines, the phases
    table and one row per week (`week` 1..n, Monday date, phase,
    `weekly_km`, `long_run_km`, `key_sessions`). Set `status: active`.

Cycling and swimming blocks use the same phases and the 10% rule, in
hours instead of kilometres: leave `weekly_km` and `long_run_km` empty
and give the hours in `key_sessions`.

Reply: the goal, the number of weeks, the peak week and long run, and
the next step (plan-week).

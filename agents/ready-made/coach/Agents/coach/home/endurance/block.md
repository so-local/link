# Training Block

The current endurance block. Written by the plan-endurance-block skill;
one block at a time. When a block ends, the plan-endurance-block skill
appends a dated summary under Completed Blocks and replaces only the
current block's header and tables. Keep all completed summaries. The weekly
review CSV has no notes column.

status:             <!-- "active" or "done"; empty before the first block -->
sport: 
goal: 
race_date: 
start_date: 
weeks: 
starting_weekly_km: 
peak_weekly_km: 
peak_long_run_km: 

## Phases

| phase | weeks | purpose |
|---|---|---|

## Weeks

One row per week, Monday dates. `weekly_km` is the plan, `done_km` is
filled in by the weekly-review skill from `sessions.csv`.

| week | start_date | phase | weekly_km | long_run_km | key_sessions | done_km | notes |
|---|---|---|---|---|---|---|---|

## Completed Blocks

Completed block summaries, appended with their date range, goal, outcome,
and the main lesson. Keep this section when starting another block.

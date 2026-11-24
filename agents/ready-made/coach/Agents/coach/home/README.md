# Coach Files

Every file the coach keeps, where it lives and its exact shape. The coach
reads this page before it writes, and never creates a file or column that
is not listed here without updating this page first.

## Layout

```text
README.md                  this page
profile.toml               who you are, your goals and limits
endurance/
  sessions.csv             log of runs, rides and swims
  zones.md                 heart-rate and pace zones
  races.csv                race calendar
  block.md                 the current training block
strength/
  sets.csv                 log of every set lifted
  exercises.csv            catalogue of exercises
  records.csv              estimated one-rep maxes and best sets
  program.md               the current lifting program
team/
  schedule.csv             practices and games ahead
  log.csv                  practices and games played
  notes.md                 team, position, demands and goals
nutrition/
  foods.csv                the foods you log, values per 100 g
  log.csv                  what was eaten
  targets.md               daily calories and protein, and how computed
body/
  measurements.csv         weight, waist, resting heart rate, sleep
plan/
  week.md                  this week's plan
  reviews.csv              one row per reviewed week
```

## Rules for Every File

- Metric units, named in the column: `_km`, `_kg`, `_cm`, `_min`, `_h`,
  `_bpm`, `_g`, `_s`.
- Dates are `YYYY-MM-DD`. Times are `HH:MM`, 24 hours.
- A pace is `M:SS` minutes per km. A duration is whole minutes
  (`duration_min`), decimal when needed (`62.5`).
- RPE is the rate of perceived exertion, 1 (very easy) to 10 (all out),
  whole numbers, or half steps for lifting.
- A CSV has one header row, never edited, and one row per event, appended
  at the end. A cell with a comma or a quote is wrapped in double quotes.
  A blank cell is empty, never `n/a` or `0` for unknown.
- Fix a wrong row by replacing that row, and say so in the reply. Never
  delete history.
- A Markdown file keeps its headings and table headers. Fill in
  `key: value` lines and table rows; do not rename them. An empty value
  means not known yet.
- Every file starts with only its headings and empty fields. A row or
  value is written only from what you said or what a skill computed from
  it, never as an example.

## profile.toml

TOML, edited in place. Keys: `onboarded`, `name`, `birth_year`, `sex`,
`height_cm`, `weight_kg`, `time_zone`, tables `experience`, `availability`,
`equipment`, `health`, `nutrition`, `preferences`, and one `[[goals]]`
block per goal with `title`, `area`, `target`, `target_date`, `why` and
`status`. The file's comments say what each key holds. `onboarded`
becomes `true` when the onboard skill finishes.

## Endurance

`endurance/sessions.csv` columns, in order:

| column | meaning |
|---|---|
| date | day of the session |
| sport | `run`, `ride`, `swim` or `other` |
| type | `easy`, `long`, `tempo`, `intervals`, `recovery`, `race` or `cross` |
| distance_km | kilometres, two decimals at most |
| duration_min | minutes of moving time |
| avg_pace_min_per_km | `M:SS`, blank for rides; for swims per 100 m is in notes |
| avg_hr_bpm | average heart rate, blank without a watch |
| rpe | 1 to 10 |
| notes | how it felt, weather, pain, anything else |

`endurance/races.csv`: `date`, `name`, `sport`, `distance_km`,
`priority` (`A` the goal race, `B` a tune-up, `C` for fun), `goal_time`
(`H:MM:SS`), `result_time` (blank until run), `notes`.

`endurance/zones.md` holds `reference_effort` (the race or hard effort
the zones come from), `threshold_pace_min_per_km` (the pace you could
hold for about an hour) and a zone table, five rows once computed. `endurance/block.md` holds `key: value` lines, a
phases table and a weeks table, with the exact columns it shows. Its
Completed Blocks section retains dated summaries before a new block replaces
the active tables. Weekly reviews keep their existing seven-column CSV schema.

## Strength

`strength/sets.csv`, one row per set: `date`, `exercise` (a name from
`exercises.csv`), `set` (1, 2, 3 within that exercise that day), `reps`,
`weight_kg` (0 for bodyweight, extra load as the number), `rpe`, `notes`.

`strength/exercises.csv`: `exercise` (unique name), `pattern` (`squat`,
`hinge`, `push`, `pull`, `carry`, `core`, `lunge` or `other`),
`primary_muscles`, `equipment`, `notes`. Add an exercise here before its
first set is logged.

`strength/records.csv`, one row per exercise, updated in place:
`exercise`, `est_1rm_kg` (Epley, see the plan-strength skill), `best_reps`,
`best_weight_kg` (the set that gave it), `date`, `notes`.

`strength/program.md`: `key: value` lines, a days table and the
progression rules.

## Team Sports

`team/schedule.csv`: `date`, `time`, `kind` (`practice` or `game`),
`opponent_or_focus`, `location`, `notes`. `team/log.csv`: `date`, `kind`,
`opponent_or_focus`, `minutes_played` (blank for practice),
`result` (such as `W 3-1`, blank for practice), `rpe`, `highlights`,
`notes`. `team/notes.md`: team, position, demands, goals, dated notes.

## Nutrition

`nutrition/foods.csv`: one row per food you have logged, values per
100 g, empty until your first. The log-food skill copies a food's row
from its reference table (`skills/log-food/reference-foods.csv`, USDA
FoodData Central, rounded) or a label. The name carries its state in parentheses, such as `Rice (cooked)`, because raw and cooked
differ. Columns: `food` (unique, no commas), `serving_g` (a usual
serving), `serving_desc`, `kcal`, `protein_g`, `carbs_g`, `fat_g`,
`fiber_g`. A packaged food takes its label values per 100 g.

`nutrition/log.csv`, one row per food eaten: `date`, `meal`
(`breakfast`, `lunch`, `dinner`, `snack`), `food` (exactly a name in
`foods.csv`), `grams`, `kcal`, `protein_g`, `carbs_g`, `fat_g` (each
computed as value per 100 g times grams divided by 100, with kcal rounded
to whole numbers and the gram nutrients to one decimal),
`notes`. At an exact halfway value, round up at that precision.

`nutrition/targets.md`: the targets and their inputs. The formula is the
nutrition-targets skill's.

## Body and Recovery

`body/measurements.csv`: `date`, `weight_kg`, `waist_cm`,
`resting_hr_bpm`, `sleep_h` (last night, one decimal), `notes`. One row
per day; leave cells blank for what was not measured.

## Plans and Check-ins

`plan/week.md`: the week's `week_start` (a Monday), `focus` and a table
with `date`, `day`, `session`, `details`, `status` (`planned`, `done`,
`skipped` or `moved`). `plan/reviews.csv`: `week_start`,
`sessions_planned`, `sessions_done`, `training_load_note`, `wins`,
`problems`, `next_focus`.

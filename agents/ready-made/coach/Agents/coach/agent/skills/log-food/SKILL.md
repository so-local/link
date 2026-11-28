---
name: log-food
description: Log what they ate from the food table and report the day against targets.
---

# Log Food

Reads: `nutrition/foods.csv`, `nutrition/log.csv`, `nutrition/targets.md`,
and `skills/log-food/reference-foods.csv`. Writes: `nutrition/log.csv`,
and `nutrition/foods.csv` for a food they log for the first time.

`nutrition/foods.csv` holds only the foods they have logged, and starts
empty. `skills/log-food/reference-foods.csv`, beside this skill, is
reference data: standard values per 100 g, read only, never their own
foods. Never tell them they eat a food because it is in the reference.

1. Get the date (today if unsaid), the meal (`breakfast`, `lunch`,
   `dinner`, `snack`) and each food with an amount. An amount may be
   grams, or a count or portion: convert with the food's `serving_g`
   (2 eggs is 2 x 50 g = 100 g). If unsure, take the serving and say so.
2. Look each food up by name in `nutrition/foods.csv`. Prefer the state
   they described (raw or cooked). If it is missing, copy its row from
   `skills/log-food/reference-foods.csv`; if the reference lacks it too,
   use the label values they give, or standard per-100 g values from
   your knowledge of USDA tables. Add that row to `nutrition/foods.csv`
   and say where its values came from:
   `food,serving_g,serving_desc,kcal,protein_g,carbs_g,fat_g,fiber_g`
   A dish with many parts is logged as its parts.
3. Each value in the log is the food's per-100 g value times `grams`
   divided by 100, to one decimal (kcal to a whole number). At an exact
   halfway value, round up at that precision.
   Example: 150 g Chicken breast (skinless; cooked) at 165 kcal and 31 g
   protein per 100 g is 248 kcal and 46.5 g protein.
4. Append one row per food to `nutrition/log.csv`:
   `date,meal,food,grams,kcal,protein_g,carbs_g,fat_g,notes`
   Shape: `<YYYY-MM-DD>,<meal>,<catalogue name>,<grams>,<kcal>,<g>,<g>,<g>,`
5. Sum that date's rows for kcal and protein (and carbs and fat).
   Compare with `target_kcal` and `target_protein_g` in `targets.md`. If
   targets are still empty, run the nutrition-targets skill first.

For a correction, identify the existing row by its date, meal, and food.
Ask when more than one row matches. Recalculate at the corrected amount
and replace only that row, preserving its notes and all other rows. A
correction never appends a second copy of the meal.

Reply in two lines: what you logged, then the day so far, such as
"1,420 of 2,300 kcal, 96 of 130 g protein. A protein snack would help."
Never shame a number; a day over or under is just data.

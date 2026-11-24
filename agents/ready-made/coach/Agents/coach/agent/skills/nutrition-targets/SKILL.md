---
name: nutrition-targets
description: Compute daily calorie and protein targets with Mifflin-St Jeor and save them.
---

# Nutrition Targets

Reads: `profile.toml`, `body/measurements.csv`, `nutrition/targets.md`.
Writes: `nutrition/targets.md`.

Needs sex, birth year, height, weight, activity level and goal in the
profile. Ask for any missing, and save it in `profile.toml`.

1. Weight: the newest `weight_kg` in `body/measurements.csv`. Age: this
   year minus `birth_year`.
2. BMR (Mifflin-St Jeor): `10 x weight_kg + 6.25 x height_cm - 5 x age`,
   then plus 5 for men or minus 161 for women. Ask if sex is empty.
   If they prefer not to provide it, offer the mean of the two (minus
   78), use it only with their agreement, and explain the estimate.
3. Activity factor from the profile's `activity_level`: sedentary 1.2,
   light 1.375, moderate 1.55, very 1.725, extreme 1.9. If missing, ask
   about ordinary activity as well as training. Do not infer activity
   level from training days alone. TDEE = BMR x factor.
4. Goal: lose is TDEE x 0.85 (never more than 500 kcal below TDEE),
   maintain is TDEE, gain is TDEE x 1.07. Never below BMR. Round to the
   nearest 10 kcal.
5. Protein: 1.8 g per kg, 2.0 when losing, in a range of 1.6 to 2.2.
   Fat: 25% of target kcal divided by 9, at least 0.6 g per kg.
   Carbohydrate: (target kcal minus 4 x protein minus 9 x fat) / 4.
6. Example: a man of 30, 180 cm, 80 kg: BMR = 800 + 1125 - 150 + 5 =
   1780. Moderate (1.55): TDEE 2759. Lose: 2345.15 kcal before rounding,
   2350 kcal saved, protein 160 g.
7. Write the values into `nutrition/targets.md` (`computed_on`, inputs,
   `target_kcal`, `target_protein_g`, `target_fat_g`, `target_carbs_g`).
8. Safety: if the goal would need a deficit over 500 kcal, a weight
   below a BMI of 18.5 or any sign of disordered eating, do not set
   targets and suggest a doctor or dietitian instead.

Reply: the calories and protein, and in one line how they are made.

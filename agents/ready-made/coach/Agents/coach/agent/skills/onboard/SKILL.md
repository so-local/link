---
name: onboard
description: Fill in profile.toml with a new person through a short conversation.
---

# Onboard

Reads: `profile.toml`, `README.md`. Writes: `profile.toml`,
`team/notes.md` if they play a team sport, and its two schedules.

1. Read `profile.toml`. Ask only for what is still empty, one or two
   questions at a time, in this order:
   1. Name, birth year, sex (say it is only for the calorie formula),
      height, current weight.
   2. What they train and their goals: the goal, a measurable target and
      a date. One `[[goals]]` block per goal.
   3. Experience in endurance, strength and team sport.
   4. Days they can train, minutes per day, preferred time, the long
      session day, fixed constraints.
   5. Equipment: gym, home gear, watch, pool, bike.
   6. Injuries, limits, max heart rate if known, anything a doctor told
      them. If something worrying comes up, follow the safety rules.
   7. Food: diet, allergies, dislikes, meals per day, activity level, and
      lose, maintain or gain.
   8. How they like to be coached and how often to check in, and their
      time zone, saved as `time_zone` (an IANA name such as
      `Europe/Lisbon`; ask their city if they don't know it).
2. After each answer, write it into `profile.toml` with the file tool,
   keeping the file's keys and comments.
3. When the essentials (goal, days, limits, height, weight, birth year,
   sex, time zone) are filled, set `onboarded = true`.
4. Add the first weight to `body/measurements.csv` as a row
   (`date,weight_kg,,,,`) if the file has no rows.
5. Before creating recurring schedules, establish that the effective
   scheduler `zone` matches `time_zone` in the profile. Saving the profile
   does not configure the scheduler. If they differ or setup is unknown,
   ask the owner to align the agent's scheduler setup and leave reminders
   explicitly pending. `cron__create` takes a five-field `cron`, with no
   per-call zone, name, or `to`. Put the message in `prompt` and keep
   each returned ID. Check-ins go where they read you: when the head of
   their message names a service, such as "on Telegram", use
   `answer: here`; when it says "on Link", this is Link's own chat, which
   they won't see from their phone. When `answer` offers a connected
   service, ask which should get them and put its name in `answer`; when
   it offers none, use `here` and say they land in this chat.
   Once setup is confirmed, create these unless they asked for fewer:
   - `daily-check-in`, Monday to Saturday at 19:15 (or after their usual
     session time), message "Run the check-in skill.";
   - `weekly-review`, Sunday at 18:00, message "Run the weekly-review
     skill, then the plan-week skill for the week that starts tomorrow."
   Say both times and the zone in one line only after creation succeeds.
6. Offer the next step: nutrition-targets, then plan-endurance-block or
   plan-strength for the main goal, then plan-week.

Reply with one line saying what you saved.

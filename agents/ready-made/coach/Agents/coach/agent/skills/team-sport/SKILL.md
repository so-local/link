---
name: team-sport
description: Set up their team, position, schedule and goals, and train for the position.
---

# Team Sport

Reads: `profile.toml`, `team/notes.md`, `team/schedule.csv`,
`team/log.csv`. Writes: `team/notes.md`, `team/schedule.csv`.

1. Ask the sport, team, position, season start and end, and the
   schedule of practices and games. Fill the `key: value` lines of
   `team/notes.md`.
2. Position Demands: write 3 to 6 short bullets from your knowledge of
   the sport and position: running volume and speeds, sprints and
   changes of direction, contact, key skills, and injuries common to it.
   Mark them as general, to be corrected by them or their coach.
3. Goals: add rows to the Goals table with a measure and a target and
   date, for example minutes played, sprint time, a skill. Link a goal
   to `profile.toml` `[[goals]]` with `area = "team"` too.
4. Schedule: append one row per practice or game to `team/schedule.csv`:
   `date,time,kind,opponent_or_focus,location,notes`
   Example: `2027-02-06,15:00,game,City FC,Home pitch,`. Recurring
   practices: add each date for the next 4 weeks, ask before more.
5. Training for the position: team sessions are the main work. Add at
   most 2 sessions a week: lifting for strength and injury prevention,
   and sprint or conditioning work matching the demands. Never on the
   day before a game. In season keep volume low, off season build.
   Hand the plan to plan-week.

Reply: team and position as saved, the next game, and the one thing you
suggest adding.

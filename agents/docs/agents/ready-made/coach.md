# Coach

Coach is a ready-made personal coach for running, cycling, swimming, lifting, team sports, and everyday eating. Coach keeps your training and food in plain files and checks in on its own, so every answer builds on what really happened.

## Who Coach Is For

Coach is for one person training toward a goal, from a first 5 km to a
marathon or a heavier squat, who wants a plan that fits their week and a
coach who remembers. It usually hears from you on your phone, so its
replies are a few short lines with one next step, and it never shames a
missed session.

## Start Coach

Import Coach as [Ready-Made Agents](README.md#bring-one-in) describes,
start it, and say hello. The first conversation is onboarding: your
body, your goals with a target and a date, the days you can train, your
equipment and limits, how you eat, and your time zone.

```sh
lnk agent start --agent coach
lnk agent chat --agent coach
```

### Log a Session

Tell Coach what you did in your own words, such as
`Ran 10 km easy in 58 minutes, felt good`. Coach adds a row to the
matching log, does the arithmetic itself, and confirms in one line what
it wrote and where you stand against the week's plan.

### Evening Check-Ins

Onboarding creates two schedules once the scheduler's zone is confirmed
to match your time zone: a short check-in each evening from Monday to
Saturday, and a review and next week's plan on Sunday evening. A
check-in with nothing to ask stays quiet, and Coach never repeats an
ignored question. Say stop, and Coach cancels the schedules.

## What Coach Keeps

Coach keeps everything in a fixed set of files in its files folder,
described by `README.md` there, so its records stay consistent from the
first day. Each log is a CSV file that opens in any spreadsheet, metric
with its unit in each column, and a wrong row is replaced, never
deleted.

- `profile.toml` holds your goals, the days you can train, your limits, and your equipment.
- `endurance/` holds runs, rides, and swims, pace and heart-rate zones, races, and a training block.
- `strength/` holds every set, an exercise catalogue, estimated one-rep maxes, and a program.
- `team/` holds a team's schedule, its practices and games, and notes on your position.
- `nutrition/` holds the foods you log with values per 100 g, a food log, and daily targets.
- `body/` and `plan/` hold your measurements, the week's plan, and weekly reviews.

## What Coach Comes With

Coach runs on Link Harness with Link's default model,
`claude-haiku-5-5` on Anthropic, the current Haiku. Its tools are file,
for its logs and plans, and scheduler, for its check-ins, and it runs no
program. A reference table of common foods per 100 g, rounded from USDA
FoodData Central, sits beside its `log-food` skill, and a food becomes
yours only once you log it.

### Skills

Thirteen skills each say which files to read and write and the exact row
format.

- `onboard` fills in the profile in conversation and creates the check-in schedules.
- `log-endurance`, `log-lift`, `log-team-session`, `log-food`, and `log-body` each write one kind of log.
- `plan-week` writes next week's plan from the block, the program, the team's schedule, and last week.
- `plan-endurance-block` builds a periodized running, cycling, or swimming block toward a race.
- `plan-strength` writes a progressive-overload program with deloads and estimated one-rep maxes.
- `team-sport` sets up your team, position, and schedule, and trains for the position.
- `nutrition-targets` computes daily calories and protein with the Mifflin-St Jeor formula.
- `weekly-review` and `check-in` look back at the week and nudge you each evening.

### Safety

Coach is not a doctor or a dietitian. Pain that sharpens or lasts,
dizziness, chest pain, or illness with a fever stops the session until
you've seen a doctor. Coach plans no extreme diet or fast, never more
than 500 kcal below maintenance, and gives no diagnosis, supplement, or
drug advice.

## What Coach Can Reach

Coach's sandbox writes only its files folder and its own instructions
and skills, and reaches its hosted model through its proxy, which the
sandbox file opens to any public host. The file grants no program.

## Text Your Coach From a Cloud Box

Kept on a small box in your Google Cloud account and connected to a
Telegram bot of your own, Coach answers and checks in while your
computer is off. The box costs what Google charges for an `e2-small`
machine and its disk, and your model provider bills its calls.

### Make a Small Box and Move Coach There

Coach runs no program, so the smallest shared-core machine is enough.
Make the box first to choose its size, then [move](../../../../modules/agents/agent/docs/agents/move/README.md#which-box) the coach to your one
box in that account, with its files, conversations, and model
credential.

```sh
lnk cloud connect gcp
lnk box create gcp --name gcp-1 --machine.gcp.size e2-small
lnk agent move --cloud gcp --agent coach
```

### Connect Telegram on the Box

Make a bot with BotFather, open a shell on the box, and connect the
coach where it now runs, as the [Telegram guide](../../../../interop/agents/connections/telegram/docs/agents/connections/telegram/guide.md#on-a-box)
describes. Pairing happens on the box, so your computer never needs to
reach Telegram. Text the bot, and the coach starts onboarding: your
goals, the days you train, and your time zone for its check-ins.

```sh
lnk box ssh gcp-1
```

## Make Coach Your Own

**Coach becomes your coach.** Your sport, your food, and the way you
like to be coached belong in Coach's skills and persona, so every change
makes Coach more yours.
Coach's tone is its `persona.md`, and its coaching principles and
safety rules are its `instruction.md`. A sport Coach doesn't cover is a
new skill, with its log added to the Home's `README.md` first, as
[Make an Agent Worth Sharing](../../../../modules/agents/agent/docs/agents/share/worth-sharing.md) describes.
Teach Coach your sport, then share the coach you made.

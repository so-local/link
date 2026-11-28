# Coach

Coach is a ready-made personal coach for running, cycling, swimming,
lifting, team sports, and everyday eating, which keeps a plan with you
and checks in on it. The full guide is [Coach](../../docs/agents/ready-made/coach.md) in
Local Link's docs.

## Who Coach Is For

Coach is for one person training toward a goal who wants a plan that
fits their week and a coach who remembers. Coach keeps your history in
fixed files in its files folder, CSV logs that open in any spreadsheet,
and writes only what you said or what a skill computed from it. Coach is
not a doctor: pain, dizziness, or illness stops the session, and Coach
plans no extreme diet.

## What Coach Comes With

Coach runs on Link Harness and stays awake, so its schedules fire
without you. Its model is Link's default, `claude-haiku-5-5` on
Anthropic, the current Haiku. Its persona is calm, upbeat, and
straight.

- **Tools:** file for its logs, plans, and profile, and scheduler for its evening check-ins and Sunday review.
- **Skills:** `onboard`, `log-endurance`, `log-lift`, `log-team-session`, `log-food`, `log-body`, `plan-week`, `plan-endurance-block`, `plan-strength`, `team-sport`, `nutrition-targets`, `weekly-review`, and `check-in`.
- **Reference foods:** a table of common foods per 100 g beside `log-food`, rounded from USDA FoodData Central.

## Bring Coach In

Download `link-coach.tar.gz` from Local Link's
[latest release](https://github.com/so-local/link-releases/releases/latest), then import it from the folder you saved it in.
Import checks the whole
archive, asks whether to keep the name `coach`, and places the agent in
`~/Link/Agents/coach/`, with its settings in `~/Link/Safe/Agents/coach/`.
Nothing is installed, connected, or started until you say so.

```sh
lnk archive import link-coach.tar.gz
```

From a checkout of Local Link's repository, pack this folder as the
release does, then import `~/Link/Archive/coach.tar.gz`.

```sh
lnk archive export --from agents/ready-made/coach
```

## Start Coach and Chat

Start Coach and say hello. The first conversation is onboarding: your
goals, the days you train, your limits, how you eat, and your time zone,
after which Coach creates its check-in schedules. Then log what you do
in your own words, such as `Ran 10 km easy in 58 minutes`. Connect
Telegram or another app to text Coach from your phone.

```sh
lnk agent start --agent coach
lnk agent chat --agent coach
lnk agent connect --agent coach telegram|slack|discord|signal
```

## What Coach Can Reach

Coach's sandbox writes only its files folder and its own instructions
and skills, and reaches its hosted model through its proxy. Coach runs
no program and needs only its model, so its hosts can shrink to its
provider's API.

```toml
# ~/Link/Safe/Agents/coach/sandbox.toml
schema = 1

[files]
write = [".", "instructions", "skills"]

[internet]
hosts = ["*"]
```

```sh
lnk agent permission remove --agent coach --internet.hosts '*'
lnk agent permission add --agent coach --internet.hosts api.anthropic.com
```


## Make Coach Your Own

**Coach becomes your coach.** An imported agent is yours. Edit its standing instructions in
`~/Link/Agents/coach/agent/instructions/instruction.md`, or ask the agent
to, since its sandbox writes them. Add or remove skills and tools, and
change one key of its specification with a field flag on
`lnk agent start`, which the agent keeps.

```sh
lnk agent skill list --agent coach
lnk agent tool add memory --agent coach
lnk agent spec show coach
```

Coach's tone is its `persona.md`, and a new sport is a new skill, with its log added to the Home's `README.md` first.
[Make an Agent Your Own](../../docs/agents/ready-made/README.md#make-an-agent-your-own)
covers each change, and
[Share an Agent](../../../modules/agents/agent/docs/agents/share/export.md#share-your-agent)
shows how to export your version for others.

## This Folder

This folder is an archive kept as a folder. `archive.toml` names the
agent and lays out its folders: `Agents/coach/` holds its instructions,
skills, and Home, and `Safe/Agents/coach/` holds its `agent.toml` and
`sandbox.toml`. This README sits beside `archive.toml`, travels in the
released archive, and is left out on import, so the agent's files stay
exactly what the archive names. The Home starts blank, with headings and
empty fields only, and fills as you talk to the agent.

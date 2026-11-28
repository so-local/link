# Import an Agent

Import an agent a friend, your community, or Local Link made, and start it as your own. Import checks every file first, gives the agent an ID of its own, and installs, connects, and starts nothing.

```sh
lnk archive import link-coach.tar.gz
lnk agent start --agent coach
```

## Choose the Agent's Name

[`lnk archive import <path>`](../../../../../tooling/archive/docs/tooling/archive/README.md#import)
checks the whole archive, then tells you the agent's name and asks
whether to rename it. It moves the agent into place under the name you
chose, as a new agent, and tells you the command that starts it.

```text
This agent is named coach. Rename it? [y/N] y
Name: my-coach
✓ Imported my-coach.
```

### Rename When a Name Is Taken

When one of your agents already has the name, import says so and asks
for another, so the clash is settled in the same question and your agent
is never touched. Without a terminal, a name already taken refuses the
import, and `--select agent.coach=agent.my-coach` names it instead.

## What the Sandbox File Opens

An agent's sandbox file comes with it, so it runs with what its author
granted. Import says what that file opens beyond Link's own, such as the
whole network, before you start it, and `lnk agent permission remove --agent
<name>` takes each back.

## What an Agent Needs

An agent's requirements are its model provider's credential, its
connections and Link's tools added on each machine, such as its web
tool. Import prints the setup command of each one its files say it's
missing, and `lnk agent start` names whatever is still missing.

| Requirement | Set up with |
|---|---|
| Model provider's credential | asked for by `lnk agent start` |
| Connection, such as Telegram | `lnk agent connect telegram` |
| Link's web, browser or desktop tool | `lnk agent tool add web` |

## Home Outside the Archive

A Home given with `--run.files` is your folder outside `~/Link/Agents`, so it
isn't in the archive. Import says so, and the agent's Home is then its
folder in `~/Link/Agents/<name>/home` until `lnk agent start --run.files`
names another.

<!-- docs-tree:start -->
<!-- docs-tree:end -->

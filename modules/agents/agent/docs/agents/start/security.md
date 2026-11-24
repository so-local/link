# Security

How shared bridges and idle-agent processes retain each agent's identity and isolation.

## One bridge for every agent

Off by default. With `shared_bridge = true` in `~/.lnk/safe/agents/policy.toml`,
one bridge (`lnk agent bridges`) carries every agent's connections and
messages from others on the machine, instead of a service each, and one
scheduler runner fires every agent's jobs in a time zone:

- It runs outside every sandbox, as each agent's own bridge does, and
  holds what each held: every agent's endpoint key and connection tokens,
  read from each agent's settings (600). A fault in it stops every
  agent's connections until it starts again. As a service, the machine's
  service manager starts it again. With agents in a terminal, each
  agent's start does, within a few seconds.
- Each agent keeps its own way in, endpoint and key, its connections'
  bridges (`lnk-<connection> bridge`, one each), the answers it owes and
  its scheduler's jobs and caps. A message at one agent's way in is
  asked of that agent's harness alone, with its key, and its answer
  goes only to that agent's chats. A test shows neither agent's words
  reach the other's conversations or bot.
- An agent started again is carried again on its own, and the others
  are left as they are. A turn cut short when the bridge stops is
  finished by its harness, and its answer delivered once when the
  bridge is back, as after any restart of a bridge.
- Whom it carries is in `~/.lnk/safe/agents/bridges` (700), which every
  sandbox hides: a record for each agent, written at its start and
  removed at its stop. With no agent to carry for 20 seconds, it ends.

## An idle agent

Where one bridge carries every agent, an agent stops when it's had
nothing to do for its specification's `[run] idle_after` (Link's `5m`): no
message, no answer still coming however long it pauses, no turn running
and no background command. It then holds no process, memory or port of
its own, and its next message starts it again:

- On Linux, for an agent sandboxed behind its proxy, the bridge listens
  on the agent's endpoint port in its proxy's place, awake or asleep,
  and passes each connection into the agent's sandbox through its
  proxy's folder (`in.sock`, in a 700 folder), as the proxy would:
  never through a link, and only to a socket of the user's own, since
  the sandbox writes that folder.
- Anywhere else (a Mac, where the harness listens on the loopback
  itself, or an agent with no proxy), the bridge listens on the port
  only while the agent sleeps. At the first connection it lets go of
  the port, starts the agent, and passes that connection on once the
  harness listens there. A connection's connection in that moment is
  refused and tried again for a few seconds.
- Whether the agent is awake or asleep, the bridge passes bytes and holds
  nothing: the endpoint
  still proves it's the harness, with the agent's own key, before it's
  sent the key or a message, and the key is never the bridge's to check.
  Every sandbox, proxy and grant is the agent's own as at any start.
- A connection to a sleeping agent starts its job first: its service,
  or its start's program in a terminal.
- A background command running holds its record's lock, and keeps its
  agent awake, so its supervisor, under the agent's own job, isn't
  stopped. A turn running (Link Harness's `turn.json`) does too, and so
  does any connection open to a harness holding its own port. The
  answers of turns nobody took are asked for when a turn ends and as it
  wakes.
- Where an owner's agents share one harness, a sleeping agent stays one
  it serves, so its stopping and waking cut no other agent's turn, and
  with every one of them asleep, that harness ends too.
- A harness that speaks one of the agent's connections itself never
  sleeps: it would stop hearing it.
- `~/.lnk/safe/agents/bridges/asleep/<agent id>` marks an agent asleep. With
  `idle_after = "never"`, or the shared bridge off, an agent runs as
  before.

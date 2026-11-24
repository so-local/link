# Security

How messages from other agents retain their source and authority when delivered to your agent.

## Messages from others

How a message from a schedule, a subagent or a teammate is told from
yours.

- A message from a schedule or a subagent reaches Link Harness with
  `Link-From`, is kept as from them, and the model is told who's
  speaking, with each line of it quoted, so no paragraph of it reads as
  yours: it's never taken as yours. A `Link-From` naming no sender is
  refused.
- A stranger's message (one from neither the owner's own schedules,
  subagents and commands nor someone the owner let in: a webhook's) is
  given only the tools its specification's `[tools] strangers` names, none by
  default, and always says so, so its conversation never keeps more. A
  harness that can't narrow a conversation's tools never takes one.
- It goes only to its own conversation, named as its sender, and is
  answered nowhere, so it never reads or narrows another and nothing it
  says reaches the owner's. Only the agent’s own specification sets `strangers`.
- Through `file`, a conversation reads only its own folder of `chats/`,
  never another's, and a stranger's reads none, even when its specification
  grants it `file`. The tool's server checks each call against the
  conversation and standing the harness names, and refuses a call that
  names neither.
- Stranger grants refuse the `task`, `scheduler`, `run`, and `code`
  namespaces, and `agent__send`, `file__definition`, `file__references`,
  and `file__diagnostics`. Those actions can run programs or send work
  as the agent, losing the stranger’s standing. Ordinary file reading
  and editing and `agent__show` remain eligible for explicit grants.
- Every such message carries a hop count; the scheduler refuses one
  past the agent's cap (`[tool.scheduler] hops`, Link's 8). Hops bound
  how deep a chain goes, not how many messages it sends (each firing of
  a recurring job starts again): that's the cap a day
  (`[tool.scheduler] per_day`), with the cap on subagents at once, so
  agents can't wake each other for ever.
- The scheduler runs as a tool, outside the agent's sandbox, confined to
  writing its jobs (`~/Link/Agents/<agent>/agent/schedule`). Which
  conversation a call is made in, and its hops, come from the harness
  (`_meta`), never from what the model writes, and a call lists and
  cancels only that conversation's jobs and its subagents'. The
  scheduler trusts what the harness sends:
- Link Harness doesn't run what the model writes itself, and `run`'s
  commands reach no tool, but a harness that does (a shell) could name
  another conversation or reset its hops, bounded then by the day's cap
  alone.
- A subagent has its parent's tools or fewer, never more. The scheduler
  takes a list only of the tools the harness says the parent's
  conversation has (`_meta` `link.local/tools`), and, given none, the
  parent's whole list; the harness keeps the list with the subagent's
  conversation and narrows it again at each list, never widens it.
- A harness that can't keep one isn't sent one: the bridge and the
  scheduler refuse it rather than send a subagent every tool it was
  meant not to have. The list bounds what the model is offered and
  calls; what each tool may do is still its grants'.
- A subagent runs on the model it was started with, else its parent
  conversation's (`_meta` `link.local/model`), only ever one of the
  specification's `[model] allow`: Link Harness refuses another, and the model
  proxy names only those it was granted.
- The bridge's way in is a Unix socket with the agent's settings (600),
  which only you reach, and each message at it is [a call proved as the
  agent](../more/interface/security.md#every-way-out-of-its-sandbox) whose scheduler or `run` hands it.
- An answer goes to an allowed user’s own chat on Telegram or Signal,
  the owner’s recorded direct chat with the bot on Slack or Discord,
  or a group an allowed user talked to the agent in. The bridge keeps
  recognized groups in the connection’s private folder and refuses
  group delivery with `groups = "never"`.
- Signal sends through the running bridge's `send.sock` in that folder
  (600), which takes a message only proved as its own agent's, with the
  key the machine's keyring holds for it still: a bridge outliving `lnk
  agent stop` takes nothing more. A Slack app or Discord bot connected
  before Link kept that sends no such answer until `lnk agent connect`
  again.

# Decisions

Where Link's tools live, why they are named and shared as they are, how
a team's tool runs once for the team, and how a coordinator's delegated
work is kept.

## Where do Link's tools live?

In a folder of their own, so any harness uses them and none can reach
into a tool: a tool there knows no harness, and reads what it reads of
an agent through a documented format, such as the conversation folder's.
Each is a program of its own (`lnk-<tool>-tool`), as each module is,
rather than one program for all of them. Link's release builds them and
Link Harness from their folders at the core's version and signs them
with Link's key, as it does the adapters.

## Why does each tool's name end in `-tool`?

So the plain name stays free for what isn't an agent's tool. A browser,
a code index or a web client is useful to programs that have no agent,
and a package for one of them would want the plain name: `browser` for
a package that drives Chromium, `browser-tool` for the module that gives
an agent a browser. The specification keeps the short names, so an agent's
`[tool.web]` is served by the `web-tool` module, as `[tool.file]` is by
`file-tool`. The store drivers follow the same rule, with `sqlite-store`
beside a `sqlite` that could only talk to SQLite.

## Why does each tool have a contract?

Because the agent module starts every tool, and the command line it
writes must be the one the tool reads. The model's side needs no
contract: the Model Context Protocol (MCP) is the open protocol through
which a model's host lists a server's tools and calls them, every tool
here is an MCP server over standard input and output, and any harness
that speaks MCP reaches any of them.

### What a tool's contract holds

Each of the eight tools has a contract crate beside it, generated from
the tool's `actions.toml`: its runtime arguments as types and the
command line each one's `argv` writes, which the tool's own parser
reads back. The agent module builds a tool's command line through that
crate, so a flag can't mean one thing to the caller and another to the
tool. Some contracts hold more: the memory tool's says how the agent
module starts a memory engine, and the scheduler tool's says which
flags and caps it runs with.

## Where are a tool's calls declared?

Each of Link's tools declares the calls a model makes in its own
`actions.toml`, as actions offered on `tool`, each with a `tool` table
naming its call. Its build renders them as a template, and the tool
fills that template with what it knows at runtime, such as the programs
it may start or whether search has a key. A call's arguments, the
arguments its title shows and its annotations then have [one home](../../../../../../../packages/actions/docs/more/developing/modules/catalog.md#tools), and a
model's provider never sees a keyword that is Link's alone.

## Why do the tools take the calls models are trained on?

A tool is only as good as the model calling it, so each call takes the
names, arguments and intuitions of the tools current models are trained
on, as closely as Link's contract allows: file's `read`, `write`,
`edit`, `glob` and `grep`; run's shell `command`; desktop's
`coordinate`; the browser's `accept`; the scheduler's `cron` calls.
Link's own machinery, its sandbox, asks and caps, fits around that
shape, never the reverse. An argument declared with dashes is called
with underscores, as models expect, so the declarations keep one
naming rule and the calls keep the trained one.

## Team tools

How a team's tool runs once for the team.

### How does a team's tool run once for the team?

As a shared tool, added with [`lnk sandbox tool add --shared`](../../../../../../security/sandbox/docs/security/sandbox/reach.md#call-a-tool-that-runs-outside): one server for every
session of every sandbox that names it, where any other tool starts a
server for each. The first member's proxy to need it starts its hub,
outside every sandbox, which runs the server once, answers each session
from it, and stops once none has been open for a while.

#### A team's memory

A team's memory is the first, once its owner turns it on: Link Memory
over the team's folder, its index with the team's, its opinions the
team's specification's over Link's,
offered only to members granted all of the team's folder, as `team-<team
id>-memory`: it searches its `memory/`, so a member granted only `drafts`
would otherwise read the rest through it. A dozen members run one
memory, not a dozen. The mechanism is the sandbox's, so any tool meant
for many sessions can be shared, and members still ask their own proxy's
rules before a call reaches it.

## Delegation

The [scheduler’s decisions](../../../../scheduler-tool/docs/agents/tools/scheduler/more/decisions.md) explain
why delegated work uses durable jobs and how dependencies stay scoped to
the conversation that created them.

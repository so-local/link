# Stable IDs

> Names That Never Change

`link-id` names a thing by what never changes, `<kind>:<id>`, with one
generic every kind of ID rides. Who acts, what runs and one run of it
are three IDs of one grammar, so a module that measures, places or
grants never needs to know which program made them.

## Three IDs

One agent is all three:

- **Principal:** who acts, or may: a user or a service, never
  anything else. An agent acts as the service with its own ID,
  `service:<its ID>`, which holds roles. [Principal](../../../../../principal/docs/more/libraries/types/principal.md) owns
  those two kinds on this generic, so callers share their grammar.
- **Workload:** what runs, kept across its runs. The workload
  `agent:<its ID>` is what's measured and what a box carries. A
  workload's kind is open: the module that keeps it names it, such as
  `agent` or `job`, so this package names none.
- **Execution:** one run of a workload, `<workload>@<run>`, such as
  `agent:a1b2c3d4e5f6@k3m4n5p6q7r8`. Its run is allocated before the
  run starts and kept with its outcome, never a process's pid, which
  is reused.

## Resources

A resource is what's acted on, such as a box, a bucket, a team or a
machine: `Resource`, whose kind is open like a workload's, named by the
module that keeps it. A box is `box:<its installation ID>` and a team's
group `group:<its ID>`. A resource is a grant's target, never who acts.

## The generic

`Id<K>` is `<kind>:<id>`, a kind of `K`'s, then an id Link minted:
`src/limits.toml`'s `candidate_length`, 12, of its lowercase letters and
digits (`is_id`). A thing whose own ID comes from outside Link gets a
minted ID, with its outside ID a field beside it. `Id::kind` and
`Id::id` give back each part.

### Closed and Open Kinds

A set of kinds implements `Kind`, which parses a kind, writes it back
and says what an ID of them is called in an error. A principal's kinds
are a closed list, `user` and `service`, which the account service's
API fixes. A workload's and a resource's are open: any name of lowercase letters, digits and `-`, from a letter,
at most `KIND_MAX`, 30, bytes (`is_kind_name`).

### Made only by parsing

An ID is made only by parsing or by `Id::new` from a kind and an id
checked alike, so every one is well formed. What isn't one is `NotOne`,
saying why. Each is written as its string, in JSON and TOML alike.

## An execution's run

An execution's run is 1 to `RUN_MAX`, 64, bytes: ASCII letters, digits
and `RUN_PUNCTUATION`, `._-`. Neither a run nor an id holds `@`, so
`<workload>@<run>` splits only one way.

## Parents

An execution's parent is the execution that started it, such as an
agent's start for each tool it runs. `Lineage` keeps an execution with
its parent and its depth, the number of parents above it. The parent is
a field beside the execution, never part of its ID, so nesting a run
changes no ID. An execution is never its own parent, and a chain ends at
`parent_depth`, 16 parents, from `src/limits.toml`. A lineage read from
JSON or TOML is checked the same way.

## Minting

An ID is minted once, by the module that owns its kind, and never names
another thing, even after what it named is deleted. A grant, a log line
or a measure naming it never comes to mean something else. [Why IDs
work this way](id/decisions.md) has the reasons.

### Candidates

`candidate` draws a new ID's id at random: 12 lowercase letters and
digits, each as likely as another, which fit a file name, a service
name, a cloud label and a bucket name. Its length and alphabet are in
`src/limits.toml`. `is_candidate` says whether an id has that shape. A
candidate is only a proposal, since two draws may meet.

### Reservations

An owner keeps a ledger of the IDs it gave, through the `Ledger` trait.
Each ID is live while what it names exists and retired once that is
deleted.

#### Reserve and Retire

Holding its own lock, the owner calls `reserve`, which draws
candidates until one is neither live nor retired and holds it. On delete
the owner retires the ID, which then stays out of reach of anything new.
`Table::revive` takes it back only for the very thing it named, brought
back whole, such as an agent restored from its backup. A
ledger that refuses `reserve_attempts`, 32, candidates in a row is
broken, and `reserve` says so rather than drawing forever.

### Arrivals

An ID that arrives from elsewhere, such as a copy from another machine,
goes through `admit`, which refuses one that is live or retired and
holds any other. `Table` is a ledger as two sets, `live` and `retired`,
which an owner keeps in its own state.

## In Output

Every `--json` field and log line writes an ID as `<kind>:<id>`, under
a field naming its axis: `principal`, `workload`, `execution`,
`parent` or `resource`, from `src/output.toml`
(`axes`, `is_axis`). A list inherits its field's name, so an agent's
teams are `"teams": [{"resource": "group:<id>"}]`, never a bare ID or
one under `id`. A reader takes an ID back with `candidate_of`, which
answers only for the kind it expects. The testing kit's
`ids::assert_named` holds every command's output to this.

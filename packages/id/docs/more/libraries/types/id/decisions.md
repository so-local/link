# Stable ID Decisions

Why Link names things by minted IDs, and why every ID keeps one form
wherever it's written.

## Naming

Why a thing is named by an ID it keeps for good.

### Why an ID and not a name?

A name is a label you choose and may change or reuse, so a grant, a log
line or a measure naming it could come to mean another thing. An ID is
minted once, when its thing is created, and kept through a rename and a
move. What people see, such as a folder, a settings file and a log, stays
by its name, and the ID is inside it.

### Why is a name never told from an ID by its shape?

A lookup takes a name or an ID and tries each as what it is. Telling
them apart by how they look would make a name that happens to look like
an ID point at the wrong thing, so the `<kind>:` prefix, not the shape
of what follows, says that a value is an ID.

### Why is a deleted thing's ID retired, not freed?

An ID that came back for something new would let an old grant or log
line name it. So a deleted thing's ID stays retired, and a candidate is
refused when it is live or retired. The one way back is a restore of the
very thing it named, which keeps every old reference true.

### Why does a thing from outside get an ID of Link's?

An outside ID has the outside system's shape and lifetime, which Link
can't hold to its rules. A minted ID beside it keeps every ID in one
grammar, with the outside one a field for whoever needs it.

## Axes

Why every ID says which of four questions it answers.

### Why does the field say what an ID names?

Who acts, what runs, one run and what's acted on are different
questions, and the same thing can be several: a workload can act as a
service. The field an ID sits under, `principal`,
`workload`, `execution`, `parent` or `resource`, says which, so a reader
never infers it from the kind.

### Why does a grant name a principal, never a workload?

A grant says who may act, so its first question for anything that runs
is who acts for it: you, or a service. A workload's ID and an
execution's correlate a run's logs and measures and are never a grant's
target, so starting a run again changes no one's access.

### Why is a parent beside the execution, not in it?

Nesting a run would otherwise change its ID, and a chain of parents
would grow every ID below it. Kept as a field, the parent links runs
without touching how any one of them is named.

### Why one form in output?

Every `--json` field and log line, and saved state with them, writes an
ID as `<kind>:<id>`, so a reader parses one grammar and a later release
reads what an earlier one wrote with no conversion.

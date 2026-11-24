# Actions File

`actions.toml` declares each thing a module does, keyed by the action's
ID: its command, its runtime arguments and the specification it reads, with the
shape it answers following from its resource and verb.

```toml
# actions.toml
schema = 1

[actions."env.approval.add"]
description = "Say yes, ahead of a run, to giving the environment file a secret it asks for"
role = "secondary"
docs = "/services/environments/approval#add-an-approval"
next = ["env.approval.list"]

[actions."env.approval.add".args]
secret = { type = "string", description = "The secret, by its name or its ID", required = true, x-link-cli = { position = 1 } }
spec = { type = "string", x-link-path = true, description = "The environment file (env.toml) that asks for it", required = true }
```

## Action IDs

An action is named whole as `<namespace>.<resource>.<verb>`. The
namespace is the module's ID from its [module file](module.md), the
resource is a shape in its [types file](types.md) or the namespace's own
name, and the verb is its last part. The command comes from the ID,
`lnk <namespace> <resource> <verb>`, with the resource left out where it
is the namespace's own name. So `env.env.create` is [`lnk env create`](../../../../../../../modules/compute/env/docs/services/environments/README.md#create-a-box) and
`env.approval.add` is [`lnk env approval add`](../../../../../../../modules/compute/env/docs/services/environments/approval/README.md#add-an-approval).

### Programs

A namespace's program is `lnk-<namespace>`, and `lnk <namespace> <resource>
<verb>` runs `lnk-<namespace> <resource> <verb>`, so a caller may run the
program directly. A program never sees its own namespace word, and a resource
named as the namespace collapses: `web-tool.web-tool.serve` is `lnk-web-tool
serve`, typed as `lnk web-tool serve`.

### Core's Namespace

Core is a namespace like any other, and its program is `lnk` itself. So
`core.module.add` is [`lnk mod add`](../../../../../../../docs/more/modules/guide.md#add-or-remove-a-module). Core declares its own verbs under
`[verbs]`, such as `doctor`, `up`, and `upgrade`, and its program's
flags under [`[program]`](#program-flags).

### Verbs

A verb is one of Link's fixed verbs wherever its meaning applies, each
pair listed on the [Catalog](../catalog.md#verbs). A module declares
each other verb it uses under `[verbs]`, with what it means and its
effect, and every declared verb is used by one of its actions. A declared verb meaning a
fixed one, such as `new` for `create`, is refused with the verb to say
instead, from `format.toml`'s `[verbs.banned]`.

```toml
# actions.toml
schema = 1

[verbs]
send = { description = "Send a teammate a message", effect = "insert" }
```

### Effects

An effect is one word saying what an action does to state: `find`
reads and changes nothing, `insert` makes a new thing, `update` changes
or sets a thing that exists, and `delete` removes one. Each verb names
its effect once, a fixed verb's in `format.toml` and a module's own in
`[verbs]`, so an action takes its effect from its verb and never says
it again. A verb with no effect is refused. The fixed verbs' effects:

- **`find`:** `list`, `show`, `get`, and `check`.
- **`insert`:** `create`, `add`, `connect`, `install`, and `run`.
- **`update`:** `set`, `start`, and `stop`.
- **`delete`:** `delete`, `remove`, `disconnect`, and `uninstall`.

Every surface derives what it says of an action from its effect: a
command tool whether it only reads, [an MCP call its hints](tools.md#annotations), and [HTTP its method and where its inputs
go](transport.md).

## Fields

An action's table holds its help and how it's offered. Every key but
`description` may be left out:

- **`description`:** its help, the first paragraph the short help and
  the rest shown with `--help`.
- **`args`:** its [runtime arguments](#runtime-arguments), by name.
- **`spec`:** the one specification kind it reads, by its name.
- **`output`:** [the shape it answers](#outputs), where its verb implies none or
  another.
- **`stdin`:** [the shape it reads on its standard input](#secrets-on-standard-input), when it reads
  one.
- **`role`:** `primary`, `secondary` (the default) or `plumbing`, which
  says where help lists it. A plumbing action is hidden and offered on
  the command line alone.
- **`surfaces`:** [where it's offered](#surfaces).
- **`next` and `fail`:** the action IDs it suggests after success and
  after failure, which become Next's graph.
- **`checks`:** named hooks, functions of the crate's `checks` module,
  run after the declared checks pass.
- **`docs`:** the guide section a person reads about it, as
  `/<address>#<anchor>`.
- **`default`:** `true` [where its group, typed bare, runs it](#group-defaults).
- **`stream`:** `true` [where its answer arrives as it's made](transport.md#streams), its output
  the union of its events, told apart by a tag.
- **`keeps`:** on a `find`, [how long its answer keeps](transport.md#caching) and whether it's
  every caller's, as `{ for = "1h", shared = true }`.
- **`rest-args`:** [the arguments the REST API accepts](transport.md#inputs), where it accepts
  fewer than every one but those saying where or how a machine runs it,
  `format.toml`'s `[rest] never`.
- **`control`:** `true` where it reads or writes a control file, which
  steers Local Link. A control action is never a model's tool, only its
  owner, a program, or a trigger calls its route, and no role a module ships holds it. `Catalog::is_control`
  answers whether an action is one, for a check refusing to give it to a run.
- **`route`** and **`tool`:** [how a bridge offers it](#routes) and [how a model calls it](tools.md).
- **`diagnoses`:** on a `show` action, the kind of thing it diagnoses,
  such as `box` or `secret`. Given one by name, its `--json` is that
  thing's state, which [`lnk issue create`](../../../../../../../docs/more/modules/guide.md#get-help) puts in a bug report.
- **`next-modules`:** `true` on an action of core's leading into every
  module, as [`lnk up`](../../../../../../../docs/more/modules/install.md#pick-your-modules) does.

### Reserved Fields

An action's `resource`, the argument naming what it acts on, and the module's
`[roles]`, the action collections it comes with by name, are kept for access
control. The catalog reads them and checks their shape, and `Catalog::on` and
`Catalog::role` answer from them, but no command acts on them yet.

## Runtime Arguments

Each entry of `args` is a runtime argument, about this one invocation,
such as a confirmation, an output switch, a resource selector or the
`--spec` file. Its name is its flag, and it never holds a dot. Its value
is a JSON Schema in the [types file's](types.md#subset) subset, or a
shape's name, with `description` its help and `required = true` where it
must be given, as an OpenAPI parameter is written. An argument is a
scalar, a choice or a list of them, and an object written inline is
refused.

### Command-Line Keys

Its `x-link-cli` table says how the command line takes it:

- **`position`:** a positional argument, at this place from 1.
- **`short`:** its one-letter flag.
- **`rest`:** every argument after the others, a list of text passed on
  unparsed. With **`last`**, it's taken only after `--`, so a mistyped
  flag before it is refused rather than handed on.
- **`env`:** the variable read when the flag is absent, defined in a
  [variables file](variables.md#flags-and-specifications). A switch reads none.
- **`shared`:** one of core's [shared arguments](#shared-arguments).
- **`default-missing`:** the value the flag alone means, so its value is
  optional, as `--sandbox` alone is `--sandbox own`.
- **`conflicts`** and **`requires`:** the arguments of the same action
  it can't be given with, or is only given with.
- **`hidden`:** kept out of help, for an argument Link passes and a
  person doesn't.

### Defaults

A text, a number or a choice may declare a `default`, the value its
absence means, as the scheduler's `driver` declares `default =
"service"`. Its generated accessor then gives that value, its help shows
it, and every surface reads the same default. A required argument, a
switch or a list has none, and a default its own shape refuses is
refused when the module builds.

### Positionals

Only a runtime argument is positional. The argument a person nearly
always gives, such as the cloud in [`lnk box create aws`](../../../../../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box), is its command's positional. Its flag,
where it keeps one, is the optional strict form of the same argument. A
switch is never positional and never required, and a positional has no
short flag.

### Shared Arguments

Core declares the arguments many modules take in `link-actions`'
`shared.toml`, each once with its help, and an action takes one by naming it,
as `json = { x-link-cli = { shared = "json" } }`. They are `json`, `yes`,
`quiet`, `request`, `cloud`, `owner`, `account` and `remote`. A shared
argument says nothing else, so its help is the same everywhere.

### Shared Groups

A table of `shared.toml` with `args` is a group, taken whole by an
argument of its name. `workplace = { x-link-cli = { shared = "workplace"
} }` gives `--on`, `--cloud`, `--zone`, `--here` and the `place`
positional after the action's own positionals, short for `--cloud
<cloud>` or `--here`.

### Argument Groups

Arguments several of a module's actions take together are declared
once, in `[groups.<name>]`, and an action takes them all by an argument
naming the group. [A group's arguments](../../../../../build/docs/more/developing/modules/catalog/generation.md#argument-groups) are optional flags on every
surface, and its generated arguments hold them as one Rust type, the
same in every action, so code handling them takes one type.

```toml
# actions.toml
schema = 1

[groups.flags]
description = "How the scheduler's runner is started"
args.driver = { enum = ["service", "cron", "address"], default = "service", description = "What wakes the runner" }

[actions."scheduler-tool.cron.install"]
description = "Have the machine's calendar start the runner"

[actions."scheduler-tool.cron.install".args]
flags = { group = "flags" }
```

### Arguments Every Action Takes

A runtime argument every action of the module takes, such as core's
`--json`, is declared once in a top-level `[args]` table.
Each action has it on every surface, and on the command line it goes
before or after the command's words. It is an optional flag, never
positional, and an action declaring the same name is refused.

```toml
# actions.toml
schema = 1

[args]
json = { x-link-cli = { shared = "json" } }
```

### Leaving an Argument Out

An action it doesn't fit writes it `false` in its own `args`, as
`json = false` for an action with no JSON answer. That action then has no
such argument on any surface, and the argument's accessor answers none
for it. A `false` naming no argument of `[args]` is refused.

### Program Flags

`[program]` declares what the program does given no command, as `lnk
--version` does, with an optional `description` and its flags in
`[program.args]`. Its flags are optional and never positional, and none
shares a name with `[args]`. A program declaring `version` gives its
own, and every other program has the version flag core's generator
gives it. A program declaring `help` renders its own help, as `lnk
--help` does, while every command under it keeps clap's `--help`.

```toml
# actions.toml
schema = 1

[program.args]
version = { type = "boolean", description = "Print Link's installed version.", x-link-cli = { short = "V" } }
```

## Group Defaults

An action with `default = true` is what its group runs when typed with
no command after it, reading the action's flags there. With
`core.module.list` its group's default, [`lnk module`](../../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) runs `lnk module
list`. A group has at most one default, and it's on the command
line. A plumbing action is a default only in a group of plumbing alone,
which Link runs bare and help hides. The default's flags are its own, so
given before another command of the group, as `lnk <group> --<flag>
<command>`, they're refused, never dropped.

## Aliases

An alias is a short word a person may type in place of a group's own,
as [`lnk mod`](../../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) for `lnk module`. A module declares
each of its aliases once, in `[aliases]`, keyed by the group's words as
typed after `lnk`. The group is a namespace or a resource with commands
under it, and the alias stands for its last word.

```toml
# actions.toml
schema = 1

[aliases]
manual = "man"
```

The command line takes the alias as the group, its help and the
`lnk --help` screen show the full command first with the alias beside
it, shell completion completes both, and the command reference lists
each alias. Core resolves a namespace's alias to its module from the
registry, so [`lnk man`](../../../../../../../modules/tooling/manual/docs/tooling/manual/README.md#manual) runs `lnk-manual`. An alias is one lowercase
word, never the group's own word or another word typed in the same
place, and no two of the first words after `lnk`, every module's
namespace, core's groups and their aliases, are the same.

### Short Forms, Never Former Names

An alias is a short form offered beside the full command, never a former
name kept working: a retired name belongs in its owner's `history.toml`,
as [History](../../../../../../spec/docs/more/libraries/types/specifications.md#history) explains.

## Specifications and Outputs

`spec` binds the action to one specification kind by its name: its own, or
another module's as `sandbox.sandbox`, read through that module's
generated types. The kind's fields become field flags spelled as their
keys, nested fields by their dotted paths and top-level fields as
`--spec.<field>`. [A field flag overrides the field it names](../../../../../../../docs/more/developing/modules/specs.md) in the file
`--spec` reads.

### Files Read as Written

An action that reads a specification file as written, such as one whose yes is
kept for that very file, binds no specification. It takes the file as an
argument marked `x-link-path`, as `env.approval.add` takes `spec`, so it
has no field flags.

### Outputs

An action's output follows from its verb and its resource, the shape
its ID names, by `format.toml`'s `[verbs.fixed]`:

- **The resource:** `show`, `create`, `add`, `set`, `get`, `connect`,
  `install` and `start`.
- **A list of the resource:** `list`.
- **The resource's ID**, its `x-link-resource` property: `delete`,
  `remove`, `disconnect`, `uninstall` and `stop`.

### Named Outputs

`output` names another shape where an action answers something else,
and every action with a module's own verb, or whose resource is no
shape, names one. An action whose `--json` is what it printed answers
`core.report`, link-format's blocks in order and its exit code. An
action relaying a tool's MCP result as it is answers `core.mcp`. A
person sees an output formatted by its verb, a `list` as a table and a
`show` as fields, and a shape's `x-link-display` names columns or labels
where that doesn't suit.

### Secrets on Standard Input

An action that reads something on its standard input names that shape
in `stdin`, as `output` names what it answers. A value marked
`x-link-sensitive` is a secret, so an action whose output or `stdin`
shape reaches one prints or reads a secret. A recorder, such as a test
run's, keeps that stream out of what it saves by this declaration
alone:

```toml
# actions.toml
schema = 1
[actions."secret.secret.create"]
description = "Keep a secret"
role = "primary"
stdin = "value"
```

```toml
# types.toml
schema = 1
[value]
type = "string"
description = "A secret's value, read from the first line of stdin"
x-link-sensitive = true
```

## Routes

An action a caller reaches through a bridge, the part of Link that
calls a module for a program it serves and enforces who may call and
how much it may answer, declares a `route` table. A route's answer is the action's
[output](#outputs), so no route names a type of its own.

```toml
# actions.toml
schema = 1

[actions."note.note.show"]
description = "Show a note"

[actions."note.note.show".route]
bridge = "command"
callers = ["owner", "program"]
limits = { bytes = 1048576, time_ms = 10000 }
```

### Route Keys

A route says who serves it, who may call it and what it may answer:

- **`bridge`:** one of `format.toml`'s `[route] kinds`: `command`, a
  command run through `lnk` and offered over the Model Context Protocol
  (MCP) and as a command tool, `model`, the model module's, the one `[route]` names as running
  the command itself, or the bridge the calling program has of its own.
- **`callers`:** who may call it, any of `owner`, `program`, `trigger`,
  and the served program, which the bridge learns from authentication.
- **`limits`:** the most its answer may hold, `bytes`, and the longest
  it may take, `time_ms`, required unless its bridge runs the command itself.
- **`carries`:** the protocol it relays in place of a typed answer, as
  `mcp`.
- **`command`** and **`names`:** that the served program's chat offers
  it as a command, and that command's other names, each a slash and a name.

### Route Paths

A route's path and its chat command's name are its command's words with
the namespace, as `/_link/box/list` and `box:list`,
unless the route declares `path`, as a tool's `"browser click"`. A route
with its own path may name its own `docs` section too. Two routes never
share a path, and two chat commands never share a name. An action only
its route offers, as a command its bridge alone runs, says `surfaces =
[]`.

## Contracts

A contract is the protocol several modules answer alike, as every store
answers the store contract, so a caller calls whichever store it was
given the same way. A contract declares its protocol once, in its
contract crate's `actions.toml`, and each module implementing it lists
the actions it answers under `[implements]`, by the contract's ID.

```toml
# actions.toml
schema = 1

[implements]
store = ["store.store.contract", "store.store.call", "store.store.serve"]
```

### Implemented Actions

Each listed action becomes the module's own under its namespace, with
the contract's arguments, specification and output, so `store.store.serve`
implemented by `file-store` is `lnk-file-store serve`. A resource the
contract names keeps its name. The catalog refuses an implementer whose
`module.toml` doesn't depend on the contract, a listed action the
contract doesn't declare, or one the module also declares itself. The
contract's shapes stay the contract's, generated once in its crate.

## Given by Core

Core gives every module two kinds of command that `actions.toml` never
declares. `uninstall` reports and removes the parts a module keeps,
given only to a module whose `module.toml` declares `[removal]`, and
core declares its own. `spec show` prints each specification kind the module
declares: the kind named as the namespace is `lnk <namespace> spec
show`, and another kind has its own, `lnk <namespace> <kind> spec
show`. No action's resource is `spec`, and no action's command is the
start of another's.

### Core's Protocols

Two protocols sit outside a module's command space, each from its
[module file](module.md#protocols). A module listing `answers` takes
the hidden root flag `--link-answer <question>`, and one running a guest
service has the hidden command `guest-service`. Every module's namespace
is dispatched to its program, [`lnk env ...`](../../../../../../../modules/compute/env/docs/services/environments/README.md#environments) to `lnk-env`, since core's
parser reads any namespace it doesn't declare as another module's.

## Surfaces

An action is offered on the command line, the REST API, the app and as
a model's tool, each generated from this declaration. `surfaces` names a
narrower list of `cli`, `rest`, `app`, and `tool`, or [`service` alone](#service-routes), and a `plumbing`
action is offered on the command line alone. A list naming `app`
names `rest`, which the app's page calls. [An action whose list
leaves out `cli`](../../../../../build/docs/more/developing/modules/catalog/generation.md#actions-off-the-command-line), such as a tool a model calls, has no command and is
never a group's default, while its handler still answers its calls.

### Service Routes

`surfaces = ["service"]` declares an action a service's own route
serves to other principals, such as another service asking it for
something. It names `service` alone and is never a default: it makes
no command, no REST route, no page, and no tool. It's declared so the
service serving it decides each call as it decides any other, from
the caller's assignments, which name the action through a role, and
its module ships those roles beside it.

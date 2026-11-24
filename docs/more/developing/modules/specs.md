# Specifications and Command Lines

Specifications and command-line flags are two inputs to the same typed configuration. Every specification field has a flag spelled as its key in the file, and every flag without a dot is a runtime argument.

<!-- spec-example -->

## Specification File

A specification is a TOML file that configures one kind of thing, such as an agent, a box or a sandbox. This standalone agent specification selects a reasoning level and turns a tool off. The saved agent setup file keeps behavior beneath `[behavior]`, while a standalone specification passed to `--spec` has no such prefix.

```toml
kind = "agent"
schema = 1
version = 1

[model]
thinking = "medium"

[tool.code]
enabled = false
```

A file names its `schema`, so a later release reads it in the shape it
was written. An empty file names none and is refused, given to `--spec`
or on standard input (`--spec -`), alone or under flags. Only a specification made
from flags alone, with no file, is the current schema without saying so.

### Same Specification as Flags

Each line of the file has a flag that sets the same key to the same value. A consuming action reads the file named by `--spec`, then applies the flags over it.

```sh
lnk agent start --spec agent.toml --model.thinking high --tool.code.enabled=false
```

## Field Flags

A field flag is the field's TOML key path after `--`, with the key exactly as the file writes it. The specification kind's shape in `types.toml` generates these flags, so a field added to the declaration has its flag in the same build.

### Nested Fields

A field inside a table is its dotted path, such as `--model.thinking` for `[model] thinking` or `--files.write_dot_files` for `[files] write_dot_files`. Keys keep their file spelling, underscores included, so a flag can always be turned back into its field.

### Top-Level Fields

A key at the top of the file is its own flag, such as `--os` for `os` or `--env` for `env`. Only the specification's own properties take the `spec.` prefix: `--spec.name`, `--spec.version` and `--spec.extends`, what the specification is called, its version and what it starts from. No specification has a top-level table named `spec`, which would make that prefix ambiguous.

#### Runtime Flags

A flag whose first word is a top-level field is a field flag, unless the command's runtime flags claim it. No runtime flag shares a top-level field's name, which a check over every command's declaration holds, so [`lnk box create --name`](../../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box) keeps naming the box while its specification's own name is `--spec.name`. Help lists the field flags under their own heading, after the runtime flags.

#### Shorthands

A table whose type names a shorthand (`x-link-shorthand` in `types.toml`) also takes a bare value on its own flag, which sets that one key: `--harness openclaw` is `--harness.name openclaw`, while the file keeps `[harness] name = "openclaw"`. An inline table, such as `--harness '{ name = "hermes" }'`, still sets the whole table.

#### Kind and Schema

Two keys outside the configurable type say what a file is. `kind` names the specification kind, unique across Link, and `schema` is that kind's format number, a plain integer. Flags always describe the current kind and format, so `kind` and `schema` are reserved and take no flag, and neither relates to a configurable `version` field.

#### Owners

A file never names the module that owns its kind. Modules export the kinds they own, so a module renamed or a kind moved to another module changes no one's files. A reader that knows its kind accepts a file without `kind` and refuses one naming another kind, and printed specifications always write both keys.

### Values

A flag's value is the TOML value the file would hold for that key. Text is taken as a string wherever the field expects one, so `--model.thinking high` needs no quotes. Numbers, booleans, arrays and inline tables use TOML syntax, and a switch given alone, such as `--commands.developer_tools`, means `true`.

#### Text Forms

A value type with a short text form accepts that form in both inputs. A box's lifetime is `deadline = "4h"` in the file and `--deadline 4h` on the command line. No flag has a syntax of its own that the file lacks.

### Lists

Repeating a flag builds a list in the order given, so `--files.write src --files.write out` is `write = ["src", "out"]`. A list on the command line replaces the file's list. An empty list is written `--files.write=[]`.

### Maps With Free Keys

A table whose keys are chosen by the writer, such as environment variables or per-tool settings, is addressed through the key itself. `--variables.REGION eu-west-2` sets one entry and `--tool.code.enabled=false` sets one value inside one entry, leaving the rest of the file alone. A key that TOML would quote is quoted on the command line too, such as `--limits.'"social.post:k7m2.show"'.max_body 1MB`.

### Lists of Records

An array of tables, such as an environment's stages and services or an agent's schedules, has no stable name for its elements. Its flag takes one whole element as an inline table, repeated for each element, and replaces the file's array.

```sh
lnk env spec show --spec env.toml --service '{ name = "web", run = "./serve" }'
```

### Precedence

The given flags form one table over the file's table, key by key, and the result is read once by the specification's strict reader. A flag replaces the value at its key, a table given whole included, and an omitted flag leaves the file alone. Command-line defaults never enter that table, so only the specification kind's declaration supplies defaults.

#### Absence

An optional field has no flag form that removes the file's value, because a file cannot express removal either. Editing the file removes a value. An empty list is written `[]`, and a table is never empty, in a file or a flag, so the reader refuses `{}` from either.

## Inspect a Specification

Every specification kind has one read-only command, `lnk <module> spec show`, which prints the effective specification with each default written in. A kind a package owns, which has no command of its own, is shown by each module binding it, as `lnk <module> <kind> spec show`. Its sources are the same as a consuming action's, plus an existing resource.

```sh
lnk box spec show aws-1
```

### Sources

A named resource prints the specification that makes it again, with Link's choices pinned from what this machine and the cloud know. A `--spec FILE` prints that file read in the current format, and field flags apply over either source. Flags alone print the specification they make. Inspection never creates, starts or rewrites anything.

```sh
lnk box spec show --spec box.toml --machine.cpus 4
lnk agent spec show main
lnk sandbox spec show --files.write . --internet.hosts example.com
```

### Output

The answer is TOML with its `schema` line, which the owning reader accepts unchanged. `--json` prints the same specification as JSON. It contains reusable configuration only, never credentials or runtime arguments.

### Provenance and Identity

Normal `show` commands print a resource's state, identity, ownership and availability. Specification output keeps that observed metadata out of the configuration, and attributes each key in a TOML comment where the specification is assembled from layers, as an agent's is. Reading a printed specification back cannot claim an identity or another owner's authority.

## Change a Command or Specification

A module that renames or removes a command, a runtime flag, a variable or a specification field records the change as data, and shared code answers every old spelling. No module writes migration code. An old command line is refused with its replacement rather than run, so people and scripts move to the current spelling.

### History File

Each module keeps `history.toml` beside its `actions.toml`, in the one schema `link-spec` owns, and exports it in its description with the steps of every specification kind it binds. Core's own history also records renamed modules, which no longer exist to answer for themselves. Link 0.1 is the baseline every history starts from: a module records only what a release after 0.1 retired, and has no `history.toml` until it retires something. This example records a command and a variable a later release could retire.

```toml
# history.toml
[[gone]]
was = "agent show --settings"
now = "lnk agent spec show"
since = "0.3.0"
until = "0.6.0"

[[gone]]
was = "LNK_AGENT_STYLE"
why = "Set style in the agent spec."
since = "0.4.0"
until = "0.7.0"
```

### Runtime Changes

An old command, runtime flag or variable is matched against its module's history before the parser reports an unknown argument. Link prints the replacement and exits without running anything, so a script fails once with its fix in the message. Only syntax is matched, never a flag's value or anything after `--`.

### Specification Changes

A specification kind's steps live in its `schema.toml` beside its type, each from one schema number to the next: `rename` moves a field, `remove` drops one with a reason and `refuse` stops with an instruction. A rename moves the field and its flag together, because the flag is the key path. A change to what a value means is a `refuse` that says what to write, never code that rewrites the value.

### Old Files

Every read applies the steps in memory and names the conversion, as in "Read agent schema 1 as schema 2. [`lnk spec convert agent.toml`](../../../../modules/tooling/spec/docs/tooling/spec/README.md#convert-a-file) rewrites it." Files keep working across an upgrade while command lines are refused, because a file is kept and a command line is typed again.

### Conversion

`lnk spec convert FILE` rewrites a file in the current schema and is the only command that changes a specification file. It reads the file's `kind` and `schema`, finds the owner that exports that kind and applies its steps, so it needs no code from the owning module. It validates the result first, keeps comments, key order and permissions, and refuses a file that changed while it was being read.

```sh
lnk spec convert agent.toml
```

### Retirement

Every entry and step names the release that brought it, `since`, and the release that deletes it, `until`. A test fails once the current release reaches an entry's `until`, so history never accumulates. After that release the old spelling is an unknown argument, and a file in that schema is refused as unknown.

### Specification Module

`link-spec` is the package every module links to read, print and check specifications and histories in its own process. `lnk-spec` is a small module running the same code as commands across every installed module's exported description.

```sh
lnk spec list
lnk spec show agent
lnk spec convert agent.toml
```

#### Commands

[`lnk spec list`](../../../../modules/tooling/spec/docs/tooling/spec/README.md#list-the-kinds) names every specification kind, the module that owns it and its current schema. [`lnk spec show KIND`](../../../../modules/tooling/spec/docs/tooling/spec/README.md#show-a-kind) prints a kind's fields with their flags, the actions that consume it and its history, and `--json` prints its JSON Schema for editors and other programs.

## Supply a Specification

An action that consumes a specification takes the file as `--spec FILE` and its fields as field flags. The operation names the verb, so creating a box, starting an agent and running a sandbox each consume a specification.

```sh
lnk box create --spec box.toml --machine.cpus 4
```

### One Specification per Action

An action binds at most one specification kind. Another file a specification refers to, such as a box's environment, is a field of that specification, so it is set as `--env env.toml`. A file an action reads beside its specification, such as an agent's sandbox file, is a runtime argument of that action, `--sandbox sandbox.toml`.

### Existing Resources

Starting an existing box resumes it, and creation settings belong to the action that creates or reconfigures it. The action's binding names its specification kind, so nothing guesses the binding from a command's words or its verb.

### Source Context

A relative path in a file resolves from that file's folder, and a relative path in a flag resolves from the current folder. Both become absolute before the flags are applied. Unknown keys, invalid values and unsupported variants are refused with the same message from either input.

#### Bundle Paths

Environment bundle inputs use the declared `confined-path` format. They resolve from the same input origins, then must remain inside the chosen environment folder. The reader retains a child-relative identifier for transfer and refuses an override outside that folder. Guest-relative working directories remain opaque values interpreted by their owner.

## One Parser

Every program's command line is generated from its module's declarations, and one shared entry in `link-spec` reads it. No other code reads the program's arguments, so specification files, field flags, runtime flags, `LNK_` variables and retired spellings are read the same way in every program.

### Program Entry

A module's build [writes its parser](../../../../packages/actions/build/docs/more/developing/modules/catalog/generation.md#command-line) from `actions.toml` and `types.toml`. The build also writes `PROGRAM`, a `link_core::help::Declared` holding the commands' specification bindings, the namespace and Next's graph, and the program reads its command line with `PROGRAM.parse()`.

#### Shared Entry

That call and core's `lnk` both enter through `link_spec::entry`. The entry captures process arguments once, answers the hidden `nexts --json`, answers a retired spelling from the module's history, separates a bound specification's field flags, and parses runtime arguments with the generated parser. Help and version come from the same declarations. Hidden operations such as uninstall, service supervision and recorder modes dispatch only after parsing. Output is chosen from the parsed value.

### Argument Categories

Every argument is either a specification field or a runtime argument, and its spelling shows which. Field flags contain a dot and come only from the action's specification binding. Every other argument is a runtime argument declared in the action's `args` table.

#### Runtime Arguments

A runtime argument concerns the current invocation: confirmation, output selection, a resource selector, a placement or the `--spec` file selector. Runtime flags never contain a dot, and one runtime flag has one meaning across every command.

#### Invocation Records

A typed machine request can contain runtime arguments because it describes one invocation. That does not make it a reusable specification. Specification readers and specification output exclude confirmation, output switches, selectors and the input file.

### Shared Flags

A runtime argument several modules take, such as `--json`, `--yes` or `--relay`, is declared once in `link-actions`' `shared.toml`. An action's `args` names it as `json = { x-link-cli = { shared = "json" } }`, and the generated parser gives it that file's type and help. A table there with `args` is a group, such as a placement, taken whole by an argument of its name.

#### Shared Types

`link-component`'s `shared.toml` gives the same flags by the shared type owning each, `output` owning `--json` and `confirm` owning `--yes`. The flags check reads that file and refuses a command whose own argument takes a shared flag's spelling with another meaning. An adapter's additional argument stays in that adapter's `actions.toml`, so another adapter never accepts an argument it ignores.

### Variables

An `LNK_` variable is defined once, in the owning crate's `variables.toml`. A runtime argument names one as its fallback, `embed-url = { type = "string", x-link-cli = { env = "LNK_MEMORY_MODEL" } }`, and its help shows it. A specification field never reads a variable. The catalog refuses [a fallback no `variables.toml` defines](../../../../packages/actions/docs/more/developing/modules/catalog.md#variables), and the docs' source audit refuses code reading a variable its crate doesn't declare.

### Machine Protocols

Programs Link runs for itself, such as tools, stores and harnesses, declare their words as plumbing actions. A JSON request on standard input is a typed invocation record, never a second set of flags.

### Enforcement

Clippy refuses `std::env::args` and `std::env::args_os` outside the shared entry's invocation reader, and also refuses clap's direct process readers, such as `Parser::parse`. [The shapes check](../../../../tests/repository/docs/more/developing/testing/repository-checks.md) refuses a clap derive, a hand-written `Specification` or a typed inventory outside the generators and the few files `shapes.toml` holds, so no command's arguments and no specification's shape is written in Rust. Every program's commands are checked against its declarations in the repository's tests, and no program is held out of that check.

## Command Interface

Command words name an operation and its object. Runtime flags qualify that operation, while field flags configure its specification.

### Name a command

Every command reads `lnk <group> <verb> [<object>] [--flags]`, so a word in one place always means one kind of thing.

#### The positional

A positional is what the verb acts on, and only that. Anything else is a flag: [`lnk agent move here --agent work`](../../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud), never `lnk agent move work here`. The agent is `--agent` on every [`lnk agent`](../../../../modules/agents/agent/docs/agents/README.md#agents) command but those that make or end it, where it's the object: [`lnk agent create work`](../../../../modules/agents/agent/docs/agents/start/create.md#create-on-a-hosting-account) and [`lnk agent delete work`](../../../../modules/agents/agent/docs/agents/start/delete.md#delete-your-agent).

#### One slot, one kind

A slot holds Link's own words or names you chose, never either: a harness's commands are [`lnk agent harness <verb>`](../../../../modules/agents/agent/docs/agents/harnesses/use/harness/README.md#inspect-the-sandbox), not `lnk agent
<harness> <verb>`. Link's words in a name's slot (`here`, `home`,
`all`) are refused as names.

#### An object left out

Leave out an optional object and the command takes the default, and says which. A setting shows what's current and the choices, as [`lnk agent harness set`](../../../../modules/agents/agent/docs/agents/harnesses/use/README.md#switch-in-one-command) lists the harnesses; an action takes the one Link would pick (the only agent, the latest release).

#### Verbs in pairs

Use `create` and `delete` for existence, `add` and `remove` for a connection between things, `install` and `uninstall` for installed software, `connect` and `disconnect` for accounts, and `start` and `stop` for running things. `set` and `get` write and read one value. The repository's `verbs.toml` checks the vocabulary, hidden commands included.

#### `--yes` and renamed commands

`--yes` answers yes to every question a command would ask, and is the only flag that does. Record renamed commands and flags in the module's [history file](#history-file), beside its `actions.toml`. The old spelling is refused with its replacement, so every module shares the same guidance. A specification's flag is retired by its specification's [schema step](../../../../packages/spec/docs/more/libraries/types/specifications.md#retired-flags) instead, with its field.

### Add a command's flags

A command's arguments are declared in TOML, and the module's build generates their Rust, so a file, its flags, a dispatched call and a test are read by one typed definition with the same validation and defaults. The format references are [Catalog](../../../../packages/actions/docs/more/developing/modules/catalog.md) and [Shared Types](../../../../packages/types/docs/more/developing/modules/catalog/shared-types.md).

```toml
# actions.toml
schema = 1

[actions."env.approval.add"]
description = "Say yes, ahead of a run, to giving the environment file a secret it asks for"
role = "secondary"

[actions."env.approval.add".args]
secret = { type = "string", description = "The secret, by its name or its ID", required = true, x-link-cli = { position = 1 } }
spec = { type = "string", x-link-path = true, description = "The environment file (env.toml) that asks for it", required = true }
```

1. Declare the action in `actions.toml` by its ID,
   `<namespace>.<resource>.<verb>`, with its `description`. Where it reads
   a specification, name the kind in `spec`, and its field flags follow from that
   kind's shape.
2. Declare each runtime argument in the action's `args`, with its `type`
   and `description`, which is its help. `x-link-cli` says how the
   command line takes it: a `position`, a `short` letter, a fallback
   `env` variable, or a `shared` flag.
3. Declare a configurable value as a field of the specification kind's shape in
   `types.toml`, the shape marked `x-link-spec`. Its `description` is its
   help and its reference entry, and its key path is its flag.
4. Give a default with `default` in the declaration. The generated type
   supplies it to a file and its flags alike, and a default its own shape
   refuses is refused when the module builds.
5. Implement the action's method on the generated handler trait, which
   takes the typed arguments and specification. Record a retired spelling in the
   module's `history.toml`.
6. Take no secret in a flag, as [No flag takes a
   secret](#no-flag-takes-a-secret) says.

#### No flag takes a secret

No flag takes a secret, since other users of the machine see command lines: a secret comes from a variable, a file (`--header-file`), standard input or the Keychain, and a switch may ask for it (`--auth`). A value that is itself a secret is declared `x-link-sensitive = true`. Its generated type holds it as a zeroized `Secret`, which is never printed, and a field flag never takes its value.

#### A public value, or a secret's name

A field that names something public, such as a public key or a key's ID, or that names a secret rather than holding it, such as `--secret NPM_TOKEN`, is a plain field. It is kept in a file and printed like any other, and its description says which it names.

#### What the checks hold

The catalog [refuses a declaration that breaks a rule](../../../../packages/actions/docs/more/developing/modules/catalog.md#rules), naming its file and why. The flags check holds every long flag to one type and one meaning across every module, and the shapes check refuses an argument or specification shape written in Rust.

#### Typed Result Comparison

The comparison serializes each actual typed reader result as both TOML and JSON. JSON retains optional `None` values as `null` where TOML omits them, so a file-only default cannot disappear from the comparison. Autogenerated help and version actions are excluded by their action type; ordinary fields with those names still receive the same checks.

#### Inherited Global Arguments

Inherited globals are also parsed through each complete child command. The check reads their values at the owning ancestor and compares them with an independently written file. A name declared by the child stays with that child. A group requiring a further subcommand supplies no bare-command proof.

## Specification References

Every specification kind has one reference page generated from its type, laid out the same way, so a reader learns one layout and finds any field by its key path.

### Layout

The page's title is the kind's name, such as Agent Specification, followed by how to use it: the commands that consume it, `--spec FILE` and `lnk <group> spec show`. Every Option follows, a TOML example with every field that the specification's own reader accepts. Each top-level key is then an H2 and each field under it an H3, with deeper tables continuing to H4, so `model.thinking` is explained under `model`, then `thinking`.

```markdown
## model

### thinking

> **Type** one of `off`, `low`, `medium`, `high` · **Default** `low` · **Flag** `--model.thinking`

How much the model reasons before it answers, where its provider supports reasoning.
```

### Field Entries

Each field's entry opens with a quoted line giving its type, its allowed values or range, its default and its flag. The prose below it is the field's `description` in `types.toml`, whose first paragraph is also its `--help` line and its JSON Schema description. A field that needs more, such as how a model name says who serves it, links to the guide section that explains it.

### Shape Declaration

Each specification kind is a shape in its module's `types.toml` marked `x-link-spec`, and [the build generates its Rust type](../../../../packages/actions/build/docs/more/developing/modules/catalog/generation.md#specification-kinds), its reader and its JSON Schema from that one declaration: its fields, nested tables, lists, maps, variants and descriptions. The flags, their help, the JSON Schema `lnk spec show` prints and the reference page all come from that shape.

### Finding a Reference

The [specifications index](../../../config/specs/README.md) lists every public kind alphabetically, generated from the bindings. Each owning module's section page links its specification reference in its opening, so a reader reaches it from either place.

### Generated Pages

A reference page is generated, so the line, heading-count, and heading-paragraph rules that hold authored pages don't hold it: each specification kind is one page, however long. Explanation beyond what a field is and means, such as how settings from several specifications combine, lives on guide pages that link to the reference.

## Specification Contract

Every specification follows the same rules for defaults, machine policy, publication, and validation.

### Defaults and Targets

An omitted field grants the least access and incurs the lowest cost that still does the job. The specification kind's declaration supplies defaults, and the same loader resolves them for files, field flags and inspection. A type composed from another specification keeps that type's validation and defaults.

#### Target Tables

An OS or cloud target table, such as `[files.macos]` or `[machine.aws]`, has its own dotted key paths. The owner applies those fields over the common table after reading the input. A restrictive list, such as hidden paths, adds its restrictions instead of reopening a path. Path values are resolved before this composition: a file's paths start at the file, and a flag's paths start at the current folder; `~/` starts at the user's home. Paths containing `..` are refused.

#### Two exceptions to the least

Missing means least, but for two exceptions: a default that asks you first, such as a box's login, and a limit that only paces what a grant already bounds, such as a sandbox's `per_minute`.

#### What a specification still names

A specification still names what belongs to whoever runs it, such as a sandbox's tools, ports and network. So a specification handed on means the same grants and not the same reach, and a grant that can't mean anything fixed on a machine is refused there.

#### A specification shown, not run

A command shows what a specification would do and does nothing, as [`lnk box size list`](../../../../modules/compute/box/docs/services/boxes/size/README.md#list-matching-machines) and [`lnk sandbox check`](../../../../modules/security/sandbox/docs/security/sandbox/guide.md#sandbox-guide) do. [Decisions](../../../config/specs/decisions.md#does-each-specification-keep-its-own-rules) says why specifications work this way.

### Control and Publication

Configuration and permissions are control files kept in Safes. No sandboxed run reaches a Safe through a direct path, alias or broad data grant. Sharing a specification grants neither access beyond machine policy nor permission to bypass an approval, and a field flag never takes a raw credential: a secret field names the secret or its source.

#### References and Availability

Specification references show actual typed TOML in the opening or first subsection. Examples stay checked against the current fields and defaults. A configured feature still reports whether it is enabled and whether its installed prerequisites and permissions allow it to run.

### Contract Checks

Repository checks hold the contract for every specification kind. A failure names the kind and the rule, and nothing is excused by a list of exceptions.

#### Binding Checks

Every specification kind has a binding that names its consuming actions and generates its `spec show` command, and every module exports a history that parses. The binding's shape agrees with the paths serde actually accepts, found independently by probing the type's reader.

#### Parser Checks

No program code outside the shared entry reads process arguments, no command redeclares a shared flag, and no code reads an undeclared `LNK_` variable.

#### Reference Checks

Every field has a description, every kind's reference page equals its generated form and its Every Option reads back through the owning reader. The specifications index lists every kind, and every owning module's section page links its reference in its opening.

#### Spelling Checks

No runtime flag contains a dot, no specification has a top-level field named `spec`, `kind` or `schema`, no two modules export one kind, and every runtime flag keeps one meaning everywhere it appears.

#### Equivalence Checks

The active generated sample reads to equal serialized specifications through its file and emitted flags, in the same source context. A specification printed by `spec show` reads back through its consumer unchanged, with no runtime argument in it.

#### Scalar Precedence

Scalar precedence also needs a conflicting accepted configuration. The guard tries one contrast, requires the complete file and an isolated explicit flag control to preserve it, then checks that the flag overrides the original file. An array record uses its actual containing collection flag. Rejected, elided or untargetable contrasts remain explicit exclusions; they establish no precedence or runtime behavior.

#### Regression Cases

Mutation tests add an optional field absent from every example, hand-write a dotted runtime flag, drop a field from a declared schema, stale an example and leave a history entry past its `until`. Each fails at the shared boundary.

#### Execution Evidence

Parser and loader tests establish the contract without provisioning real resources.

## Test a specification with files

A test case is a file in `tests/specs/<group>/` that holds the specification as a user writes it, and what must come of it. A new bug is a new file, not new test code.

```toml
# tests/specs/sandbox/openai-only.toml
[spec]
schema = 1
name = "openai-only"
[spec.internet]
hosts = ["api.openai.com"]
[spec.commands]
allow = ["curl"]

[expect]
reaches = ["api.openai.com"]
refused = ["example.com", "api.openai.com:80", "192.168.1.1", "127.0.0.1:22", "169.254.169.254:80"]
runs = ["curl"]
refused_commands = ["sh", "ls"]
```

One runner per group reads every case, in the tests of the crate that owns the specification, and every specification that reads is printed and read back as itself. The sandbox's runner (`modules/security/sandbox/tests/specs.rs`) runs [`lnk sandbox check --spec`](../../../../modules/security/sandbox/docs/security/sandbox/guide.md#sandbox-guide) on it, and the box module's (`modules/compute/box/tests/box/specs.rs`) runs its matching against a fake cloud's sizes, with `[expect] size = ...` or `error = ...`. A specification that must be refused is a case too, and its `[expect] invalid` holds the words of the error it gets.

### Policy cases

A policy case gives a policy, the `[access]` of the resources it's checked on, then each `[[check]]` (a principal, an action, a resource and the words of the answer) and each `[[usage]]` within its limits or the one it `exceeds`. Its runner is `packages/policy/tests/specs.rs`.

### The docs' blocks

Every `toml` block the docs and the website show is read by its type, so a page never shows a file Link refuses. A box's pages hold box specifications and the sandbox's hold sandbox specifications. A block elsewhere is one of those, an agent's specification or a team's, or a case whose first line is its path, like the one above.

### An agent's specification

An agent's specification differs from the rules above in one way, because
**missing means Link's** and not least. It holds opinions and not grants,
and Link's own are a specification of their own beneath it, so a key left out is Link's, and `extends = "none"` starts from nothing. Its cases in `tests/specs/agent/` give the agent's own specification, its teams' files and the union that must come of them (`modules/agents/agent/contract/spec/tests/specs.rs`).

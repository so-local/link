# Module File

`module.toml` names a module and what it needs, and its ID is the
namespace every action, command and specification kind of the module begins with.
This page covers what the catalog reads from it, and
[Components](../../../../../../component/docs/more/libraries/types/component-metadata.md)
covers what the installer reads.

```toml
# module.toml
schema = 1
next = ["env.env.create", "env.env.setup"]
recommends = [{ dependency = "sandbox" }]

[module]
id = "env"
kind = "module"
owner = "modules/compute/env"

[dependencies.box]
kind = "module"
id = "box"

[removal]
parts = ["environments"]
```

## Identity

`id` is the module's one name. Its program is `lnk-<id>`, its commands
begin `lnk <id>`, its actions begin `<id>.`, and `actions.toml` never
repeats it. Core's `id` is `core`, its program is `lnk` itself, and
every catalog loads its declarations beside the modules. `kind` is the
component's kind, such as `module`, a connection, or a store,
and `owner` is the module's folder, which the layer and dependency
checks read.

## Dependencies

`[dependencies.<name>]` names a component this one needs, each by its
`kind` and `id` with that kind's own key where it has one: a `module`'s
`protocol`, an `external` program's `version`, a `capability`'s
`min_abi`, or a `package` whose shapes it names, such as a contract,
with none. `recommends` names one it works better with. A shape of
another module is named only where that module is a dependency, and [an
implemented contract](actions.md#contracts) must be one.

### Needs of One Run

Dependencies are the module's, never an action's. A need that depends on
what a run asks for, such as [`lnk env setup --on <box>`](../../../../../../../modules/compute/env/docs/services/environments/guide.md#set-a-machine-up-without-an-image) needing the box
module where `--here` doesn't, is a check hook on the action. It refuses
the run, naming the command that installs what's missing.

## Protocols

Two top-level keys give a module one of core's protocols, each generated
into its parser and never declared in `actions.toml`. `answers` lists
[the questions it answers](../../../../../../component/docs/more/libraries/types/component-metadata.md#question-answers) for modules below it, asked as `lnk-<id>
--link-answer <question>`.
`guest_service = true` says its program runs a guest service, re-entered
by the hidden `guest-service` command.

## Removal

`[removal]` names the parts the module keeps, which [`lnk uninstall`](../../../../../../../docs/more/modules/guide.md#uninstall)
reports and removes. Only a module declaring it is asked, so only its
parser has core's hidden `uninstall` command, and a module keeping
nothing has none.

## Next

`next` lists the actions that lead into the module, by their IDs: what
a person reaches from [`lnk up`](../../../../../../../docs/more/modules/install.md#pick-your-modules) and is offered at `lnk <id>`. Each is the
module's own, on the command line, and never plumbing. Its program
answers [Next's graph](../../../../../build/docs/more/developing/modules/catalog/generation.md#nexts-graph) as the hidden `nexts --json`, generated from its
declarations.
Core's `lnk up` and [`lnk mod add`](../../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) lead into every module, which an
action of core's says with `next-modules = true`.

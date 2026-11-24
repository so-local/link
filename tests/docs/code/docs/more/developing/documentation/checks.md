# Docs Code Checks

`link-docs-check-code` checks documentation against the code Link releases.
Its rules validate commands, examples, environment declarations and
source-backed references. Run `cargo test -p link-docs-check-code` to check
them without invoking a built module. The text and reading-tree rules
belong to `link-docs-check`, as [Writing Docs](../../../../../docs/more/developing/documentation/writing-docs.md)
explains.

## `commands`

Every `lnk` command a page gives is one a module's `module.toml`
declares, or one the core handles itself. The registry holds every
group and every command the components declare, those hidden from `lnk
--help` too: a page may name `lnk agent state-send`, which an agent's harness
runs, not a person. A group whose first word may be a name, such as
[`lnk measure --agent <agent>`](../../../../../../../modules/tooling/measure/docs/tooling/measure/README.md#measure), takes any.

### Shown in its section

The reverse holds too: `command-pages` reads every `actions.toml`, and
each command a person runs, on the command line and not plumbing, names
the section explaining it, `docs = "<address>#<anchor>"`: the page's
website address, where its owner's `docs/` puts it, and its heading's
anchor.

#### What the Section Holds

That section, from the heading to the next one as high, shows it
as `lnk <command>`, and a primary command's is a heading of its own:
below its page's title, and no other primary command's of its module.
Generated indexes and inventories don't count, since they list a
command without saying what it does. A command without its section when
the rule came in is under `[held]` in `tests/docs/large-files.toml`, as
`<actions.toml>#<command>`.

### Linked to its owner

`owner-links` reads every page with the same sections: the first time a
page shows, in a code span, a command another package owns, the span
links to that command's section. The package is the nearest folder with
a `Cargo.toml`, so a contract's page explaining its module's command
links it too. `lnk-docs-inventories --write` wraps each such mention in its
link, or points a link already around it there; a mention inside a
longer link's text is left for you to move out of it.

### A folder for each command group

A command group, a module's command with commands of its own, whose
word is no approved verb from `tests/repository/verbs.toml`, such as
[`lnk box image`](../../../../../../../modules/compute/box/docs/services/boxes/image/README.md#create-an-image), gets a folder of its own: `command-folders` wants a
folder named its word, or its plural, in its owner's docs, with a page
there showing it. The agent's `chat`, `model` and `team` are the
pattern. A verb's command, such as [`lnk box create`](../../../../../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box), is a section on
its module's pages instead.

## `flags`

Flags in explicit inline commands and shell examples come from the
referenced command's typed declaration, including parent flags and
short aliases. Continued lines, quoted values and attached values are
read as one invocation. After `--`, flags belong to the invoked program;
a pipeline's other programs have their own arguments.

### Scope

The check covers named flags on explicit `lnk` invocations, including
internal registry commands. Help and version switches follow the CLI's
built-in behavior. A flag mentioned on its own in prose has no command
to check against. Value types, required arguments and flags that conflict
remain separate checks.

## `arguments`

Explicit literal values use the actual Clap value parser, including enums,
numeric ranges and custom types. Complete shell examples also satisfy
required arguments, cardinality, conflicts and dependencies. Inline
fragments may omit required arguments. The check clears parser environment
inputs and treats shell placeholders as symbolic values.
Developer workflow examples use the developer module's parser, and their
TOML uses its owning repository configuration type.

### Expected errors

A deliberately rejected example uses a `sh expect=error` fence. Its
arguments must actually be rejected. Parsing verifies the command contract;
quickstart fixtures
verify supported examples' effects in an isolated environment.

## `environment-reads`

The source audit follows production Rust environment reads through imports,
constants and reader helpers. Every static `LNK_*` name needs an owning
command declaration or library Cargo declaration. Dynamic namespaces and
caller-selected reader APIs have separate declarations. A wildcard never
authorizes a newly added literal name. Tests, fixtures and build-only code
are outside this production audit.

## `json`

A JSON fence has an immediately preceding `<!-- json: owner-id -->` marker.
The registered binding deserializes it through the released wire type or
website schema. Duplicate keys, trailing data, unknown fields, missing
fields and wrong types fail. Defaults that silently discard supplied
fields fail too. A new owner needs a binding rather than a copied shape.

### Website schemas

The sponsors and offers examples use the same TypeScript schema constants
as the runtime readers. Rust reads their strict JSON literal declaration
and rejects unsupported schema keywords. Website tests validate the same
published examples through the runtime validator.

## `references`

The writer derives each specification's field summary from its real deserializer,
with source discovery for flattened or custom representations. Nested fields
and struct variants are included; an opaque representation fails closed.
Checked defaults come from actual deserialized values and owner constants.
The generated summary supplements the authored explanation of each setting.

### Reference ownership

Every published specification page binds to an owning type or an existing bound
shared section. A proposed format must visibly say it is unimplemented
and cannot present a runnable TOML example. Machine configuration includes
extensible consumer settings, so its summary explicitly covers its typed
identity and machine-kind records.

## `specs`

A TOML block a page shows is a specification Link reads, read strictly by its
type: a box specification on the boxes' pages (or a setup stage's sandbox specification),
a sandbox specification on the sandbox's, and elsewhere one of them, an agent's
specification or a team's, an agent's sandbox file, a model's, or
`/etc/lnk/kind.toml`, its first line naming it.

### Other TOML a page may show

A `module.toml`, or a specification test case, its first line `#
tests/specs/<group>/<case>.toml`, whose `[spec]` is its group's.
Owner-specific settings are checked by the workspace that contains their implementation. Public documentation checks import only public settings types.

## `inventories`

Inventories enumerate their owning sources, and the check compares the
whole generated block. Missing, obsolete, duplicated or changed entries
fail. The writer updates only unique marker pairs and preserves the
surrounding explanation.

### Sources

The command index reads registered routes and core command declarations.
The environment index reads command and library declarations, including
internal and test overrides, with dynamic namespaces listed separately. The [Specifications](../../../../../../../docs/config/specs/README.md) index reads
the published pages whose titles end in Specification, excluding the index, shared
field references and design rationale. The shared-state inventory reads the same `state::Area` table
that Link's code resolves paths from.

### Namespace commands and aliases

The namespace command and alias references read command-surface actions
from the agent and model registries consumed by Link Harness. The alias
inventory records each spelling and its canonical target. Replaced
and CLI-only actions are excluded. Duplicate primary names, duplicate
aliases, aliases shadowing primary commands and missing targets fail.
Root-level `/help` is a discovery command outside these namespaces.

### Credentials

The credential index reads the runtime catalogue, checks unique identifiers
and labels, and resolves each owning document anchor and production source
symbol. New credential categories enter through that typed catalogue,
rather than a second expected list.

### Explained beyond the inventory

An inventory says what exists, and a page says what it's for. The
`variable-pages` and `state-pages` rules read the environment and state
inventories' own sources, and each `LNK_*` variable and state folder is
named in backticks on some page beyond the generated list. What either
rule held when it came in is under `[held]`, as
`<declaring file>#<name>`.

### Survey the docs

`lnk-docs-survey` reports, in Markdown, what the structure and coverage
rules measure: how many pages the links in paragraphs reach from the
home page, and in how many hops, how deep each page sits, each
package's docs and Rust lines against the `docs-ratio` curve, the
commands each owner's pages show, and what each rule still holds. Run
it from a workspace, or name another with `--root`, and its
`large-files.toml` with `--large` when it isn't in `tests/docs/`. Run
`cargo run -p link-docs-check-code --bin lnk-docs-survey`, which prints
the survey as Markdown to save, such as in `survey.md`.

#### Docs Reviews

`--reviews` names a record of docs reviews, a TOML file of each owner's
last review: the commit read, the day, the review checklist's version,
its outcome and what's left. The survey then lists the owners with docs
never reviewed, and how many commits have touched each reviewed one
since, so a review's next owner is the survey's to name.

### Review leads

The survey lists what no rule fails but a review should read. Paragraph
pairs on two pages that are near copies, by the thresholds in
`limits.toml`'s `[survey]`, may be one explanation in two places. Pages
the sidebar nests under a page their address isn't below are placed by
hand away from their address tree, which the sidebar's order may keep
but its nesting shouldn't.

### Regenerate

Run the inventory writer after changing a declaration or reference,
then the reading-tree writer to persist any new page's navigation.
The default inventory mode checks without writing. Run `cargo run -p link-docs-check-code --bin lnk-docs-inventories -- --write`, then `cargo run -p link-docs-check --bin lnk-docs-tree -- --root . --write`.

# Repository Checks

The checks that read the whole repository rather than one package's
code: its layers, its dependencies, its boundaries, its declarations and
the rules its tests keep. Each is a Rust test that runs with
`cargo test --workspace` and in every session check, and each can run
alone with `cargo test -p link-repository-check <module>::`.

## Why They Exist

A rule a reviewer must remember is broken the first time nobody
remembers it. Each rule here was a review comment once, and is now a
test that fails naming the file, the line and what to do instead. Where
the repository broke a rule when it came in, a list beside the check
names the files that still do. Each such list only shrinks: a file that
keeps the rule must leave it, and no file joins.

## Layers and Words

Each crate depends only on its own group or the groups its `layer.toml`
lists, as the [layout](../../../../../../docs/more/architecture/guide.md#layout) says,
read from every `Cargo.toml` and `layer.toml` by the docs checks
(`cargo test -p link-docs-check`).

### Layer Words

A lower layer never names a higher layer's concepts. A `layer.toml`
declares the words its layer introduces (`words = ["agent", "team",
"harness"]` for the agents' layer), and no file in a layer below says
one, in an identifier, a file's name, a key, an error or its docs. A
word matches as a whole part of a name, singular or plural, in any
case, so `for_agent`, `AgentAt` and `agent.toml` all say `agent`. What
lower layers said when the rule came in is counted in
`layer-words.toml` (`layer_words::`). A part that needs a higher layer's
knowledge [asks a question in its own types](../../../../../../docs/more/architecture/organization.md#layers).

## Dependencies

Every locked crate and npm package is permissively licensed, at one
version, published by someone trusted, and free of advisories
(`dependencies::`). The check reads `Cargo.lock`, every tracked
`package-lock.json`, `Cargo.toml` and `trust.toml`, and runs offline.

### Licenses and Versions

Licenses come from `cargo metadata` for every platform, and every
`LICENSE-MIT` and `LICENSE-APACHE` names the holder and years the root
`Cargo.toml` declares (`licenses::`). A crate locked at two versions is
listed under `[workspace.metadata.second-versions]` with why.

### Trusted Publishers

Every crate has an owner under `[workspace.metadata.trusted-owners]` or
a million downloads, and every npm package an owner under
`trusted-npm-owners` or 100,000 downloads a week. A lock file under
`trusted-npm-trees` is trusted whole, with why, for a tool nothing Link
releases reads, which never trusts its authors elsewhere. Every
advisory against a locked version is listed under
`[workspace.metadata.advisories]` with why, and every listed one still
applies.

### When a Lock File Changes

The evidence (owners from crates.io and npm, npm's weekly downloads and
OSV's advisories) is saved in `trust.toml`, so a new or updated package
fails until it is read again. Run `cargo test -p link-repository-check
-- --ignored refresh` with `LNK_TODAY` set to the day, then review the
change to `trust.toml`. A package neither first-party nor widely used is
replaced, or its owner trusted with why. An advisory is fixed by
upgrading. One no release fixes is listed, and an unmaintained crate
only when it reads no outside input.

## Boundaries and Releases

The sandbox contract reaches no implementation, the default build no
connection, neither it nor Link Memory a document parser, and the
harness and HMAC code stay in their owners (`architecture::`). One line
for Link's signer is everywhere it's trusted, and no committed file
holds a staging or dev build's key (`release::`). Every member ships,
and every declared component appears in the registry (`released::`).

### Private State

Link-owned hidden state belongs under `~/.lnk`. `~/Link` holds no
hidden file Link made, and atomic writes use visible temporary
siblings from the shared path API (`state_roots::`). The sandbox
check's [temporary native probes](../../../../../../modules/security/sandbox/docs/security/sandbox/security.md#temporary-native-names)
are the one exception.

### Saved Files

[Every file Link saves](../../../../../../packages/files/docs/more/libraries/files/files.md#saved-files)
names its schema version, written through `link_files::saved`. Outside
test code, a function that writes a file and serializes a bare
`toml::to_string` or `serde_json::to_vec` fails, as does a bare line
appended to a ledger (`saved::`). `saved.toml` allows another program's
format, a runtime file, a build's output or a fixture. Every saved
format's `schema.toml` lists its files, so [an upgrade finds
them](../../../../../../packages/files/docs/more/libraries/files/files/upgrades.md),
and `placed.toml` lists the formats still unplaced (`placed::`).

### Ready-Made Agents

Each ready-made agent's Home [holds only headings, table headers and
empty fields](../../../../../../modules/agents/agent/docs/agents/ready-made/README.md#blank-until-you-speak),
so the agent never takes an example for its owner's data, and its tools
are pinned in `ready-mades.toml`, so [what it comes with changes only
on purpose](../../../../../../modules/agents/agent/docs/agents/ready-made/README.md#choose-an-agent)
(`ready_mades::`).

## Declarations

A command's arguments and a specification's shape are declared in TOML
and [generated into Rust](../../../../../../packages/actions/build/docs/more/developing/modules/catalog/generation.md),
so no Rust type is the source of a shape (`shapes::`). The check refuses
a hand-written clap argument or choice type, a hand-written
`Specification` impl and a JSON Schema derived for a Rust type, in every
Rust file but tests and examples. `shapes.toml` lists the generator's
own shapes, the files still held until their module is declared, and
each declaration naming a Rust type through `x-link-rust`.

### Commands and Flags

Every command's last word is a lifecycle pair, `list`, `show`, `check`,
`run` or a word `verbs.toml` allows with its reason, and a banned word
such as `new` or `cancel` fails (`commands::`). A flag means the same in
every module: one type across every `actions.toml`, and one set of
values for a choice. A shared type fixes some meanings, such as
`--cloud` and `--owner` (`flags::`). Each tool name a model can call,
`<namespace>__<action>`, is one an agent can call (`tool_names::`).

### Manifests

Every program the workspace builds is `lnk` or `lnk-<name>`
(`program_names::`). Every member takes the workspace's lint levels
with `[lints] workspace = true` (`lints::`). A root `Cargo.toml` keeps
its tables in the order `manifest-order.toml` gives, and every
`package.json` opens with its name and the workspace's version
(`manifest_order::`).

### Code for Older Versions

Function code reads and writes only the current shape, and support for
an older one is declared data in one migration channel. A tracked
source file whose lines say what such support says of itself, such as
"backward compatibility", an alias, an old-path fallback or a sniffed
format, is listed in `legacy.toml`, which only shrinks (`legacy::`).
Outside a `migrations/` folder, no line names a migration.

### No Credential Shapes

No tracked file holds another company's credential, not even a made-up
one for a test (`credentials::`). GitHub's push protection, its scanner
that refuses a push holding a secret, matches the shape of a token
rather than whether it works, and it has no list of exceptions. So a
fake Discord, Slack or Telegram token in a test would stop the
repository from being published. `credentials.toml` names each
provider's shape, and a line that matches one fails, naming the file,
the line and the provider. A test that needs a credential-shaped value
builds it from parts, so its source never matches:

```rust
const TOKEN: &str = concat!("xoxb", "-1234567890-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx");
```

## Tests

The rules every test keeps are read here where Clippy can't see them
(`test_kit::`), and each workspace runs them on its own root.

### Every Test Program Declares Its Kind

Each program Cargo builds for `cargo test` holds
`link_test_kit::program!(unit);` or `program!(system);` at its root,
read from `cargo metadata`. Without one, its tests find Link's folders
in the account's real `~/.lnk` and `~/Link`. A unit program whose tests
start a program or open a socket fails, naming the line: it's declared
`system`, so each test runs in a process of its own.

### Tests Keep to the Testing Kit

Each rule is a pattern and what to use instead. Every file is read for a
skip printed and returned, an `LNK_REQUIRE_*` variable read outside the
kit's gates, and a port found free and let go. A test's code is read for
an `#[ignore]` with no reason, an assertion on `elapsed()`, a loop that
sleeps, a change to the process's environment, a folder made in `/tmp`
by hand, a program made runnable by hand, and a child's environment
cleared with no runtime folder given.

### The Lists That Only Shrink

`test-kit.toml` lists, under each rule's name, the files that still
break it. Its `[promises] left` counts the raw calls whose reason says
they move onto the testing kit, and only goes down.

### Flakes Written Down

A flaky test met in code a change didn't write is in `flakes.toml`
beside this check, with its package, its whole path, the day and commit,
how it ran and what it said (`flakes::`). Whoever takes the list
fixes each at its cause and proves it with `cargo test-copies`, as the
[flake lifecycle](../../../../../../docs/more/developing/testing/README.md#flakes)
says.

## Outside Rust

Every tracked `.sh` file passes `shellcheck`, and the website's checks
are TypeScript in `web/`: `npm run lint` and `npm run build`. `cargo
full-check` runs both where `shellcheck` and `npm` are installed.
`dependencies.yml`, started by hand, reads the trust evidence again and
fails on a new advisory or a package that no longer passes.

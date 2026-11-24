# Decisions

Why `code` asks language servers, and why each runs in a sandbox with no
network.

## Why does `code` ask language servers rather than search text?

A language server knows what a name is: the definition a call reaches,
every use and no comment or string that only spells it, and the errors a
build would show without building. Searching text answers none of that
well in a large project. `code` asks the servers editors already use, so
a language is a setting (`[tool.code.servers]`), never code of Link's: a
server is any program speaking the Language Server Protocol on its input
and output.

## Why does each language server run in a sandbox, with no network?

A language server runs the project's code: `rust-analyzer` its build
scripts and macros, `gopls` and `clangd` the builds they ask. So a
server is held as a `run` command is, made from the same sandbox file,
and starts only what `[commands]` allows, the server named there too:
one list says what the agent may run. It reaches no network, since
indexing needs none and a project's code shouldn't reach out while the
agent only asks about it; fetching dependencies is a `run` command's,
under `[internet]`.

### While its session lives

Unlike a command, a server lives while its session does, because
indexing a project again for every question would cost more than the
answer.

## Why does Link's default `[commands]` allow any program?

Language servers run their projects' build scripts and procedural
macros, which a list of named programs can't foresee, so a strict list
breaks ordinary Rust projects. Link's default sandbox file allows any
program on the agent's `PATH` beside its toolchains. The sandbox still
confines what they reach.

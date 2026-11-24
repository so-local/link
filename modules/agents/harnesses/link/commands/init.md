---
description: Write the repository's AGENTS.md from what's in it
---
Write `AGENTS.md` at the root of your files folder, the repository you
work in: what anyone, a person or an agent, needs before they change it.

First look, without changing anything:

- what it holds: `file__glob` the files by name (`*`, then `**/*.md`,
  the manifests: `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`,
  `Makefile`), and read the README and the manifests;
- how it's built, tested and checked: the commands its README, manifests,
  scripts and continuous integration (`.github/workflows/`) run;
- its conventions: how its code is laid out, named and tested, read from
  a few files of each kind;
- any instructions it has already: an `AGENTS.md` or `CLAUDE.md` here or in
  a folder of it, `.cursorrules`, `.github/copilot-instructions.md`.

Then write it, short, in plain words, facts you found and nothing you
guessed:

1. What the repository is, in a sentence or two.
2. How to build, test and check it: the exact commands, from where.
3. How it's laid out: each top folder and what it holds.
4. Its conventions, those a newcomer would get wrong.

When an `AGENTS.md` is there already, improve it: keep what's still true,
correct what isn't, add what's missing, and say what you changed. When
only a `CLAUDE.md` is, write `AGENTS.md` from it and leave it be. If you
may not write the root of your files folder, say so and give the file in
your answer instead.

$ARGUMENTS

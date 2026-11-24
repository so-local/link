---
name: explore-codebase
description: Find a repository's entry points, build system, tests and conventions before changing anything.
---

# Explore a Codebase

Do this before your first edit in an unfamiliar repository.

1. **Guides.** Read AGENTS.md, CLAUDE.md, CONTRIBUTING.md, README and any
   `docs/` index. Note the commands they name for build, test, lint and
   format, and the rules they set.
2. **Shape.** `ls`, then `git ls-files | head -200` or `find . -maxdepth 2
   -not -path './.git*' -not -path '*/node_modules*' -not -path '*/target*'`.
   Identify languages and build systems from the actual manifests and
   lockfiles, then load the matching language skills.
3. **Commands.** Read the repository's documented build entry, task scripts,
   and CI configuration. CI shows what must pass. Prefer these commands
   over guessing commands from the language alone.
4. **Entry points.** Find `main`, the CLI, the server routes or the public
   API of the package you will touch. Follow one request or call from the
   entry to the code you will change.
5. **Find the code.** `grep -rn "<symbol>"` for the name, its callers and
   its tests. Read the whole function and its neighbors, not a snippet.
6. **Tests.** Find where tests for that area live and how they are named,
   which helpers and fixtures they use, and run them once to see a green
   baseline before you change anything. Note failures that already exist.
7. **History.** `git log --oneline -15 -- <path>` and `git blame` on lines
   you will change show the intent and the style of recent changes.
8. **State.** `git status` and the current branch: know what is already
   modified.

Write down, briefly: the build command, the test command (whole and
single), the lint and format commands, and the conventions to match.
Do not read the entire repository; read what the task touches and what
touches it.

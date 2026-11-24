---
name: write-docs
description: Update the README, doc comments and guides a change makes stale, concretely and in the repository's style.
---

# Write Docs

Docs change with the code, in the same commit.

1. **Find what the change makes stale.** `grep -rn` for the old name,
   flag, command or behavior across `README*`, `docs/`, `CHANGELOG*`,
   examples, doc comments and help text. Update each.
2. **Follow the repository's docs rules** if its guide has any (a docs
   section in AGENTS.md or CONTRIBUTING.md): structure, tone, where facts
   live. Keep one authoritative home for each fact and link to it.
3. **Write for a capable reader new to the project.** Say what the thing
   is and what the reader gets in the first sentence. Define a term where
   it first appears. Give a short, runnable example, then details.
4. **Be concrete and true.** Use exact commands, flags and file names, and
   run the example before you write it down. Do not document what you did
   not verify.
5. **Doc comments** say what a function promises, its inputs, outputs,
   errors and surprising behavior, not how it works line by line. Use the
   form already used by the repository, with syntax from the matching
   language skill. A comment says why, never restates the code.
6. **Changelog.** If the repository keeps one, add an entry in its format.
7. Check links and code blocks still work, and keep lines to the file's
   existing wrap width.

Do not create new documentation files nobody asked for.

---
name: review-diff
description: Review your own diff as a careful colleague would, for correctness first, then tests, security, scope and clarity.
---

# Review a Diff

Read the whole diff (`git diff`, and `git diff --staged` or
`git diff <base>...HEAD` for committed work), then each changed function
whole, and the callers of anything whose behavior changed. Check, in order:

1. **Correctness:** inputs at the edges (empty, null, zero, huge,
   unicode), off-by-one, error paths handled and not swallowed, resources
   released, concurrency, nothing left half-changed. Does it do what the
   task asked, in every case the task implies?
2. **Tests:** the new behavior and its edges are tested, the tests fail
   without the change, none was skipped, deleted or weakened.
3. **Security:** run the security-check skill if the change touches
   input, files, processes, secrets or the network.
4. **Scope:** nothing changed that the task did not ask for: no unrelated
   reformatting, renames, new dependencies or drive-by fixes.
5. **Leftovers:** debug prints, `TODO`s you added, commented-out code,
   stray files, build output, secrets (`git status`), missing
   `.gitignore` entries for generated files.
6. **Style and clarity:** matches the repository's conventions; names say
   what things are; comments say why; no needless complexity or
   duplication of an existing helper.
7. **Docs:** README, doc comments and changelog updated where stale.
8. **Checks:** the repository's formatter, linter and full tests pass on
   exactly this diff.

List each finding with its file and line, severity (must fix, should fix,
nit) and fix the ones that are yours before you commit. Re-read the diff
after fixing. State honestly in your report any finding you left.

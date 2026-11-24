---
name: refactor
description: Restructure code without changing behavior, in small steps with the tests green between each.
---

# Refactor

A refactor changes structure, never behavior. Do not mix it with a
feature or a fix in the same commit.

1. **Pin the behavior.** Run the existing tests for the area and see them
   green. If the area is untested, write characterization tests first that
   record what it does now, including odd behavior. Commit them.
2. **State the goal** in one line (extract a function, rename, remove
   duplication, split a module, replace a conditional with a type) and the
   boundary you will not cross (public API, file formats, wire protocol).
3. **Take small steps.** One mechanical move at a time: rename, extract,
   inline, move, change a signature. After each step build and run the
   area's tests. If one goes red, undo that step (`git checkout -- file` on
   your own change, or `git diff` to review) instead of debugging a pile.
4. **Use the tools.** Language servers and compilers find all uses:
   rename with `grep -rn` plus the compiler's errors, not guesses.
   Use the repository's compiler and static checks to catch missed uses,
   with commands from its guide and the matching language skill.
5. **Keep public behavior.** If a public API must change, that is a
   different task: ask first, and update every caller.
6. **Delete what you replace.** No dead code, no old and new paths side by
   side, no commented-out code.
7. **Finish.** Run the full checks, review the diff for accidental
   behavior changes (altered conditions, ordering, error messages, default
   values), and commit in steps a reviewer can follow, each green.

Stop and report if a refactor needs more than the task justified.

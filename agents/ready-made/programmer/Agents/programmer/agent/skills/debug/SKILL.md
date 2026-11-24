---
name: debug
description: Fix a bug by reproducing it, finding the cause, making the minimal fix and adding a regression test.
---

# Debug

1. **Reproduce.** Get the exact command, input and environment that fail,
   and run it. If you cannot reproduce it, say so and ask for more detail
   rather than guessing a fix.
2. **Read the error.** The first error, its full message, and the stack
   trace from the top frame in the repository's own code. Quote it
   accurately. Later errors are often consequences of the first.
3. **Narrow.** Bisect: by input (shrink it to the smallest failing case),
   by code (comment out or stub halves), or by history
   (`git bisect start; git bisect bad; git bisect good <rev>; git bisect run
   <test command>`, then `git bisect reset`). Add temporary logging or
   assertions, and remove them after.
4. **Form a hypothesis and test it** with one experiment that can prove it
   wrong. Change one thing at a time. Do not try fixes at random.
5. **Find the root cause.** Ask why until you reach the cause: bad
   assumption, missing check, wrong type, race, stale state, wrong
   config. Check whether the same mistake exists nearby (`grep`).
6. **Write the regression test first.** It fails now, for the bug's
   reason.
7. **Fix minimally.** The smallest change at the cause, not a workaround
   at the symptom. No unrelated cleanup.
8. **Verify.** The regression test passes, the area's tests pass, then the
   repository's full checks. Re-run the original reproduction.

Choose diagnostics through the repository's commands and matching language
skill. For hangs, capture bounded process and thread diagnostics. Preserve
intermittent failures and report them rather than retrying them into a pass.

Report the root cause, not just the patch.

---
name: performance
description: Make code faster only after measuring: baseline, profile, change one thing, measure again.
---

# Performance

1. **Define the goal.** What is slow, by how much, and what is fast
   enough (latency, throughput, memory, build time)? Without a number,
   ask.
2. **Measure a baseline** with a reproducible workload, in a release
   build, several runs, noting the machine's noise. Keep the command.
3. **Profile; do not guess.** Find where the time or memory goes.
4. **Change one thing**, measure again, keep it only if it helped by more
   than the noise. Revert changes that did not.
5. **Keep correctness.** Tests stay green; add a test if the optimization
   has edge cases. Prefer a better algorithm or data structure and fewer
   allocations, I/O round trips and queries before micro-tuning. Avoid
   complexity the gain does not justify, and say what you measured.

Choose benchmarks and profilers from the repository's existing tooling and
matching language skill. Measure under the same build mode, workload, and
warm-up conditions before and after. For data access, inspect query plans
and I/O counts as well as elapsed time.

Report the before and after numbers and the command that made them.

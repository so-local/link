# Harness Benchmarks

A controlled snapshot compares Link Harness's startup and idle memory
with Hermes and OpenClaw. These measurements isolate harness overhead
from real model inference and answer quality.

## Measurements

The source deployments include each harness and its required support
processes. First answer measures launch to the first complete verified
answer. Each baseline contributes one saved first-answer value, with no
first-answer spread reported. Idle memory measures the deployment's median
proportional set size, which divides shared memory among its users.

| Metric | Link Harness | Hermes | OpenClaw |
|---|---:|---:|---:|
| First answer | 0.42 s | 5.71 s | 21.5 s |
| Idle memory | 92.8 MB | 493.9 MB | 1698.7 MB |

## Comparison with Hermes

Hermes takes 13.6 times as long to reach its first answer in this
snapshot. Link Harness uses 81.2% less idle memory. Ratios use the saved,
unrounded measurements; table values are rounded. A comparable Link
Harness installation-size measurement is unavailable, so this snapshot
supports no claim about smaller downloads or disk usage.

## Comparison with OpenClaw

OpenClaw takes about 51 times as long to reach its first answer in this
snapshot. Link Harness uses 94.5% less idle memory. These ratios use the
rounded table values. Rounded down to what holds at either end of that
rounding, they're 50 times and 94%.

## Conditions

The completed source runs use separate Linux workers with the same
recorded processor class and resource limits. A controlled model stand-in
isolates harness costs; the snapshot's recorded settings are below.

| Setting | Recorded value |
|---|---|
| Measurement date | `2026-11-07` |
| Processor | AMD EPYC 9V74 |
| Effective CPU cores | 4 |
| Memory limit | 17.2 GB |
| Build | Release |
| Model | Controlled stand-in |
| Transport | Wire |
| Sandbox | Disabled |
| Restart samples | 3 |
| Idle settling and sampling | 120 s settling, 300 s sampling |

## Deployment Paths

Link Harness uses its first-party agent endpoint, including its
supervisor, model bridge and proxy. Hermes and OpenClaw use pinned
persistent Agent Client Protocol (ACP) runtimes without Link's external
adapters. These are source-deployment costs, rather than measurements
of Hermes or OpenClaw installed through Link. Real model latency,
task quality and sandbox overhead require separate measurements.

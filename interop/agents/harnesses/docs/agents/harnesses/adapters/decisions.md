# Decisions

Why Link integrates third-party harnesses through shared contracts while keeping their native differences in adapters.

## Why focus on personal agents?

Personal agents need ongoing conversations, reusable skills, memory, schedules, and messaging connections. Hermes is the preferred third-party personal harness and first target for shared integration work, followed by OpenClaw. Their adapters map these native resources into Link's management and transfer model. DeepSeek has a narrower mapping centered on portable history and bounded continuation.

## Why keep an adapter for a harness that speaks the protocol?

The Agent Protocol supplies common conversation and portable-state operations. A harness still has its own installation, configuration, processes, native stores, and release behavior. Its adapter joins those responsibilities to Link's lifecycle. Direct protocol support reduces translation, while native differences stay with the integration that understands them.

## Why do transfer capabilities differ?

Harnesses expose and restore different native resources. Hermes can write imported conversation content through its session database, while OpenClaw's gateway cannot reconstruct foreign sessions. The shared hub can retain a portable archive and supply bounded continuation where native import is unavailable. Declaring those capabilities separately preserves useful control without claiming an unsupported state transfer.

## Why invite more adapters?

The goal is useful management, reasonable defaults, and clearly described transfer capabilities. A contributor can add a harness or improve an existing adapter through the same contract and conformance kit. Harness creators can expose supported state through the Agent Protocol directly. Each contribution remains useful on its own, and its declarations tell users what they can rely on.

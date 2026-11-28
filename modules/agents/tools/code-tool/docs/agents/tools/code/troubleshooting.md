# Troubleshooting

What to do when [your agent has no `code`](README.md), or a language server doesn't
start or answer. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## Your agent has no `code`

No server it may run is on this computer: install one, or add its path
to `[commands]` and your specification.

## "no language server for go here: it would run gopls, but this machine doesn't have it"

The server isn't on the system's `PATH`. Install `gopls`, then `lnk
agent start`.

## "... but your sandbox file's [commands] doesn't allow it"

The server is on this computer, but your agent may run only the
programs its sandbox file names. Add the server to `[commands]`, then
[`lnk agent start --sandbox`](../../../../../../agent/docs/agents/README.md#start-your-agent) your file.

## "no language server serves .zig files here"

No language Link knows serves that extension. Add the language under
`[tool.code.servers]`.

## Diagnostics say the server was still working

A large project's first index took longer than the call waits: ask
again, or raise `[tool.code] timeout`.

## A server finds no dependencies

It reaches no network: fetch them first (`cargo fetch` with `run`,
where your sandbox file allows the internet).

# Troubleshooting

What to do when [`lnk measure`](README.md) shows no history, can't watch, or sends
nothing. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk measure
```

## No day's numbers, only now

History is the watcher's: run `lnk measure start`, and the day fills in
as it looks.

## "background services are off here"

`LNK_SERVICES=off` turns background services off on this machine, so
the watcher cannot register a background service. For a development session,
use the [foreground watcher](../../../contract/docs/tooling/measure/more/protocol.md#foreground-watcher)
in a terminal instead.

## Nothing reaches your endpoint

Read `~/.lnk/safe/measures/watch.log`, and check that `<url>/v1/metrics`
takes OTLP JSON.

## An agent's memory is `-` in `lnk agent list`

It isn't running, the watcher isn't (`lnk measure start`), or the
`measure` module isn't here: [`lnk mod add measure`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module).

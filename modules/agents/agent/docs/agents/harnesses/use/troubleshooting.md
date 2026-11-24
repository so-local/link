# Troubleshooting

What to do when [switching harnesses](README.md) goes wrong, one
symptom to a heading. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## `harness set` says there's no harness by that name

Add it first, from Link or from its own repository, then switch to it
by its name: [Use another harness](README.md#use-another-harness).

## "runs in a terminal"

Stop the foreground agent with Ctrl-C in that terminal or
`lnk agent stop --agent <name>`, then switch its harness.

## "<harness> doesn't speak Slack"

Run [`lnk upgrade`](../../../../../../../docs/more/modules/guide.md#upgrade), in case the harness's module is older than the
connection, or `lnk agent harness set` the harness the message names.

## `start` says the harness asks for more than Link's defaults

Run it in a terminal to answer its question, or add `--yes` to agree
ahead in a script.

## A harness's need you said no to

Run `lnk agent harness-permission add` with the flags its start shows, such as
`--network.mode host`, or a risk's name; or run `lnk agent harness reset --harness <name>` to be asked again on its next start.

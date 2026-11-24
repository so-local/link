# Troubleshooting

What to do when [a move](README.md) doesn't go as it should, or your agent on a box
has no internet. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## `start` says your agent runs on a box

Bring it back first, with `lnk agent move here --agent <name>`, then
start it here.

## A move says the agent is in another format

Run [`lnk upgrade`](../../../../../../docs/more/modules/guide.md#upgrade) on both machines, then move again: both ends must
speak the same format.

## A move is refused by where it's going

The place it's going says why it refused, and when the next move is
allowed: move again then.

## A move stopped in its copy

Move again: only what's missing is sent, so a copy tried again picks
up where it stopped.

## `start` says an earlier move may have started it on a box that doesn't answer

Run [`lnk box start <box>`](../../../../../compute/box/docs/services/boxes/lifecycle/README.md#start-a-box) if the box is stopped, then `lnk agent start`
again, so the move is finished or taken back.

## A move says the agent came but didn't answer

Read its logs on the box, where it came and stopped: `lnk box run
<box> -- agent log show --agent <name>`.

## `move` asks for a key

The box can't reach your local model: give a hosted key, so the agent
has a model where it goes.


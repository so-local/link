# Troubleshooting

What to do when [your agent's desktop](README.md) doesn't start, stays black, or a
program can't open it. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## "no Xvfb in the system's folders"

The tool found no `Xvfb` to start the display with in `/usr/bin`,
`/usr/local/bin`, or `/bin`, owned and written only by root. It never
looks on your `PATH`. Install it from your system's packages, or name
its program with `--xvfb <program>` when you register the tool by hand. Run `sudo apt-get install -y xvfb`.

## "display :120 is taken"

Another X server took the display number your agent's desktop keeps,
in `~/.lnk/data/agents/<agent>/desktop/display`. The error says when
another user runs that server, which the tool never sends its cookie
to. Stop that server, or delete
the file and run [`lnk agent tool add desktop`](../../../../../../agent/docs/agents/README.md#add-tools-and-skills) again, which picks the
first number no X server holds and no other agent keeps.

## The screenshot is black

Nothing is on the desktop yet, since it starts empty: start a program
with `run`.

## A program says it can't open the display

It was started before the desktop, which starts at the tool's first
call: take a screenshot first, then start it.

# Format Decisions

Why `link-format` looks and behaves as it does: what its colors, marks,
words and layout mean, and why each is kept to so few.

## The look

Everything `lnk` prints is meant to teach: a person who has typed [`lnk up`](../../../../../../../docs/more/modules/install.md#pick-your-modules)
can learn the rest from what `lnk` prints, without `--help` or the website.
The look serves that in three habits, in this order.

### What does the eye find first?

**The outcome.** Color and bold mark only what a person must take away: did
it work, what state is it in, what went wrong. What can be typed looks
typeable, since every command shown is bold, everywhere. Every command ends
on a way on, Next, last and in the same place, on success and on failure.

### Why is the look one file?

The look is `look.rs`, and no command names a color, a mark or a gap. A
block holds typed values and the formatter alone decides how they show, so
tuning the look changes one file, while a type changed touches every
command that builds it. The types are therefore the careful part, and
each is `#[non_exhaustive]` and built by a builder so a field added later
moves no caller.

## Color

Color is for outcomes alone, and `[output] color = "never"` loses nothing
but them, since the mark and the word say it too.

### What do the colors mean?

**Three roles, each one of the terminal's own 16 colors.** Green is a state
that is as it should be and a success's mark, yellow a state worth a look
and a warning's mark, red a failed state and an error's mark. A person's
theme sets the shade, so light and dark terminals both read, and Link names
no RGB color.

### What is never colored?

Commands, headlines, names, paths, addresses, ids, numbers and prose keep
the terminal's own color, never a forced white, which vanishes on a light
terminal. A warning's text stays plain because its mark says it is a
warning, so a long one does not turn the screen yellow.

### How does a refusal differ from a failure?

A refusal is a report's kind, not a color. It reads as an error, in red,
with its own mark, so a sandbox's no is apart from a failure, and its why
names who refused.

### When is color off?

Color is `auto`: on for a terminal, off for a pipe or a file. `NO_COLOR`
turns it off and `CLICOLOR_FORCE` on, over the settings. The colors are
Rust's own tools' (`anstyle`, which clap uses), so every module drops them
the same way, and `--json` and what one module reads from another stay
plain.

## Emphasis

Bold is the only emphasis, and it is for the commands, a report's headline,
the one value that matters in a block of fields, the word Next and
`--help`'s headings.

### Why only bold?

Faint text is the one emphasis a person may not know they are seeing, and it
is easy to miss on a pale theme. Many terminals and multiplexers draw
italic as reverse or not at all, and underline reads as a link where a
terminal already underlines one. What is secondary is therefore found by
where it is: the context line last, another program's tail indented, a
Next's why after its command.

### Why at most one bold value?

Emphasis is scarce. A block shows at most one bold value, commands aside,
so the eye lands on the one thing the reader must take away. `emphasis =
"never"` keeps color and drops bold, and `color = "never"` keeps bold. The
plain rendering has neither, and nothing in it depends on them.

## Symbols

Each symbol has one meaning in each of the three `symbols` settings, and a
block shows at most one emoji.

### Which marks does `lnk` print?

**Five, each with one meaning.** ✅ is a success, the command's whole job
done. ⌛ is a command that did all it can and waits on the person for the
rest, which it says: neither done nor failed. ⚠️ is a warning that
something set may keep it from working well, such as a model too small to
call its tools. ❌ is an error. ⛔ is a refusal. A state that is only as it
should be, such as an agent found running, gets none.

### What do the other settings print?

With `plain` the marks are ✓, `…`, `!` and ✗ in the same colors, for
terminals that draw emoji badly. With `none` they are the words
`waiting:`, `warning:`, `error:` and `refused:`, and a success gets
nothing. A pipe or a file always gets
`plain`.

### Why is a mark in a cell never an emoji?

A terminal may draw an emoji wider than it counts it, which shifts a table's
columns. A check or a state in a cell or a field is ✓, ✗ or `!`, or `ok`,
`fail` and `warn` with no symbols, so columns stay lined up.

### Why one ellipsis, in ASCII?

Progress ends in `...`, three ASCII dots, only on a line for an action still
going. Tables keep their complete values, using fields when columns do
not fit. A progress bar is
`[######----] 60%` in ASCII so every font has it. A spinner, for an
activity with no count, is drawn by hand on a terminal and nothing
elsewhere.

## Words

Output is written in the docs' voice: plain words, present tense, no
Markdown, and no jargon a person did not type.

### How are words cased?

One rule a kind, so a test checks it at the type. A sentence opens with a
capital and ends with a stop. Progress opens with a capital and ends with
`...`. A state word is lowercase, from a closed list per kind, so `OFF`
cannot be typed. A table header is in capitals, a field label has a capital
and no colon, and a name is as it is on disk or typed.

### Why do commands never ride a sentence?

A command is a Next, on its own line, because a sentence that carries one
cannot be copied from. A report's why may name a command, in bold and never
in backticks, since bold is what lets it stand in a sentence. Next lists at
most five commands, the likeliest first, each in bold with its why after
it, the whys lined up.

### How do success, error and refusal open?

A success opens with what was done, a verb in the past tense and then
where. An error's headline says what did not happen and to what, and its
why says the cause in a sentence or two. A refusal's headline names who
refused. A warning is a fact and its cost. A name a person typed is in
double quotes in a report so an empty or spaced one shows.

## Layout

The same layout in every command, so a reader knows where to look.

### Where does each part go?

Progress comes flush left, then a blank line, then the result indented two
spaces, then Next last, just above the prompt. Columns are as wide as their
widest cell, and labels and headers are plain.

```text output
[1/3] Installing the hermes harness...
[2/3] Pulling qwen3:8b...
[3/3] Starting my-otter...

✅ Started my-otter.

  Name     my-otter
  Model    qwen3:8b on Ollama
  Harness  hermes

  Next
    lnk agent connect telegram   text it from your phone
    lnk agent show             inspect its current state
```

### How does a report look?

A report is its mark and headline, its why indented under it, another
program's tail indented four, Next, and one context line when it is Link's
own fault.

### Why does Next go to stderr?

What is about the run goes to stderr, so a pipe gets only the result:
[`lnk box list | wc -l`](../../../../../../../modules/compute/box/docs/services/boxes/guide.md#list-your-boxes) counts boxes, not suggestions. The result is held
to the end on stdout so the success line leads it.

### Why wrap prose?

Prose wraps at the terminal's width or 80, whichever is less, under its own
indent, so a paragraph reads on a narrow screen. A pipe gets it unwrapped,
while tables use the full terminal width. A table too wide for that
terminal shows each row as fields, keeping names and diagnostics readable.

## Values

Each value is a type, so a command passes the value and the formatter
writes it, the same everywhere.

### Why typed values and not strings?

A size is `4 GB`, 1,000-based, and bytes in `--json`. A duration is `3m 20s`
and seconds. A recent time is `5 min ago` and an older one a date, both RFC
3339 in `--json`. Money is `$0.12/h`, a count `1 box` or `3 boxes`. A
secret never reaches a block, and a field of one shows `<secret>`.

## Design

Why output is typed blocks, and what that settles for everything that
prints.

### Why blocks and not strings?

A command returns blocks and never prints, so how output looks is the
formatter's alone and a command cannot color, pad or word anything
itself. A string error leaves its remedy riding the line after a colon in
Markdown's backticks, and a dead end says nothing of what to do next. A
block holds typed values, so the same block is a styled terminal, a
plain pipe and `--json`, with nothing in one that is not in the others.

### Why is the package called `link-format`?

"Output" could be anything, and the package formats. All of `lnk`'s
output goes through it first, and the styling is a sensible default a
person tunes in `[output]`.

### Why is every setting a variable?

A person chooses a setting in `~/Link/Safe/settings.toml` or in an
environment, so every setting is also a variable whose name is derived
from its table and key: `[output] color` is `LNK_OUTPUT_COLOR`. The
names come from the `Settings` type, so they never drift. Not every
variable is a setting. Plumbing between programs and tests stays
internal, and a secret is never a setting.

### Why is the output's setting a choice of three?

Output has four settings. Color, emphasis and links are `auto`, `always`
or `never`, and symbols are `emoji`, `plain` or `none`. Those are the
axes a terminal, a font or a person's taste disagree on, and `NO_COLOR`
wins over them.

## Next

Next is how a person learns Link without its help, manual or website.

### Why does every command end on Next?

On success it leads on, such as [`lnk agent connect telegram`](../../../../../../../modules/agents/agent/docs/agents/README.md#connect-your-channels) after a first
[`lnk agent start`](../../../../../../../modules/agents/agent/docs/agents/README.md#create-an-agent), and on failure it is the way out. A report's Next is
never empty, and when nothing can be done it says how to report it with
the record. A Next is a typed command, parsed against the command line
when it is made, so every one can run exactly as shown.

### Why does Next remember nothing?

Next says what the command knows now. An install that failed always
offers another harness, and no count of past failures is kept, so the
same input gives the same Next.

### Why is Next checked as a graph?

A Next that leads nowhere is found by a test, never by reading. Each
command declares the Nexts it can give, a program with a Next its command
did not declare panics in a debug build, and the graph read from the
programs must reach every command from `lnk up`. No failure may trap a
person in a retry, and [`lnk issue create`](../../../../../../../docs/more/modules/guide.md#get-help), in every report's last line, counts
for neither since it cannot stand in for a real way on.

## Reports and records

A report has two readers, the person now and the maintainers reading the
issue the person pastes.

### Why does the record keep everything?

The record, in `~/Link/Logs` on the person's machine, keeps the whole
report with its paths and its causes. It is theirs, and nothing in it
leaves unless they send it. What is made to leave is `lnk issue create`.

### What goes into an issue?

Only what happened, and almost everything about that, so nobody has to ask
twice. The things a report names are in scope, each diagnosed by the
module that owns its kind through the same blocks a person sees, and
nothing else from `~/.lnk` is. No secret, username, hostname, private
path or address of the person's is in it, and Link sends nothing itself.

### Why scrub twice?

The first pass is by type and certain, since a secret holds no value and
a path under home is `~/...`. The second is by pattern over text from
elsewhere and errs to removing. A line too few costs a question on the
issue, while a secret too many cannot be taken back.

### Why one diagnosis a kind?

A module that owns a kind of name answers its `status` with typed
blocks, so no module grows its own shape and drifts. More detail is a
field of that `status`, so the person gains it too and there is no second
place to keep in step.

### Why does a report record its own places?

Release builds are stripped, so a backtrace there has no names. Each
step records its file and line with `within`, which a stripped binary
keeps, and the paths are the workspace's, never the build machine's.

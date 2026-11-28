# Format

> Present Results and Recovery Steps

`link-format` is everything `lnk` prints, as typed blocks one formatter
renders: styled to a terminal, plain to a pipe or a file, and as
`--json`. A command never prints; it writes blocks and returns how it
ended, so every command looks the same and how they look is one file. [Why it looks as it does](format/decisions.md) has the reasons.

## A command

A module's `main` hands its command to `run`, which gives it an `Out`
to write blocks to as they come. The command returns `Done`, its
success and what's next, or a `Report`. `run` renders the end, turns a
panic into a report too, and exits with the report's code.

### Help

A command's `--help` takes the look too. `link_core`'s help gives
clap its styles from it: headings, commands and flags bold, an error's
word in the error's color, and nothing else, as `[output]` has emphasis
and color. `lnk` with no command ends on Next, [`lnk up`](../../../../../../docs/more/modules/install.md#pick-your-modules) first.

### Next's graph

Next is how a person learns Link from `lnk up` alone, so it's checked
as a graph. Each command declares the Nexts it can give in its
module's `actions.toml`: `next` on success and `fail` on failure, each
a command as `cmd!` writes it, `retry` for itself again. A debug build
panics on a Next its command doesn't declare, once it declares any.

#### What it counts

The graph holds the commands a person is offered. A hidden command,
and one under a hidden group, is Link's own and is not counted.

#### What the test holds

Every program answers a hidden `nexts --json` with its declarations,
through `link_core`'s help, and an end-to-end test reads the graph
from the programs. Every command is reached from `lnk up` by success
Nexts, but those on a short list of unreached commands, which may
shrink, never grow. A command that declares Nexts declares a failure
Next that isn't `retry`, and every cycle of failure Nexts has a way out.
[`lnk issue create`](../../../../../../core/cli/docs/tooling/issue.md#report-a-bug) counts for neither.

### Handing the terminal over

A command that gives the terminal to another program, such as `lnk box
ssh` or a harness's own screen, calls `out.hand_over()` first. That ends
any spinner, flushes what is held and lends the terminal, and the
program's output is its own, never cleaned or styled. Its exit code
passes on as the command's, and a child that would inherit stderr is
handed over the same way so its lines and Link's never interleave.

### Another program's output

A job's own log or a harness's text is passed on as it came: shown a
line at a time on a terminal, and byte for byte to a pipe. A module that
runs another reads its `--json`, never its screen, and a failure there
arrives as a report the caller wraps, so the headline is the command the
person typed and the inner report is its cause.

### An answer from another program

A command that passes on another program's JSON answer, such as an
agent's action or a model's route, writes it with `out.answer`. With
`--json` that is the reply, as it came. A person reads its members by
name, plain values as fields, nested objects and text of several lines
under headings, and lists of flat objects as tables. A list longer than
16 values shows its count. A command never prints JSON to a person,
only with `--json` or on a protocol another program reads.

### A daemon

A daemon, a service with no person to read it, logs rather than prints.
It calls `link_format::daemon::logs()` once at its start, then logs
with `tracing`'s `info!`, `warn!` and `error!`. Each line is its time in
UTC, its level and its words, never colored, on stderr, which its
service manager keeps as its log. `RUST_LOG` sets how much it logs, and
`info` and up is the default.

#### Crates held to warnings

The crates a daemon serves or calls
through (`hyper`, `hyper_util`, `h2`, `reqwest` and `axum`) log
warnings alone, whatever `RUST_LOG` says, since below that they log a
request's bytes. A daemon that reaches its own services through another
such client, a database's for one, names it to
`link_format::daemon::logs_holding` instead, and it's held too.

### An agent's log

An agent's log, `~/.lnk/safe/agents/<agent>/logs/<harness>.log`, is JSON
Lines (one JSON object a line), each a `link_format::log::Line`
record. A record has its time in UTC (RFC 3339, to the millisecond),
its level (`info`, `warning` or `error`), the IDs that place it where it
has them (`agent`, `chat` for its conversation, `turn` and `call` for a
tool call) and its words:

<!-- json: link_format::log::Line -->
```json
{"at":"2026-11-09T14:05:41.042Z","level":"warning","agent":"3e75d6ac33d355c208c3e1140708b8a9","chat":"link:3e75d6ac33d355c208c3e1140708b8a9","call":"toolu_01ABCDEFGH","text":"Tool agent__send failed: tool agent answered 502 Bad Gateway"}
```

`link_files::saved::line` writes it, as every line ledger is written,
adding its schema to the line as `"schema":1`.

#### Link Harness's lines

A harness whose stderr is the log calls `log::to_stderr` once at its
start, then says each line through `log::say` with its level and
`log::Ids`. On a terminal the same call shows a block aside.

#### Lines in a terminal

[`lnk agent start --foreground`](../../../../../../modules/agents/agent/docs/agents/README.md#start-your-agent) runs the harness with `log::relay`, which
copies its output to the terminal as it comes and logs it a line at a
time. A line that is already a record is logged as it is, naming the
agent when it names none. Any other line becomes a record of its own
with its time.

#### Other harnesses' lines

A harness other than Link Harness, run by launchd or systemd, has its
output appended to the log as it wrote it, so its lines carry no time.

#### Lines shown to a person

`log::Entry::read` reads either kind of line. `log::collapse` and
`log::Shown::render` turn the entries into what [`lnk agent log show`](../../../../../../modules/agents/agent/docs/agents/start/log/README.md#read-the-log) shows:
the local time, a mark, the words and short IDs, with a run of the same
line shown once with `×N`. `Live::log` prints them, or each record with
`--json`.

### The guard

Only this package prints or sets up logging. The docs checks' `output`
rule fails, in any code `lnk` runs, a `println!`, `eprintln!`, `print!`
or `dbg!`, a stdout or stderr taken by hand (`io::stdout()`, its
`tokio` or `anstream` form, `/dev/stdout`, `/dev/tty`, its descriptor),
an ANSI escape or a backtick in a string, and logging set up anywhere
else: a `tracing_subscriber`, `env_logger` or `RUST_LOG` of its own,
where a daemon calls `daemon::logs()`. No file is excepted.

#### What only tests and builds run

Tests, a `tests.rs` module among them, fixtures, examples and build
scripts are outside the rule, and so is a library only they link: one
every crate linking it links among its dev- or build-dependencies, or
is itself a test's or such a library, as the testing kit and
`link-component-build` are. A program is always inside it.

#### Streams that aren't blocks

What a program writes that isn't for a person has a type in `streams`,
each only for what it's for:

- **`protocol()`:** a protocol another program reads on stdout, such
  as MCP's messages, a bridge's frames or a JSON reply; `protocol_on`
  for stderr, and `Protocol::file()` for an async writer.
- **`passed(stream)`:** another program's output passed on as it came,
  or each line after its label (`[build] `) with `labeled`.
- **`child_stderr()`:** stderr for a child to write to in place of its
  stdout.
- **`Terminal::open()`:** the terminal a question is asked on from any
  thread, which nothing else reads or writes.
- **`stdout_is_terminal()`, `stderr_is_terminal()`:** whether each is a
  terminal.

None is a handle for words a person reads: those are blocks on an
`Out`, or an `aside` and `ask_aside` from code that holds none.

## Blocks

Each block holds typed values, never styled text, so its look is the
formatter's alone. Each is built by a builder, so a field added later
moves no caller.

| Block | Holds |
|---|---|
| Success | What the command did, and what changed where |
| Fields | Labels and values, one perhaps the key |
| Table | Typed columns and rows, and what empty says |
| List | Bulleted typed values or paragraphs, and what empty says |
| Tree | Rows in a tree, each node's cells and children |
| Text | A paragraph in the docs' voice |
| Section | A heading over blocks of its own |
| Tail | Another program's last lines and its log |
| Next | Suggestions, each a command and why |
| Report | An error, a warning or a refusal |
| Verbatim | Lines to copy as they are, after a lead-in |
| Pairing | A code to type elsewhere, and when it expires |

### How they look

Every color, mark, indent, gap and branch is in one file, `look.rs`,
and a test holds each block's example in all three renderings. A table
uses the terminal's full width. When its columns do not fit, each row
shows as fields, preserving every value and diagnostic without cutting
text short. A pipe gets every character too. A list puts each value or paragraph on its own bullet and
wraps continuation lines under the item. Its JSON retains each item's
type.

### Uninstall notices

Uninstall uses lists for programs and distinct module notices, showing
repeated shutdown or retained-resource explanations once across modules.

## Streams

The result (a success, fields, a table, a tree, text) goes to stdout,
held to the end so the success line leads it. What's about the run
(progress, a report, Next, a question) goes to stderr at once. A pipe
gets only the result on stdout, and `--json` one document of every
block and the exit code. `--json` and `--stream-json` are `lnk`'s only
before `--`, after which they belong to the program it runs.

### A reply

A command another program reads gives `Out` its typed reply, and
`--json` prints exactly that on success, in the shape its readers
already parse; a person's run shows the blocks. Stdout only ever
holds a success's answer, so a program reading it never takes a
failure for one: on failure it's empty, and the report is plain text on
stderr, where a program running it reads why.

### What a reply escapes

A reply writes C1 control characters, and characters that hide text or
turn its direction, as `\uXXXX` escapes. A terminal shown the JSON then
draws what is in it, and a program reading it gets the same text.

### A reply a line at a time

A command whose `--json` is read a line at a time, such as a log, gives
`Out` each item as it comes. Each is printed at once as one JSON object
on its own line, and nothing is added after the last, so its readers
keep reading the same shape.

### A passed exit code

A command that runs another program as its own, such as a sandbox's
guest, ends with that program's exit code, as a shell would. The code
passes through untouched: it is not on the list of exit codes, and it
is not a failure of the command that ran it.

### A credential

A reply holding a credential, such as an access token, is a private
reply: refused on a terminal, where scrollback or a shared screen
would keep it, and printed only to another program.

### An inner run

A command another runs as part of its own, through `link_core::lnk`,
is an inner run: it shows its result and progress but no Next and no
report's footer, since what to do next is the outer command's to say.

### Now

A block a person needs while the command runs, such as a code to type
in a browser, shows at once on stderr rather than with the result. A
pairing code is one: the code in plain text, where to type it, and when it
expires. While stdout is JSON, such a block still shows on stderr,
plain, and is recorded too. The code itself has no emphasis or color,
so copying it into another app does not carry terminal formatting.

### Live output

Output a person keeps open, such as a log followed live or a chat,
shows each line at once, and `--json` writes one object a line. A
streamed piece of a line, such as a model's answer as it arrives, shows
and flushes at once, cleaned of control characters but its newlines and
tabs.

### Aside

Code that holds no `Out`, such as a library's warning or a step it
takes, writes a block aside. It shows at once on stderr, as an `Out`
shows what's about the run, styled on a terminal and plain elsewhere.

### A note

A note is a sentence about something that happened along the way,
neither progress nor a warning, such as a box tagged as yours in its
cloud. It goes to stderr, plain. Another thread writes one through a
handle the command lends it, so its notes show in their place.

## Words

Text in a block is a `Sentence`, a `Phrase` or `Prose`, each holding
spans of words and typed values, so a command inside one is still a
command and shows in bold. A debug build checks the words a command
writes: a sentence opens with a capital and ends with a stop, a phrase
has no end mark, and none holds a backtick, an escape or a second line.

## Values

A value knows how it shows and how it serializes: a name and its kind,
a product, a command, a path (home as a tilde), an address, a state, a
size, a duration, a time, money, a count and its noun, a version, a
secret (only ever its placeholder), text from another program,
cleaned of anything that could hide text, and a mark. A mark in a cell
or a field, such as a permission allowed or one beyond the defaults, is
never an emoji, so a table's columns stay lined up.

## States

A module declares its states with the `states!` macro: each variant's
word, lowercase, and its tone. The tone sets its color on a terminal,
green for good, yellow for worth a look, red for wrong, plain for as it
should be, so a module never picks a color itself.

## Reports

A report has a kind, a headline that says what happened, why, another
program's tail, what to do next, an exit code and a trace. An error
from `?` becomes a report too, its first cause the headline, so a
module can move to reports one function at a time.

### The trace

Release builds are stripped, so a backtrace there has no names.
Instead a report records its place in the code, and `within`, used
where `context` was, records each step's on the way up. A report's
`--json` carries its trace, so a module that runs another adds its
steps to the inner one's.

### Across module calls

A module that runs another through `link_core::lnk()` needs nothing
more for its report to hold the other's steps. The inner run, failing,
leaves its trace in a private folder under `~/Link/Logs` that
`LNK_INNER` names, and the outer report, if it fails too, takes the
latest one under its own steps, each marked with the command it came
from.

### The record

A bug record keeps a command's failure and the information needed to report it.

#### Destination Preparation

A machine can supply a preparation function for its record destination.
The formatter calls it only when writing a bug record, and leaves the
record unwritten if preparation fails. Selecting output and reporting a
user error do not prepare that destination.

#### Recorded Failure

A failure writes its record to `~/Link/Logs`: which `lnk`, the
platform, the command as typed, when, and the report with its trace, as
private JSON. Only a report that is Link's own fault
writes one: a panic, a report made a bug, or an error no command wrote
a report for. Its last lines name the record and ask the person to report it,
after a context line of the version, platform, command and the place in
the code that made it. A machine keeps its newest 20 records, and
one that can't be written never fails the command.

### A secret flag

A command declares the flags that take a secret with `Out::secret`, such as `--token`. What follows each, whether it is the next word or joined by `=`, shows as `<secret>` in the record's command line and in a retry's, since anything else would write the secret to disk or the screen.

### Into an issue

A record also keeps its issue: the bug form's fields, made as the
record is written, while its values are still typed. A name the person
chose is its kind and a number (`<agent 1>`), the same all through; a
path under home is `~/...`, anywhere else `<path>`; an address is
`<address>`. Every word is then scrubbed by pattern, a sentence adopted
from an error's text among them: the home folder, username, hostname,
IP and email addresses and anything shaped like a token. A line from
elsewhere, the trace's steps and a tail, that still looks like a secret
is left out whole and counted.

#### What it prints

`lnk issue create` prints the newest record's fields to paste, and `--open`
fills them in on the form. For each name in scope it asks the module
that diagnoses its kind for its state and adds it, scrubbed by the
types its JSON names, under the same stand-ins.

### What isn't Link's fault

A usage error, a refusal and a report a command wrote are the person's
to act on, not Link's to fix, so each ends on its Next with no context
line and no record.

### A program's exit

A `main` that doesn't print blocks ends through `link_core`'s
`style::exit`, which makes its error a report as `?` does, so its
failure looks like a block command's and writes its record too.

### A warning mid-run

A warning is a report the command goes on after. Written as a block, it
shows on stderr at once, in its place among the progress lines, and the
command still ends as it would have.

### A usage error

A program given a command line it doesn't answer, such as a flag
missing or with no value, fails with a `Usage` error where it reads its
arguments. Where it ends, that error becomes one report for every
program: what it wasn't given, its `--help` next, and exit code 2.

### Exit codes

Exit codes are one list in the package, each code one meaning:
success, failure, a usage error, a refusal, a module not installed,
the sandbox's two, an interrupt, and each module's own, declared once
so no number means two things. [Exit codes](../../modules/exit-codes.md)
is their public page, checked against the list.

## Questions

A question is asked on the terminal, its prompt in bold: text, a
hidden answer, yes or no, a choice by number, or a code typed back.
Its variable answers it without a terminal, a hidden answer's read as
a secret, so it still answers once taken out of the environment. With no terminal, or with
`--json`, it's never asked: a report says how to answer it on the
command line instead.

### What Enter means

A text question may have a default, shown in its prompt as what Enter
answers. A command that asks only when a person can answer checks
whether `Out` can ask first.

### Without asking

A yes or no question can take `--yes`, which answers it yes without
asking and is the flag a report names when no one can answer. That
confirmation flag takes no value; text and choice flags name their answer. A
question with a default can instead carry a sentence to show where it
can't be asked. That sentence shows as a note, the question takes its
default, and the command goes on.

### On another terminal

A question can be asked on a terminal held open by a thread while the
command runs, as a sandbox asks about a guest's request. It waits a
given time for an answer, and no answer refuses it. A code question
says whether it allowed or refused once it's answered.

## Progress

Progress is a counted step, a spinner and its clock for a wait with no
count, or a bar toward a total, drawn in place on a terminal, in ASCII: `[######----]`. A bar's
total may grow as it goes, as a download's does when it finds each
part, and what it's doing may change, in another program's words. A pipe
gets each line once, when it starts, and a bar's total when it ends.
An animated wait shortens a long description to keep its spinner and
clock on one terminal row, adjusting to the terminal's current width.
When the wait ends, its full description and elapsed time appear once.

### Waits on a Pipe

A wait with no count tells a pipe it's still going, once a minute, as
`[2/3] Saving the disk of aws-1, still going after 3m 0s`. So a log never
goes quiet for minutes while a cloud saves an image, and a runner that
ends a silent command can tell a long wait from a hung one.

### Ctrl-C

Ctrl-C during a counted flow ends the step it stopped at, never the person's terminal alone. The command prints `Stopped at step 2 of 3: Pulling qwen3:8b.` with a Next to start again, writes no record, and exits 130. Each step arms this, a finished command disarms it, and outside a flow Ctrl-C is the system's own.

## Settings

Output has four settings, kept under `[output]` in
`~/Link/Safe/settings.toml`. `color`, `emphasis` and `links` take `auto`,
`always` or `never`, and default to `auto`. `symbols` takes `emoji`,
`plain` or `none`, and defaults to `emoji`.

### Each a variable

Every setting is also a variable, its name made from its table and key
in capitals: `[output] color` is `LNK_OUTPUT_COLOR`. A variable wins
over the file, and the file over the default. `NO_COLOR` turns color
off and `CLICOLOR_FORCE` on, over both. A command that doesn't
print blocks reads only the variables.

#### Presentation Overrides

`LNK_OUTPUT_EMPHASIS` and `LNK_OUTPUT_LINKS` override their respective settings
with `auto`, `always`, or `never`. `LNK_OUTPUT_SYMBOLS` selects `emoji`, `plain`,
or `none`. These inputs affect presentation alone, so a pipe still receives the
command's selected output format rather than an interactive terminal screen.

# Run Tool Command Line

`link-run-tool-contract` gives the callers of Link run, `lnk-run-tool`,
each command's runtime arguments as a Rust type and the command line that
gives them. Both are generated from the tool's declarations, its
`actions.toml` and `types.toml`, as is the parser the tool reads that line
with, so a flag is spelled in one place.

## Commands

Link runs every command, never a person, so each is plumbing, hidden
from help. The agent module builds `serve`, whose sandbox each call runs
under comes after `--` and is required. The tool itself builds the rest:
`supervise <id> --state <folder>`, which runs one background command to
its end, `gate`, which the supervisor starts the command through, and
`sweep`, `stop` and `delete`, each over the records in `--state`.
`install` downloads nothing.

## Background Commands

`serve`'s background flags come together or not at all: `--state` needs
`--way-in`, `--as` and every cap (`--background`,
`--background-timeout`, `--background-output`, `--background-keep`,
`--background-per-day` and `--hops`), and each of those, or `--keyring`,
is refused without `--state`. The rule is the declaration's check hook,
`background`, which this crate holds once for the arguments a caller
builds and the ones the tool reads.

## Building a Command Line

A caller fills a command's arguments type, as `RunToolServeArgs` or
`RunToolSuperviseArgs`, and runs the program with the words its `argv`
writes: the command, its positionals in the order they're declared, then
its flags, and last `--` and the sandbox. `lnk-run-tool` reads them back
through its generated parser, so it reads exactly what its caller meant.

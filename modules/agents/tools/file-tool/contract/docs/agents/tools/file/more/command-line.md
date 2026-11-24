# File Tool Command Line

`link-file-tool-contract` gives the callers of Link file, `lnk-file-tool`,
each command's runtime arguments as a Rust type and the command line that
gives them. Both are generated from the tool's declarations, its
`actions.toml` and `types.toml`, as is the parser the tool reads that line
with, so a flag is spelled in one place.

## Commands

Link runs every command, never a person, so each is plumbing, hidden
from help: `serve` and `apply`, which the agent module builds from the
agent's sandbox file and teams, and `bridge`, which Link's machines that
keep Home in a bucket build. `install` downloads nothing.

## Teams Through the Files Bridge

`serve` runs its calls in the command after `--`, which is required.
Its `--team-bridge` flags name teams reached through the files bridge,
and need `--socket`, `--as` and `--account`; those, and `--keyring`, are
refused without one. The rule is the declaration's check hook,
`team_bridge`, which this crate holds once for the arguments a caller
builds and the ones the tool reads. `apply`'s `--team-folder` is
`<name>=<folder>`, and its `--team-write` and `--team-read` are
`<name>/<folder>`.

## Building a Command Line

A caller fills a command's arguments type, `FileToolServeArgs`,
`FileToolApplyArgs` or `FileToolBridgeArgs`, and runs the program with
the words its `argv` writes. `lnk-file-tool` reads them back through its
generated parser, so it reads exactly what its caller meant.

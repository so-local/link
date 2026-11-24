# Native Program

The Signal adapter installs and links `signal-cli` for the harness and its bridge. Link runs these native setup commands; a user connects the account through the [Signal guide](../guide.md).

## Where it runs

The Signal module installs and links it (`lnk-signal cli install`, `cli
link`), for the adapters as for its own bridge, so nothing about
`signal-cli` is in the library every module links. For a harness
speaking Signal, the harness module runs both in that harness's sandbox,
reaching its adapter's hosts, rather than outside it: the program lands
in a home the harness can write, so running it outside the sandbox would
run what the harness wrote. The adapter is handed the program's path in
Signal's settings.

# Code Tool Command Line

`link-code-tool-contract` gives the callers of `lnk-code-tool` its
runtime arguments as types, `CodeToolServeArgs`, and the command line
each one's `argv` writes, generated from the tool's `actions.toml`, as
the parser the tool reads it with is. The agent module builds `serve`,
whose sandbox for each language server, after `--`, is required.

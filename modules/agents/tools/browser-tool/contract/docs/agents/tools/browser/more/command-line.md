# Browser Tool Command Line

`link-browser-tool-contract` gives the callers of `lnk-browser-tool` its
runtime arguments as types, `BrowserToolServeArgs`, and the command line
each one's `argv` writes, generated from the tool's `actions.toml`, as
the parser the tool reads it with is. The agent module builds `serve`,
whose sandbox, after `--`, is required.

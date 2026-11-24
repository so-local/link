# Code

> Code Your Agent Checks with Language Servers

**Your agent asks the project's language servers what is wrong in a
file, where a name is defined and where it's used, instead of searching
the text.** When you ask your agent to fix the errors in your Rust crate,
or to find every call to a function before renaming it, the agent asks
`rust-analyzer`, as an editor would.

<!-- quickstart -->

## Add Code to Your Agent

Start your agent and enable the code tool, then review any requested permissions. Check what Code [requires and what the sandbox grants](guide.md).

```sh
lnk agent start
lnk agent tool add code
```

## List Your Agent's Tools

Check which tools your agent has enabled, and confirm that Code is on the list.

```sh
lnk agent tool list
```

## Ask Your Agent to Use Code

Ask your running agent to inspect a project with the project's language servers. Learn [what Code can do and how to configure access](guide.md).

```sh
lnk agent chat
```

Local Link starts `lnk-code-tool` itself and [builds its command line](../../../../contract/docs/agents/tools/code/more/command-line.md) from the tool's own types. You never type the tool's commands.

## Learn How Code Works

Read why Code is built as it is in [Decisions](more/decisions.md), how the tool's process and calls fit together in [Developing](more/developing.md), and what to do when the tool fails in [Troubleshooting](troubleshooting.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Code Guide](guide.md): Enable Code to ask installed language servers for diagnostics, definitions, and references

### More

- [Developing](more/developing.md): `lnk-code-tool` is Link Code: an MCP server over stdio that asks a project's language servers for diagnostics, definitions and references, each server in a sandbox
- [Decisions](more/decisions.md): Why `code` asks language servers, and why each runs in a sandbox with no network
- [Troubleshooting](troubleshooting.md): What to do when [your agent has no `code`](README.md), or a language server doesn't start or answer

<!-- docs-tree:end -->

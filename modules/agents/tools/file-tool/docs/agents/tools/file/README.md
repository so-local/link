# Files

> Files Your Agent Reads, Writes, and Edits

**Your agent reads, writes, and changes the files in the agent's files
folder, as the agent's sandbox file allows.** Ask your agent to keep a
list, fix a typo in a note, or find where the agent wrote something.

<!-- quickstart -->

## Add Files to Your Agent

Start your agent and enable the file tool, then review any requested permissions. Check what Files [requires and what the sandbox grants](guide.md).

```sh
lnk agent start
lnk agent tool add file
```

## Ask Your Agent to Read or Edit a File

Ask your running agent to read or edit a file in the agent's files folder. Learn [what Files can do and how to configure access](guide.md).

```sh
lnk agent chat
```

Local Link starts `lnk-file-tool` itself and [builds its command line](../../../../contract/docs/agents/tools/file/more/command-line.md) from the tool's own types. You never type the tool's commands.

## Learn How Files Works

Read why Files is built as it is in [Decisions](more/decisions.md), how the tool's process and calls fit together in [Developing](more/developing.md), and what to do when the tool fails in [Troubleshooting](troubleshooting.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Files Guide](guide.md): Enable File to read, write, edit, and find files within your agent’s granted folders

### More

- [Developing](more/developing.md): `lnk-file-tool` is Link File: an MCP server over stdio that reads and writes an agent's files, its calls in a sandbox
- [Decisions](more/decisions.md): Why `file`'s calls are Claude Code's, why it keeps to `[files]` before its sandbox does, what it narrows per call, and how it works through a files bridge and in a bucket
- [Troubleshooting](troubleshooting.md): What to do when your agent can't write or read one of its files

<!-- docs-tree:end -->

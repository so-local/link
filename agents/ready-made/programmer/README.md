# Programmer

Programmer is a ready-made coding agent that explores, plans, tests, reviews,
and commits its own changes in one repository, in Rust, Python,
TypeScript, Go, Java and Kotlin, or C and C++. The full guide is
[Programmer](../../docs/agents/ready-made/programmer.md) in Local Link's docs.

## Who Programmer Is For

Programmer is for a developer who wants a task taken from a request to a
reviewed commit on a branch, with tests to prove it. Programmer reads the
repository's own `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`, or
`README.md` first and follows it. Programmer never pushes, merges, or
releases unless you ask, and ends every task with a report of what
changed, what it ran, and what it couldn't verify.

## What Programmer Comes With

Programmer runs on Link Harness, stays awake for the next task, and thinks
at medium effort, with 40 rounds and 80 tool calls a turn and up to 10
minutes a call. Its model is `claude-sonnet-5-5` on Anthropic, the
current Sonnet, since a change spans many files. Its persona is a
careful senior engineer.

- **Tools:** run for builds, tests, and Git, file for edits, scheduler, code for language servers, and Link MCP for Link's manual.
- **Workflow skills:** `explore-codebase`, `plan-change`, `write-tests`, `debug`, `refactor`, `fix-ci`, `dependencies`, `security-check`, `write-docs`, `performance`, `review-diff`, and `commit-message`.
- **Language skills:** `rust`, `python`, `typescript`, `go`, `java` for Java and Kotlin, and `c-cpp`.

## Bring Programmer In

Download `link-programmer.tar.gz` from Local Link's
[latest release](https://github.com/so-local/link-releases/releases/latest), then import it from the folder you saved it in.
Import checks the whole
archive, asks whether to keep the name `programmer`, and places the agent in
`~/Link/Agents/programmer/`, with its settings in `~/Link/Safe/Agents/programmer/`.
Nothing is installed, connected, or started until you say so.

```sh
lnk archive import link-programmer.tar.gz
```

From a checkout of Local Link's repository, pack this folder as the
release does, then import `~/Link/Archive/programmer.tar.gz`.

```sh
lnk archive export --from agents/ready-made/programmer
```

## Start Programmer and Chat

Start Programmer with `--run.files` naming your repository, which becomes its
files folder. The first start asks for your Anthropic credential. Then
ask for one change, such as
`Fix the failing date parsing test and commit the fix`.

```sh
lnk agent start --agent programmer --run.files ~/code/app
lnk agent chat --agent programmer
```

## What Programmer Can Reach

Programmer's sandbox writes only the repository and its own instructions and
skills, reaches any public host through its proxy, and runs its
toolchains' programs and what their builds make. It writes the
repository's `.git` so it can commit. A hook or Git setting written
there runs as you the next time you run Git, so review `.git/hooks` and
`.git/config`, or take the grant back. Your Git credentials and `~/.ssh`
stay outside the sandbox, so you push.

```toml
# ~/Link/Safe/Agents/programmer/sandbox.toml
schema = 1

[files]
write = [".", "instructions", "skills"]
write_dot_files = ["."]

[internet]
hosts = ["*"]

[commands]
toolchains = ["shell", "git", "rust", "node", "python", "go", "jvm", "c"]
```

```sh
lnk agent show --agent programmer --permissions
lnk agent permission remove --agent programmer --files.write_dot_files .
```


## Make Programmer Your Own

**A coding agent you own and shape can be the best one you've used.** An imported agent is yours. Edit its standing instructions in
`~/Link/Agents/programmer/agent/instructions/instruction.md`, or ask the agent
to, since its sandbox writes them. Add or remove skills and tools, and
change one key of its specification with a field flag on
`lnk agent start`, which the agent keeps.

```sh
lnk agent skill list --agent programmer
lnk agent tool add memory --agent programmer
lnk agent spec show programmer
```

Rules for one project belong in that repository's `AGENTS.md`, which Programmer reads before it edits.
[Make an Agent Your Own](../../docs/agents/ready-made/README.md#make-an-agent-your-own)
covers each change, and
[Share an Agent](../../../modules/agents/agent/docs/agents/share/export.md#share-your-agent)
shows how to export your version for others.

## This Folder

This folder is an archive kept as a folder. `archive.toml` names the
agent and lays out its folders: `Agents/programmer/` holds its instructions,
skills, and Home, and `Safe/Agents/programmer/` holds its `agent.toml` and
`sandbox.toml`. This README sits beside `archive.toml`, travels in the
released archive, and is left out on import, so the agent's files stay
exactly what the archive names. The Home starts blank, with headings and
empty fields only, and fills as you talk to the agent.

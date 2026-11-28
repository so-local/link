# Programmer

Programmer is a ready-made coding agent that explores, plans, tests, reviews, and commits its own changes in one repository. Programmer works in Rust, Python, TypeScript, Go, Java and Kotlin, or C and C++, and leaves every push to you.

## Who Programmer Is For

Programmer is for a developer who wants a task taken from a request to a
reviewed commit on a branch, with the tests to prove it. It reads the
repository's own `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`, or
`README.md` first and follows that guide over its own instructions. A
request for a plan, a review, or an audit alone gets that result, with
nothing implemented or committed.

## Start Programmer in a Repository

Import Programmer as [Ready-Made Agents](README.md#bring-one-in) describes,
then start it with `--run.files` naming your repository, which becomes
its files folder. Its first start asks for your Anthropic credential.

```sh
lnk agent start --agent programmer --run.files ~/code/app
lnk agent chat --agent programmer
```

### Give Programmer a Task

Ask for one change, such as
`Fix the failing date parsing test and commit the fix`. Programmer reads
`git status` and the recent log, explores the code, states a plan, and
works on a branch of its own, each step building and passing the tests.
Its report says what changed with the commit hashes, the commands that
verified it, and anything not verified or still failing.

## What Programmer Comes With

Programmer runs on Link Harness with `claude-sonnet-5-5` on Anthropic, the
current Sonnet, at medium thinking. Its settings allow 40 rounds and 80
tool calls a turn, and let each call wait up to 10 minutes, since a
build or a test suite outlasts Link's defaults. Its tools are run, for
builds, tests, and Git, file, scheduler, code, which answers from language
servers, and Link MCP, which reads Link's manual.

### Workflow Skills

Twelve skills cover the work itself, in any language, each read when
its situation comes up.

- `explore-codebase` finds the entry points, build system, tests, and conventions.
- `plan-change` plans what changes, where, how it's tested, and what could break.
- `write-tests` writes fast, focused tests first, in the repository's framework.
- `debug` reproduces a bug, finds the cause, and adds a regression test.
- `refactor` restructures code in small steps with the tests green between each.
- `fix-ci` reads a failing run's log and reproduces the failing step locally.
- `dependencies` adds, upgrades, or removes a dependency with its lockfile.
- `security-check` looks for injection, leaked secrets, and path traversal.
- `write-docs` updates the README, doc comments, and guides a change makes stale.
- `performance` measures before and after one change at a time.
- `review-diff` reviews the whole diff for correctness, tests, security, and scope.
- `commit-message` commits one logical change in the repository's own style.

### Language Skills

Six skills hold each toolchain's commands and idioms: `rust`, `python`,
`typescript`, `go`, `java` for Java and Kotlin, and `c-cpp`. Programmer picks
the one the repository's files call for and checks the toolchain's
version first.

## What Programmer Can Reach

Programmer's sandbox writes only the repository and Programmer's own instructions
and skills, and reaches any public host through its proxy, since
package registries and documentation vary by project. It runs its
toolchains, `shell`, `git`, `rust`, `node`, `python`, `go`, `jvm`, and
`c`, with the programs a build makes under the repository. A toolchain
in your home folder, such as rustup's, is found on the `PATH` of its
last start.

### Commits and the Repository's Dot Files

To commit, Programmer's sandbox file grants the repository's `.git`
(`write_dot_files`). A hook or Git setting written there would run as
you the next time you run Git, as
[What It Opens](../../../../modules/agents/agent/docs/agents/more/settings/README.md#what-it-opens) explains,
so review a change's `.git/hooks` and `.git/config`, or take the grant
back and Programmer leaves its finished commit message to you. Your Git
credentials and `~/.ssh` stay out of the sandbox, so Programmer pushes only
to a remote that asks no login and leaves other pushes to you.

```sh
lnk agent permission remove --agent programmer --files.write_dot_files .
```

## Make Programmer Your Own

**A coding agent you own and shape can be the best one you've used.**
Programmer's instructions, skills, and sandbox are files you can change,
so Programmer can work the way your team works.
The repository's own `AGENTS.md` or `CLAUDE.md` reaches Programmer before it
edits, so rules for one project belong there. A skill for your team's
toolchain or review habits goes in Programmer's skills folder, as
[Make an Agent Worth Sharing](../../../../modules/agents/agent/docs/agents/share/worth-sharing.md) describes.

## Contribute Here

**A skill for planning, testing, or reviewing in your language makes
Programmer better for everyone.** Programmer's instructions, skills, and
sandbox are files in `agents/ready-made/programmer/`, packed on every
release, so an improvement is an ordinary pull request. Send us the
skill that made your copy better.

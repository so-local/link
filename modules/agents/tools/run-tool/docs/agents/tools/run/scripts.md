# Run Scripts

Give your agent Python or JavaScript, the two languages models write
best, and it edits many files at once, works out sums and statistics,
and queries tables, in a sandbox that reaches only what you grant.

## Why Your Agent Wants a Script

A script does in one call what File does in many. Ask your agent to
"rename Acme to Northwind in every note, but not in Acme's own folder",
and it writes ten lines of Python instead of editing each file in turn.
Ask for "the median order in work/sales.csv" or "the integral of x² sin
x", and it computes the answer rather than guessing it.

### What scripts need

You need Link Run, which [`lnk agent tool add run`](../../../../../../agent/docs/agents/README.md#tools)
installs and turns on, and Python or Node.js on the machine your agent
runs on. Run finds a program on the `PATH` your agent was started with,
and a language this machine lacks is never offered.

```sh
lnk agent tool add run
```

## Grant Python or JavaScript

Your agent's sandbox file names what Run may start. A toolchain names a
language's programs in one word, and
[the sandbox specification lists each toolchain](../../../../../../../security/sandbox/docs/config/specs/sandbox.md#toolchains)
with what it grants. `python` grants Python and its project tools, such
as `pip` and `uv`. `node` grants Node.js and its package managers, such
as `npm`, `pnpm`, and Bun. Run needs the shell too, so `shell` stays in
the list.

```toml
# ~/Link/Safe/Agents/main/sandbox.toml
schema = 1
[files]
write = ["work"]
[commands]
toolchains = ["shell", "python", "node"]
```

```sh
lnk agent start --sandbox sandbox.toml
```

### Other languages

Go, Rust, C and C++, and Java and Kotlin each have a toolchain too, and
each is granted the same way. Most personal agents need only Python or
JavaScript, and a coding agent takes the language of its project.

## Give Your Agent Libraries

A library is code another person wrote for a task, such as reading a
spreadsheet or solving an equation. Each language keeps a project's
libraries in a folder beside the file that lists them, and the sandbox
runs a library's programs from that folder wherever your agent may
write.

### Python's libraries

Python keeps them in a virtual environment, a `.venv/` folder beside a
`requirements.txt` that lists them. Write the list in `work/`, then make
the environment and install the list there yourself, with
`python3 -m venv .venv` and `.venv/bin/pip install -r requirements.txt`
run in that folder, or with `uv`. Your agent then runs its scripts with
`.venv/bin/python`, which sees the libraries.

```text
# ~/Link/Agents/main/home/work/requirements.txt
sympy
pandas
numpy
scipy
```

### JavaScript's libraries

Node.js keeps them in a `node_modules/` folder beside a `package.json`
that lists them. Write the list in `work/`, then install it there
yourself with `npm install`, `pnpm install`, or `bun install`.

```jsonc
// ~/Link/Agents/main/home/work/package.json
{
  "private": true,
  "dependencies": {
    "mathjs": "^14.0.0",
    "papaparse": "^5.4.0"
  }
}
```

### Let your agent install libraries

Your agent can install a library itself once its sandbox reaches the
library's registry: `pypi.org` and `files.pythonhosted.org` for Python,
and `registry.npmjs.org` for JavaScript. An agent that installs
libraries brings code from the internet into its files, so it can't
reach a registry until you grant its hosts.

```toml
# ~/Link/Safe/Agents/main/sandbox.toml
schema = 1
[files]
write = ["work"]
[commands]
toolchains = ["shell", "python"]
[internet]
hosts = ["pypi.org", "files.pythonhosted.org"]
```

## What Libraries Give

Python's own library already covers a great deal before you install
anything. `re` and `pathlib` change text across many files, `statistics`
gives means and medians, and `csv` and `sqlite3` read a table and query
it with SQL.

### Mathematics

SymPy gives exact answers to integrals, derivatives, equations, and
trigonometry, as a computer algebra system does. In JavaScript, math.js
evaluates expressions, units, and matrices.

### Data

pandas reads a spreadsheet or a CSV file into a table your agent sorts,
groups, and sums. NumPy and SciPy add arrays, statistics, and the
methods of science and engineering. In JavaScript, Papa Parse reads a
CSV file.

## What the Sandbox Holds

A script runs in Run's sandbox, as every command does. It reads your
agent's files folder, writes only the folders `[files]` lets it, starts
only the programs `[commands]` names, and reaches only the hosts
`[internet]` names. [Choose what it may run](guide.md#choose-what-it-may-run)
has the whole of it.

### Narrow your agent's internet

A script reaches whatever your agent's internet does. The personal
ready-made agent reaches every host, so a script it writes could send
your files anywhere. That's why it doesn't have Python or JavaScript
until you add them. Before you add either, narrow `[internet] hosts` to
the sites your agent needs, as the sandbox file above does.

### Loosen the sandbox on a machine you trust

On a machine of your own, where your agent's work is yours alone, grant
more: `write = ["."]` for the whole files folder, and the hosts its
scripts fetch from. The sandbox still holds what you grant, and nothing
it leaves out.

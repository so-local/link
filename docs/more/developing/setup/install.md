# Install `lnk`

Get `lnk` from a release, from source with cargo, or straight from a
build, and switch between them without losing your agents or files.

## Three Ways

Pick one and keep to it. `lnk` runs its modules (`lnk-agent`,
`lnk-box`, ...) from its own folder first and then the PATH, so two
installs on one PATH mix versions.

| Way | `lnk` and its modules live in | Modules come from | Update with |
|---|---|---|---|
| [The script](#the-script-a-release) | `~/.lnk/exec/bin` | [`lnk up`](../../../../core/cli/docs/more/architecture/files.md#source-module-installation), [`lnk mod add`](../../modules/guide.md#add-or-remove-a-module) (signed release) | [`lnk upgrade`](../../modules/guide.md#upgrade) |
| [Cargo](#cargo-from-source) | `~/.cargo/bin` | Selected source modules from your checkout | `install.sh --source`, `lnk up --source .` |
| [No install](#no-install-targetdebug) | `target/debug` | `cargo build` builds the core; `-p` selects other modules | `cargo build` |

## The Script (a Release)

The script installs a signed release, and `lnk` then adds and updates
its modules from releases too. [Install](../../modules/install.md) is
that whole route for a person running Local Link, from what the machine
needs to the first modules.

## Cargo (from Source)

Install Rust and Cargo with [rustup](https://rustup.rs), then run the
bootstrap, `sh core/cli/install.sh --source`, from the repository root.
It installs only the CLI on your PATH, normally in `~/.cargo/bin`. Add
modules with the installed CLI, from the same folder.

```sh
lnk up --source .
```

### Build Prerequisites

The first source build needs your platform's compiler and development
libraries. The bootstrap installs release-mode binaries rather than
selecting the programs in `target/debug`, and it never downloads
released modules.
Unsupported platforms are skipped according to each package's source metadata.

### Update Core and Modules

Run `lnk up --source .` again to update core and its modules together.
Run the bootstrap again only for an older CLI that doesn't update itself
or doesn't support source installation. Both use Cargo's installation records and reuse build output.
A failure stops the command and names the package. Earlier installations
remain. Neither command starts or restarts agents.

### Include Another Workspace

The source command also accepts module-only workspaces. Repeat `--source`
to include additional workspaces in the same installation:

```sh
lnk up --source . --source /path/to/addons
```

### Validate the Combined Installation

All selected workspaces are checked before installation starts. Duplicate
workspace paths and conflicting executable names are refused. Build output
is shared in the first workspace's target directory. Core is excluded from
module installation. Module discovery scans only explicitly named workspaces,
while builds also read their dependencies. Installing does not publish code.

### Select Modules

Separate names with commas or repeat `--module` to install only named modules. Names are executable
names without the `lnk-` prefix, such as `agent` or `manual`, rather than
Cargo package names. Each named module brings every module its `module.toml`
lists as a module dependency, and theirs in turn. Unknown names and explicitly
selected unsupported modules are refused before any installation changes.

#### Its Own Checkout Comes Too

A `lnk` built from source always installs from its own checkout as well, so
`lnk up --module agent` needs no `--source`, and `--source` names only the
other workspaces. That holds while it's where the checkout put it, in the
checkout's target folder or a Cargo installation made from the checkout. A
copy moved anywhere else installs only the workspaces `--source` names.

```sh
lnk up --source . --module agent,man,model
```

### Where Modules Go

A source-built `lnk` puts its modules where it is itself, so it finds
them beside it and never mixes them with a second install. Run from a
Cargo target folder, such as `./target/debug/lnk`, it builds each module
with `cargo build` into that folder and profile. Run from a Cargo
installation's `bin`, such as `~/.cargo/bin`, it installs each with
`cargo install --root` into that installation. Anywhere else it changes
nothing, and names where it is and what to run instead, since a module
put elsewhere would be one `lnk` might not find, or might find stale.

### Refresh Installed Modules

Use `--stale` to refresh only modules already beside the running
CLI or on PATH, whatever build each file is from. Cargo checks source, dependencies, build scripts, compiler
and build settings, then builds changed inputs. The command compares the
resulting executable with the installed file and skips identical files.
Missing modules remain uninstalled. Add `--module` to narrow the refresh.

```sh
lnk up --source . --stale
lnk up --source . --module agent --stale
```

#### Core Updates First

A `lnk` built from source rebuilds itself from its checkout first, on
every `lnk up` that installs from source. It and its modules then always
come from one build, so neither refuses the other as built from other
sources.

#### Refresh Locations and Failures

A changed module is replaced [where `lnk`'s modules go](#where-modules-go). In a target folder, Cargo
rebuilds it in place. Unchanged executables need no installation write. A failed build leaves that module in place and
stops the refresh. A dependency change whose output is identical requires
no replacement. A source-built `lnk` updates itself first, as above.

### Without a Checkout

Without a checkout, Cargo installs from a branch, where `main` is
everything merged, released or not. With `CARGO_NET_GIT_FETCH_WITH_CLI`
set to `true` in the environment, run `cargo install --git
https://github.com/so-local/link --branch main link-cli link-agent
link-manual`.

### Harnesses and Connections

The source installer includes the workspace's supported harness adapters and connection
modules. It installs Link's adapters, not the external harness programs or
accounts those adapters use. A module from another checkout can be installed
with `cargo install --path /path/to/module`.

### Add Modules with Cargo

With a Cargo install, use `lnk up --source .` for all source modules,
or add `--module <name>` for selected modules. A source-built `lnk`
records the checkout it was built from, so plain `lnk up`,
`lnk mod add` and the "Install it?" offer of a missing module build
from that checkout the same way, never downloading a release. If the
checkout has moved or been deleted, they name the `lnk up --source`
command with the path they expected. Don't run `lnk upgrade`, because
it replaces the programs in `~/.cargo/bin` with a release's.

## No Install (`target/debug`)

Run directly from build output when you do not want to install programs
on your PATH. Run `cargo build -p link-cli -p link-manual`, `./target/debug/lnk --version`, then `./target/debug/lnk mod list`.

### Other Packages

Build other desired packages with `cargo build -p <package>`, or select
all workspace members with `cargo build --workspace --bins`.
`./target/debug/lnk up --source .` builds every module supported on your
host into `target/debug`, and `./target/debug/lnk mod add <module>`
builds one there. A `lnk` built from source never installs a released module of Link's
tools on its own. Without `lnk-scheduler-tool`, `lnk-run-tool` or
`lnk-file-tool` built next to it, an agent starts without that tool
and says so.

## Switch Between Them

Your agents, settings and files (`~/.lnk`, `~/Link`) are shared by
every install and kept by default. These are shared data folders, not
isolated production and development environments. [`lnk uninstall --yes`](../../modules/guide.md#uninstall)
stops services. Script installations also remove their programs. Cargo
installations print a `cargo uninstall` command you must run to finish.
`--yes` only confirms the prompt. Start the agents again with
the new install, because a running agent's service points at the old
one's programs.

### Script to Cargo

Remove the script's programs, install with cargo as above, with
`sh core/cli/install.sh --source` between the first two commands, and
start your agents again.

```sh
lnk uninstall --yes
lnk up --source .
lnk agent start
```

### Cargo to the Script

Remove the programs cargo installed, run the script, and start your
agents again:

```sh
lnk uninstall --yes
```

Run the complete `cargo uninstall` command it prints, including the module
packages. The core package is `link-cli`, not `lnk`. Keep `~/.cargo/bin` on
PATH because other Rust tools use it. Then install the release:

```sh
curl -fsSL https://local.link/install.sh | sh
lnk up
lnk agent start
```

### Check It Worked

One `lnk` should be on the PATH, with its modules beside it, and
`type -a lnk` lists every one your shell finds.

```sh
lnk mod list
lnk --version
```

If your shell remembers the previous executable, open a new terminal or
run `rehash` in zsh, or `hash -r` in bash. Another executable path means
another installation still exists. Uninstall that copy before restarting
agents with the intended installation. With two on the PATH, every `lnk` command in a terminal starts with a
note naming both.

## More About It

What to do when two installs mix, or a module won't install, is in
[Troubleshooting](../troubleshooting.md). Install options
(`LNK_INSTALL_DIR`, `LNK_VERSION`, ...) are in
[Files and Variables](../../../../core/cli/docs/more/architecture/files.md#installsh).

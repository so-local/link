# Shell Completion

Shell completion lets your shell finish `lnk` commands, their flags,
and a flag's values when you press Tab, each shown with its help in zsh.

## Turn On Completion

The scripts are in this repository's `scripts/completion/` folder.
In bash 4 or later, source `lnk.bash` from `~/.bashrc`, and in zsh,
source `lnk.zsh` from `~/.zshrc`, after `compinit` runs, each by its
full path in your checkout. Each script is one file, so it works the
same from wherever your checkout is.

## What Completes

Each command a published module offers a person on the command line
completes, word by word, from `lnk` through its resource and verb. After
a command, Tab offers its runtime flags and, for a command that reads a
specification, `--spec` and its field flags. A flag whose values are a closed
set, such as `--model.anthropic.thinking`, completes to those values.
Plumbing commands, which other programs run, are left out.

## How Completion Is Made

The scripts are generated, never written by hand. The
[`link-actions-surfaces`](../../../../../packages/actions/surfaces/README.md)
completion generator reads the catalog of every module's declarations,
the same one `lnk`'s parsers and help come from. A command's runtime
flags come from its declared arguments, and its field flags from its
bound specification, as its help lists them. The docs check refuses a script
that is absent, stale, or edited by hand, and `lnk-docs-inventories
--write` writes it again from the declarations.

<!-- docs-tree:start -->

## Docs

- [Turn It On](#turn-on-completion): In bash 4 or later, source the bash script from `~/.bashrc`
- [What Completes](#what-completes): Each command a published module offers a person on the command line completes, word by word, from `lnk` through its resource and verb
- [How It's Made](#how-completion-is-made): The scripts are generated, never written by hand

<!-- docs-tree:end -->

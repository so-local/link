# Troubleshooting

What to do when [installs of `lnk` mix](setup/install.md), a module won't install, or a
pasted command misreads.

## zsh passes a pasted `# note` to the command as arguments

`lnk` ignores plain words, but zsh still reads a `?`, `'`, `(` or `;`
in one. `setopt interactive_comments` in `~/.zshrc` fixes both.

## `which -a lnk` shows two, or `lnk` notes another on your PATH

[Uninstall one of the two installs](setup/install.md#switch-between-them),
or put the one you want to run first on the PATH.

## A command says its module isn't installed

Run [`lnk mod add <module>`](../modules/guide.md#add-or-remove-a-module). From the script it installs the
release's module, and from cargo it builds the module from the checkout
`lnk` was built from.

## `lnk mod add` says the checkout isn't there any more

Your `lnk` is from source, and it installs modules from the checkout it
was built from, which has moved or been deleted. Run the
[`lnk up --source <checkout> --module <module>`](setup/install.md#cargo-from-source) it names, with the
checkout's path now, or build `lnk` again from where the checkout is.

## `lnk` says it's neither in a Cargo target folder nor a Cargo installation

A source-built `lnk` [puts its modules where it is](setup/install.md#where-modules-go), and it was copied
somewhere Cargo doesn't build or install into. Run the `lnk` in your
checkout's `target/debug`, or install it with
`sh core/cli/install.sh --source`.

## A module "was built from other sources than this lnk"

The module and `lnk` are release builds from different commits of your
checkout, which both report the same version and may disagree. Run
`lnk up --source <checkout>` to [build them together](../../../core/framework/docs/more/modules/protocol.md#built-from-the-same-sources).

## "The <module> module at ... isn't in lnk's folder"

It's from the other install: remove it the way it was installed, so two
installs don't mix versions on one PATH.

## Agents fail to start after switching

Run [`lnk agent start`](../../../modules/agents/agent/docs/agents/README.md#start-your-agent) again, from the new install: a running agent's
service points at the old install's programs.

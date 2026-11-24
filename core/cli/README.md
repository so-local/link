# Command Line

`lnk` is the command-line entry point for Local Link programs. It installs
Link modules, routes commands to their executables, and updates or removes
the installed parts.

<!-- docs-tree:start -->

## Docs

### Pages

- [Files and Variables](docs/more/architecture/files.md): The files and environment variables every `lnk` program reads, and those of the core and its installer
- [Shell Completion](docs/more/reference/completion.md): Shell completion lets your shell finish `lnk` commands, their flags, and a flag's values when you press Tab, each shown with its help in zsh

### References

- [Core Commands](docs/more/reference/modules/core.md): Every command the `core` module offers, from its declarations, in order of the resource each acts on

<!-- docs-tree:end -->

## Code

- [`cli.rs`](src/cli.rs): the core's own commands
- [`install.sh`](install.sh): the installer
- [`main.rs`](src/main.rs): the program

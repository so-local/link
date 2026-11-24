# Completion

Completion holds `lnk`'s shell completion for bash and zsh, generated from
every module's declarations. [Shell Completion](../../core/cli/docs/more/reference/completion.md)
says how to turn it on.

## Code

- [`lnk.bash`](lnk.bash): completion for bash 4 or later, sourced from `~/.bashrc`
- [`lnk.zsh`](lnk.zsh): completion for zsh, sourced from `~/.zshrc` after `compinit`

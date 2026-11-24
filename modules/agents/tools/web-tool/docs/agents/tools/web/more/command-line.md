# Web Tool Command Line

`lnk-web-tool`'s command line is declared in the module's `actions.toml`,
and its parser is generated from that file when the tool builds, so the
commands a harness runs and the ones the tool reads can't differ.

## Commands

A harness runs `lnk-web-tool serve`, never a person, so both its
commands are plumbing, hidden from help. `install` completes without
downloading anything, since the program is the tool. `serve`'s one flag,
`--private-host`, names a host on a private network it may still reach,
repeated for more; a host with a blank in it is refused.

## Outputs

Each command's answer is declared in `types.toml`: `serve` ends with an
empty `served` once its caller closes its input, and `install` answers
an empty `installed`. Those and the answers of web's calls, `page` and
`results` with each `hit`, are generated into the tool's contract crate,
[`link-web-tool-contract`](../../../../../contract/README.md), so the
tool and a module naming them, such as the agent's bridge offering web's
calls as routes, read one Rust type.

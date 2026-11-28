# Variables

`lnk variable list` lists the environment variables Local Link's
commands read, by name, with the commands that read each and what it's
for. It never reads a variable's value, so it shows nothing private.

## List a Command's Variables

Alone, `lnk variable list` lists every command's variables, from the
core and this build's modules. Pass command words to narrow the list to
one command, with the variables of the program and the commands above it
included. An unknown command is refused, and `--json` gives an array of
entries with `name`, `command`, and `about`.

```sh
lnk variable list
lnk variable list agent start
```

## Every Variable

[Variables](../../../../docs/more/reference/variables.md) lists every
variable Local Link defines, with its type and default, and the
[environment index](../../../../docs/more/reference/environment.md) also
covers libraries and tests.

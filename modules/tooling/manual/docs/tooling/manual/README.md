# Manual

`lnk man` browses the documentation tree, offline. `man` is short for
`manual`, the command's full name.

<!-- quickstart -->

## Read a Topic

Alone, `lnk man` shows the documentation's front page and lists every
topic. A path selects the same topic as the website. Flags select the
topic's reference views, and `--command <name>` reads one command's
reference.

```sh
lnk man services/boxes
lnk man services/boxes --commands
lnk man services/boxes --command "box create"
lnk man services/boxes --variables
```

## See What the Manual Shows

Installed programs carry their own pages. The documentation site's configured
front page and general guides come with the manual and work offline.
A topic that belongs to an uninstalled program shows the program's registry
entry, installation command and website address. Reading the topic installs
nothing. The core's ordinary `--help` works without the manual.

## Fix a Failed Lookup

A topic that doesn't exist, or a path that isn't a canonical
documentation path, exits with the usage code and suggests `lnk man` to
list the topics. A reference flag with no topic, or two reference flags at
once, exits with the usage code and the command's usage before anything is
read. A `--command` name that matches no command, or more than one, exits
with the usage code and suggests `--commands` to list the topic's
commands.

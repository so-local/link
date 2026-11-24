# Files

The files `lnk model` keeps, and what each one holds. Inspect the local models Link finds before editing their configuration.

```sh
lnk model list
```

## Models (`lnk model`)

The models module finds runtimes at their default ports and at the addresses you add, each time; `lnk model serve --upstream` selects another address for that server. Typed model actions read `LNK_MODEL_URL`, the granted endpoint described in [Model Actions](api.md#the-grant). [`lnk help <command>`](../../../../../../../docs/more/modules/guide.md#get-help) lists the
environment variables each command reads, as
[`module.toml`](../../../../module.toml) declares them.

### `~/Link/Safe/Models/models.toml`

Mode 600, written by `lnk model runtime add`: each added runtime's `name` and
`base_url`. The first add prepares and marks the Link folder so its
identity survives a move or a missing machine locator. Invalid addresses
and names leave this state untouched.

### `~/.lnk/data/models/context/`

Written by the model bridge: each agent's model calls, a folder of
private files for each agent, one for each chat, as [names and numbers
only](../context.md#where-its-kept).

### `~/.lnk/safe/models/facts.toml`

Yours, optional: models you describe or correct (`[model."<name>"]`:
`window`, `answer`, `thinking`, `price`), over Link's own, field by
field. `lnk model show <model>` inspects the merged figures.

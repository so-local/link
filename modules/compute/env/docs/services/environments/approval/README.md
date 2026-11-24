# Environment Approvals

> Approvals Hand Over Secrets Only on Your Yes

Manage the permission an environment file needs to receive secrets you
hold. An approval ties the file and its environment name to a secret's
stable ID. Another path, a changed environment name, or a replaced secret
needs a new approval. Edits at the same path and name keep the permission,
so review the file before you run it.

## Add an Approval

`lnk env approval add` records your permission before a non-interactive run.
The file must name the secret under `[secrets]`, and you must already hold
it. The [approval rules](approvals.md) explain how Local Link identifies a
file and how field overrides count.

```sh
lnk env approval add NPM_TOKEN --spec env.toml
```

## List Approvals

`lnk env approval list` lists your saved approvals. Add `--spec env.toml` to
limit the list to one file, or `--json` when another program needs the
records. Neither view shows secret values.

```sh
lnk env approval list --spec env.toml
```

## Show an Approval

`lnk env approval show` checks whether you approved the named secret for the
environment file. An approval is permission for a later run. It does not
show that the environment has run or used the secret.

```sh
lnk env approval show NPM_TOKEN --spec env.toml
```

## Remove an Approval

`lnk env approval remove` revokes the saved permission. The next run must ask
again before it gets that secret. A non-interactive run without an approval
refuses the secret.

```sh
lnk env approval remove NPM_TOKEN --spec env.toml
```

<!-- docs-tree:start -->

## Docs

- [Approvals](approvals.md): An environment you run is given a secret its file asks for only after your yes, kept for that file

<!-- docs-tree:end -->

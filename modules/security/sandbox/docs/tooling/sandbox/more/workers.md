# Worker Controller Maintenance

The worker installer maintains an ordinary Linux box's privileged launcher
and controller identity. Run it on a made box as the controller account;
a non-root controller needs its existing non-interactive sudo authority.

## Refresh the Launcher

After upgrading the sandbox module, the controller refreshes the root-owned
copy with the internal worker installer. It records the controller's UID,
primary GID, and supplementary groups in ordinary mode before later jobs run.

```sh
lnk sandbox workers --install
```

## Installed Records

The installer writes `/etc/lnk/workers.json` and
`/usr/local/lib/lnk/lnk-sandbox`, then installs the `lnk-workers-reap`
boot-cleanup service. Its sudo command default preserves the launcher's
explicit descriptors and PATH, and grants workers no additional sudo access.
[Worker Security](../../../../../../compute/box/docs/services/boxes/lifecycle/on-a-box.md#working-users)
explains identity isolation and cleanup that remains uncertain.

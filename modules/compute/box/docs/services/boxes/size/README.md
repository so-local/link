# Machine Sizes

Preview the machines that match a box specification before you create a box. You
need a connected cloud account. Listing matching machines creates no machine
and reserves no capacity.

## List Matching Machines

`lnk box size list` shows the machine Local Link would choose first, then
the other matches. Pass the same flags or file you plan to use when you
create the box, so the preview shows your real choice.

```sh
lnk box size list aws|gcp --machine.cpus 4 --machine.memory.min "8 GB" --machine.memory.max "16 GB"
```

## Preview a Saved Box Specification

Pass `--spec` to preview a saved box file. You can [describe a box in a file](../guide.md#describe-a-box-in-a-file),
and the [Box Specification](../../../config/specs/box.md) lists the machine fields
you may set. A preview does not promise that the provider will still have
capacity when you create the box.

```sh
lnk box size list aws|gcp --spec web.toml
```

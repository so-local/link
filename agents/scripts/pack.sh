#!/bin/sh
# Packs every ready-made agent as its release does, link-<name>.tar.gz in
# dist/, then imports each as anyone would, so a broken agent fails here
# and not on someone's machine. Needs lnk with its archive and agent
# modules: lnk module add archive agent --yes.
#
#   sh scripts/pack.sh
set -eu
cd "$(dirname "$0")/.."
rm -rf dist
mkdir dist
for manifest in ready-made/*/archive.toml; do
  agent=$(basename "$(dirname "$manifest")")
  lnk archive export --from "ready-made/$agent" --to "dist/link-$agent.tar.gz" </dev/null
done
# Imported where the modules are: a CI runner's home is thrown away, and
# each agent's own name is free there.
for archive in dist/link-*.tar.gz; do
  lnk archive import "$archive" </dev/null
done
ls -l dist

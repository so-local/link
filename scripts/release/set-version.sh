#!/usr/bin/env bash
# Set the public workspace's version and update its one lockfile offline.
# All public members inherit this version. Called by releases and staging builds.
#   scripts/release/set-version.sh <workspace> <version> [downstream workspace...]
# <workspace> is the public workspace's folder (. from it); each
# downstream workspace is a folder of its own, from where it's run.
set -euo pipefail
root=$1
version=$2
shift 2
# A version lnk reads (link_core::Version): numbers, then perhaps
# a prerelease of letters, digits, dots and dashes.
printf '%s' "$version" | grep -qE '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$' || {
    echo "set-version: $version isn't a version: 0.0.1, or 0.0.1-staging.202610131900" >&2
    exit 1
}
# A workspace's first version line, its [workspace.package] one, which
# every package inherits.
set_version() {
    awk -v version="$version" '
        !done && /^version = "[^"]*"$/ { print "version = \"" version "\""; done = 1; next }
        { print }
    ' "$1" >"$1.new"
    mv "$1.new" "$1"
}
# The workspace owns every public package's version and its one lock.
manifest=$root/Cargo.toml
[ -f "$manifest" ] || { echo "set-version: no $manifest" >&2; exit 1; }
set_version "$manifest"
(cd "$root" && cargo update --workspace --offline --quiet) || {
    echo "set-version: the public lock file cannot be updated offline; run cargo fetch first" >&2
    exit 1
}
# Each downstream workspace takes the same version, since lnk embeds a
# component only at its own version, and refreshes its lock for its path
# dependencies. Public-only callers name none.
for workspace in "$@"; do
    [ -f "$workspace/Cargo.toml" ] || {
        echo "set-version: no $workspace/Cargo.toml" >&2
        exit 1
    }
    set_version "$workspace/Cargo.toml"
    (cd "$workspace" && cargo update --workspace --offline --quiet)
done

# Artifact

`link-artifact` caches immutable downloads by SHA-256, so installers can
reuse verified bytes across modules and checkouts.

## Downloading

`Cache::shared().bytes(url, digest, max_bytes)` accepts HTTPS and a
lowercase SHA-256 digest. Both downloaded and cached bytes must match the
digest and fit the caller's size bound. A corrupt cached entry is removed
and downloaded again; an integrity failure never publishes new bytes.

## Cache settings

`LNK_ARTIFACT_CACHE` chooses the directory. Without it, the cache is in
the mapped artifacts location, `~/.lnk/safe/artifacts` by default.
`LNK_ARTIFACT_BUDGET` is a positive byte count, defaulting to 2 GiB.
Writers share a lock, publish whole files and evict older downloads
within that budget. Mutable resources and build output do not belong here.

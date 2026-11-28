# Google Cloud

The Google Cloud adapter, `lnk-gcp`, manages accounts, buckets, boxes, and
images on Google Cloud through gcloud. The `lnk cloud` and `lnk box` commands run
the adapter for you, so you never type `lnk-gcp` yourself.

<!-- docs-tree:start -->

## Docs

### Pages

- [Google Cloud](docs/services/cloud/gcp.md): `lnk-gcp` is the Google Cloud adapter: accounts, buckets, boxes and images on Google Cloud, through gcloud

### More

- [Google Cloud Credential Isolation](docs/services/cloud/gcp/security.md): Every account operation uses that account's saved credential file, or the machine's own service account for an account connected with `--machine`

<!-- docs-tree:end -->

## Code

- [`boxes.rs`](src/boxes.rs): boxes as Compute Engine instances
- [`buckets.rs`](src/buckets.rs): buckets, each with a service account of its own
- [`connect.rs`](src/connect.rs): a Google Cloud account, as one credential file
- [`images.rs`](src/images.rs): images of a box's boot disk
- [`main.rs`](src/main.rs): the adapter's commands

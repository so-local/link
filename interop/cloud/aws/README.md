# Amazon Web Services

The AWS adapter, `lnk-aws`, manages accounts, buckets, and boxes on Amazon Web
Services through the `aws` CLI. The `lnk cloud` and `lnk box` commands run the
adapter for you, so you never type `lnk-aws` yourself.

<!-- docs-tree:start -->

## Docs

- [Amazon Web Services](docs/services/cloud/aws.md): `lnk-aws` is the AWS adapter: accounts, buckets, boxes and images on Amazon Web Services, through the `aws` CLI

<!-- docs-tree:end -->

## Code

- [`boxes.rs`](src/boxes.rs): boxes as EC2 instances
- [`buckets.rs`](src/buckets.rs): buckets on S3, each with a credential of its own
- [`connect.rs`](src/connect.rs): an AWS account, connected and checked
- [`main.rs`](src/main.rs): the adapter's commands

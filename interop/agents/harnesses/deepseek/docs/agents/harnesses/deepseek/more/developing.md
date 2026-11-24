# Developing

Build and inspect the deepseek adapter from its workspace package. These commands validate local code; native runtime and provider behavior need their own qualified environment.

## Build and test

`lnk-deepseek` depends on `link-core` from the core, beside it in the public
workspace. From its own folder, run `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, then `cargo test --locked`.

## Release

Link's release builds `lnk-deepseek` from this folder and releases it as one
of Link's modules, at the core's version, signed with Link's key.

## License

MIT or Apache-2.0, at your option.

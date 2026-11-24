//! Writes the hosting commands' shapes from their declarations
//! (`types.toml`): the request and reply of each `agent-*` command.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    // The account every request carries, the cloud contract's.
    let cloud = manifest.join("../../../../cloud/cloud/contract");
    link_actions_build::write_with(
        &manifest,
        &[link_actions_build::Owner {
            directory: &cloud,
            rust: "::link_cloud_contract::declared",
        }],
        &out.join("declared.rs"),
        link_actions_build::Parts::Types,
    )
}
